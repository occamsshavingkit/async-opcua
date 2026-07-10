# Contracts: Transport Asymmetric Crypto Offload

Internal contracts (crate-private). The "interface" this feature exposes is (a) the extracted
crypto-core functions and (b) the behavioral guarantees the offload must uphold. No external/wire
contract changes — that is the whole point (SC-004).

## C1. Crypto-core function contract

Two owned-input cores in `async-opcua-core/src/comms/secure_channel.rs`:

```
asymmetric_sign_and_encrypt_owned(
    security_policy: SecurityPolicy,
    signing_key:     PrivateKey,
    encryption_key:  Option<PublicKey>,
    src:             Vec<u8>,
    encrypted_range: Range<usize>,
) -> Result<Vec<u8>, StatusCode>

asymmetric_decrypt_and_verify_owned(
    security_policy:     SecurityPolicy,
    our_private_key:     PrivateKey,
    our_cert:            X509,
    verification_key:    PublicKey,
    receiver_thumbprint: ByteString,
    src:                 Vec<u8>,
    encrypted_range:     Range<usize>,
) -> Result<Vec<u8>, Error>
```

**Guarantees**:
- **G1 (Send + 'static)**: no borrow of `SecureChannel`, no thread-local, no I/O — callable
  inside `spawn_blocking`.
- **G2 (equivalence)**: for identical inputs, produces byte-identical output to the current
  `&self` methods. (Proven by routing the `&self` wrappers through the cores and keeping the
  existing `secure_channel.rs` crypto regression tests green.)
- **G3 (panic-free)**: no reachable `unwrap`/`expect`/indexing panic; all fallible steps return
  `Err(StatusCode|Error)`.

## C2. Offload-site contract (all 4 sites)

Each site, in its async context, MUST:
1. Extract owned inputs by cloning from the channel/session (brief borrow, no lock held across
   `.await`).
2. Run the core via `spawn_blocking`.
3. On `Ok(inner)`: use `inner` (the crypto `Result`) exactly as the inline code did — same success
   handling, same error propagation for `Err(fault)`.
4. On `JoinError` (task panicked/cancelled): drop the connection with a transport-level fault;
   MUST NOT convert a specific inner crypto `Err` into a generic error (the inner `Result` is
   consumed first; `JoinError` is a distinct arm).

**Applies to**: (1) server inbound OSC decrypt, (2) server outbound OSC sign+encrypt, (3) client
OSC sign+encrypt + response decrypt, (4) CreateSession server-signature signing + ECC keygen.

## C3. Invariant — sequence ordering (OPC-10000-6 §6.7.2)

- The sequence number is assigned to the chunk plaintext **before** the crypto core runs (at
  `SendBuffer::encode_next_chunk`), so offloading does not touch sequence assignment.
- Only OpenSecureChannel chunks are offloaded; OSC is a single message per channel establishment.
- The transport MUST `.await` the offloaded chunk before emitting/processing the next, so no
  chunk can be written out of sequence order.
- **Test**: existing chunk/sequence tests stay green; the all-policies handshake test exercises
  multi-chunk-free OSC exchange.

## C4. Invariant — status-code fidelity (OPC-10000-4/§6)

- The inner crypto `Result` is returned verbatim: `BadSecurityChecksFailed`,
  `BadCertificateInvalid`, `BadSecurityPolicyRejected`, and any other existing fault reach the
  peer unchanged.
- **Test**: an untrusted/malformed handshake input yields the same fault status code as before
  (assert exact `StatusCode`).

## C5. Invariant — symmetric path untouched (FR-007)

- The per-request symmetric encrypt/sign/verify path MUST NOT enter any `spawn_blocking`; only
  the `is_open_secure_channel` / asymmetric branch is offloaded.
- **Test**: a normal Read/Write round-trip incurs no new off-thread dispatch (verified by the
  hot-path lock/callback tests staying green and by inspection).

## C6. Structural contract — offload actually happens

- **Test (load-bearing)**: an OSC handshake completes on a `current_thread` runtime whose single
  worker is kept busy — impossible unless the crypto runs on the blocking pool. This distinguishes
  "refactored into cores" from "actually offloaded".
