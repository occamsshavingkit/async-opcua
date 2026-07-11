# Phase 0 Research: Transport Asymmetric Crypto Offload

All items resolved — no open `NEEDS CLARIFICATION`. The approach was settled in a prior
brainstorm; this records the decisions with code grounding for the tasks phase.

## R1. Approach selection — "thin offload seam" (Approach A)

**Decision**: Extract the bodies of `SecureChannel::asymmetric_sign_and_encrypt` and
`asymmetric_decrypt_and_verify` into free functions taking owned key material + owned buffers;
keep the `&self` methods as thin wrappers; `spawn_blocking` only the OSC/asymmetric branch in
the async layers.

**Rationale**: The asymmetric path is already a *branch* inside `apply_security`
(`secure_channel.rs:905`: `if is_open_secure_channel { asymmetric… } else { symmetric… }`). Only
that leaf needs to leave the async thread. The symmetric per-request path is fast and stays put.

**Alternatives considered**:
- **B — `Send` crypto-context bundle** (clone all crypto state into one `AsymmetricCryptoContext`):
  more surface (a new type to keep in sync with `SecureChannel`), same result. Rejected.
- **C — offload the whole `apply_security`/`verify_and_remove_security` for OSC**: `SecureChannel`
  isn't cheaply `Send`-cloneable (thread-local + borrowed refs), and the server holds
  `&mut SecureChannel` (can't move into a `'static` closure) — this is the invasive
  "transport-layer restructuring" feature 070 balked at. Rejected.

## R2. Why the current code can't be wrapped as-is

`asymmetric_sign_and_encrypt(&self, …, dst: &mut [u8])` (`secure_channel.rs:1267`) reads
`self.private_key` (`:1276`) and `self.remote_cert.public_key()` (`:1295`); `apply_security`
(`:874`) runs it inside `PADDING_AND_SIGNATURE_SCRATCH.with(|scratch| …)` (`:893`), a
`thread_local! RefCell<Vec<u8>>`. `asymmetric_decrypt_and_verify(&self, …)` (`:1497`) reads
`self.cert` (`:1525`) and `self.private_key`. A `spawn_blocking` closure must be `'static + Send`
— it cannot borrow `self`, `dst`, or touch a thread-local. Hence owned inputs + a local scratch.

## R3. Owned key material is cheap

`PrivateKey` derives `Clone` (`async-opcua-crypto/src/aes/rsa_private_key.rs:78`). `X509` and
`PublicKey` are cloneable/derivable from the certs already in hand at each call site. So
"extract owned material" is a clone of small structs, done once per handshake (infrequent). The
OSC local scratch `Vec` replaces the thread-local only on this path — a per-handshake allocation,
negligible vs the RSA op itself.

## R4. The four offload sites (grounded)

| # | Operation | File:fn | Direction | Pre-auth |
|---|-----------|---------|-----------|----------|
| 1 | OSC decrypt+verify | `async-opcua-server/src/transport/tcp.rs` → `SecureChannel::verify_and_remove_security_server` (`secure_channel.rs:1220`) → `asymmetric_decrypt_and_verify` (`:1497`) | server ← client | **yes** |
| 2 | OSC sign+encrypt | server `transport/tcp.rs` send path → `SendBuffer::encode_next_chunk` (`buffer.rs:164`) → `apply_security` (`:874`) → `asymmetric_sign_and_encrypt` (`:1267`) | server → client | no |
| 3 | Client OSC sign+encrypt + response decrypt | `async-opcua-client/src/transport/stream.rs:286` (`encode_next_chunk`) + inbound verify | client ↔ server | no |
| 4a | CreateSession server-signature RSA sign | `async-opcua-server/src/session/controller.rs` `CreateSessionDraft::prepare_endpoint_preflight` (~`:565`) | server | no |
| 4b | ECC ephemeral keygen | `async-opcua-server/src/session/manager.rs` `issue_server_ephemeral_key` (`:357`, and renew at `:1539`) | server | no |

Site 4 is already at an async point *before* the session-manager write lock (the draft is
prepared outside the lock), so it is the cleanest place to add a `spawn_blocking` boundary.

## R5. Invariant 1 — chunk sequence ordering (OPC-10000-6 §6.7.2)

**Finding**: the sequence number is stamped into the chunk plaintext (the signed Sequence Header)
*upstream* of the crypto, and `SendBuffer::encode_next_chunk(&mut self, …)` (`buffer.rs:164`) is
the serialization point — it pops one chunk and calls `apply_security` on an already-stamped
chunk. OSC is a single handshake message per channel establishment with no other chunks competing
on that channel. **Therefore offloading the crypto cannot reorder chunks**; the design only needs
to `.await` the offloaded chunk before processing the next (naturally satisfied). No sequence
logic changes.

## R6. Invariant 2 — status-code fidelity + JoinError

**Decision**: the offload returns the inner `Result<_, StatusCode|Error>` verbatim, so every
existing fault (`BadSecurityChecksFailed`, `BadCertificateInvalid`, `BadSecurityPolicyRejected`,
…) reaches the client unchanged. `spawn_blocking(...).await` yields `Result<T, JoinError>`;
`JoinError` occurs only on task panic/cancel. Map panic → drop the connection with a generic
transport-level fault (`BadCommunicationError`/`BadInternalError`) **without** overriding a more
specific inner crypto fault (the inner `Result` is unwrapped first; `JoinError` is a separate
arm). The crypto cores are panic-free (`async-opcua-crypto` and the secure-channel boundary deny
`unwrap`/`panic` outside tests and were DoS-panic-hardened previously), so `JoinError` should be
unreachable in practice — but it is handled, not `unwrap`ed (constitution I/IV).

## R7. Client-side offload value (site 3)

Client-side handshake crypto is *not* a DoS lever (a client opens one channel). The value is that
a client application sharing its async runtime with other work isn't blocked by its own handshake
crypto. Once the crypto cores are owned-input free functions, offloading the client OSC path is
nearly free, so it is included for symmetry and completeness (FR-008). Noted as lower-priority
than site 1.

## R8. Test strategy

- **Structural proof** (behavioral): run an OSC handshake on a `#[tokio::test(flavor =
  "current_thread")]` runtime while the single worker is kept busy; the handshake can only
  complete if the crypto runs on the blocking pool. This is the load-bearing proof that the
  offload actually happened (not just that the refactor compiles).
- **All-policies correctness**: handshake + session across RSA `Basic256Sha256` /
  `Aes256Sha256RsaPss` and ECC `NistP256`/`NistP384`, in `Sign` and `SignAndEncrypt`.
- **Equivalence**: `async-opcua/tests/integration/conformance.rs` stays green unchanged — the
  no-wire-change proof (SC-002/SC-004).
- **Unit**: the extracted crypto cores get direct unit tests (round-trip + the existing
  `secure_channel.rs` regression vectors, now callable on the free functions).
- **Storm** (`#[ignore]`'d, manual): ≥50 clients opening channels concurrently while one
  established session issues reads; assert the established session's p99 latency stays bounded
  vs a baseline. Run with `taskset -c <core>` per repo benchmarking convention.

## R9. No new dependencies; T091 already done

`spawn_blocking` is from `tokio` (already a dependency). The blocking-pool size knob
(`max_blocking_threads` builder method, feature-070 T091) already exists — out of scope here.
No `cargo deny` impact.

## R10. Zeroize posture (recorded, out of scope)

Cloning `PrivateKey` into the offload closure spreads key material to a second transient location
that drops at closure end. This is no worse than the existing `Clone` usage of `PrivateKey`
elsewhere. Whether `PrivateKey::drop` zeroizes is a pre-existing question, not introduced by this
feature; if a gap exists it is tracked separately (see memory `zeroize-audit`), not expanded into
here.
