---
description: "Task list for Transport Asymmetric Crypto Offload"
---

# Tasks: Transport Asymmetric Crypto Offload

**Input**: Design documents from `/specs/071-transport-crypto-offload/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/crypto-offload-contracts.md

**Tests**: INCLUDED — the spec (R8) and constitution require three-way verification (behavioral, correctness, equivalence). Behavior-preserving refactor; existing crypto/conformance tests are the equivalence net.

**Organization**: By user story. US1 (server-side offload = the DoS lever) is the MVP. US2 (no-wire-change) is a verification story over US1's changes. US3 (containment + client offload) is P2.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different file, no dependency on incomplete task)
- **[Story]**: US1 / US2 / US3 (setup/foundational/polish carry no story label)
- Every task that touches a wire-relevant path cites the OPC UA Part/§ it must preserve.

---

## Phase 1: Setup (baseline)

**Purpose**: Establish the byte-identical / green-suite baseline this feature must not regress.

- [ ] T001 Capture the pre-change equivalence baseline: run `cargo test -p async-opcua --test integration_tests` (conformance smoke matrix) and `cargo test --workspace`, record green status, and note the conformance matrix output as the byte-identical wire baseline (SC-002/SC-004 guard). No code change.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Extract the owned-input crypto cores so any site can run them under `spawn_blocking`. **BLOCKS all user stories** — nothing can offload until the cores exist.

**⚠️ CRITICAL**: No offload-site work (US1/US3) may begin until T002–T004 are complete.

- [ ] T002 Extract `asymmetric_sign_and_encrypt_owned(security_policy, signing_key: PrivateKey, encryption_key: Option<PublicKey>, src: Vec<u8>, encrypted_range) -> Result<Vec<u8>, StatusCode>` as a free function in `async-opcua-core/src/comms/secure_channel.rs`, using a local scratch `Vec` instead of the `PADDING_AND_SIGNATURE_SCRATCH` thread-local (G1 `Send + 'static`, G3 panic-free); rewire the existing `&self` `asymmetric_sign_and_encrypt` (~:1267) as a thin wrapper (clone material → call core). Byte-identical output — OPC-10000-6 §6.7.2 (chunk message security).
- [ ] T003 Extract `asymmetric_decrypt_and_verify_owned(security_policy, our_private_key: PrivateKey, our_cert: X509, verification_key: PublicKey, receiver_thumbprint: ByteString, src: Vec<u8>, encrypted_range) -> Result<Vec<u8>, Error>` as a free function in `async-opcua-core/src/comms/secure_channel.rs` (G1/G3); rewire the `&self` `asymmetric_decrypt_and_verify` (~:1497) as a thin wrapper. OPC-10000-6 §6.7.2.
- [ ] T004 [P] Unit tests for both cores in `async-opcua-core/src/comms/secure_channel.rs` (`#[cfg(test)]`): sign→verify / encrypt→decrypt round-trip per policy, and route the existing secure-channel crypto regression vectors through the free functions to prove G2 equivalence (identical output to the `&self` methods).

**Checkpoint**: Owned-input cores exist, `&self` wrappers unchanged for sync callers, existing crypto tests green. Offload sites can now begin.

---

## Phase 3: User Story 1 - Established sessions survive a handshake storm (Priority: P1) 🎯 MVP

**Goal**: Move all *server-side* OSC/CreateSession asymmetric crypto off the request-processing threads so a pre-auth handshake storm cannot starve established sessions.

**Independent Test**: Under ≥50 concurrent channel-opening clients, an already-established session issuing periodic reads keeps bounded latency (does not scale with handshake load); and a handshake completes on a `current_thread` runtime whose sole worker is busy.

### Implementation for User Story 1

- [ ] T005 [US1] Site 1 (the DoS lever, **pre-auth**): offload the server inbound OSC decrypt+verify onto `spawn_blocking` in `async-opcua-server/src/transport/tcp.rs` — the `SecureChannel::verify_and_remove_security_server` (`secure_channel.rs:1220`) path calling `asymmetric_decrypt_and_verify_owned`; extract owned inputs (brief borrow, no lock across `.await`), unwrap the inner crypto `Result` first, and add a distinct `JoinError` arm that drops the connection with a transport fault without masking a specific crypto fault (C2/R6). OPC-10000-4 §5.6.2 (OpenSecureChannel Service); message security OPC-10000-6 §6.7.2.
- [ ] T006 [US1] Site 2: offload the server outbound OSC sign+encrypt onto `spawn_blocking` at the OSC-encode seam — `SendBuffer::encode_next_chunk` (`async-opcua-core/src/comms/buffer.rs:164`) async path driven by `async-opcua-server/src/transport/tcp.rs` send; the transport MUST `.await` the offloaded chunk before emitting the next, preserving the monotonic per-`MessageChunk` sequence number (OPC-10000-6 §6.7.2.4 Sequence Header; C3/R5). No dedicated reorder test is needed — OSC is a single `MessageChunk` per channel establishment, so reordering is structurally impossible; the existing chunk/sequence tests guard the general path.
- [ ] T007 [US1] Site 4a: offload CreateSession server-signature RSA signing onto `spawn_blocking` in `async-opcua-server/src/session/controller.rs` (`CreateSessionDraft::prepare_endpoint_preflight` ~:565 — already outside the session-manager write lock). OPC-10000-4 §5.7.2 (CreateSession Service — serverSignature over clientCertificate + clientNonce).
- [ ] T008 [US1] Site 4b: offload ECC ephemeral-key generation onto `spawn_blocking` in `async-opcua-server/src/session/manager.rs` (`issue_server_ephemeral_key` ~:357 and the renew site ~:1539). OPC-10000-6 §6.8.2 (ECC EphemeralKey returned in the CreateSession response for UserIdentityToken encryption) + OPC-10000-4 §5.7.2 (CreateSession Service) / §7.15 (EphemeralKeyType).

### Tests for User Story 1

- [ ] T009 [US1] Structural proof test (load-bearing, C6) in `async-opcua-server/tests/`: an OSC handshake on `#[tokio::test(flavor = "current_thread")]` while the single worker is kept busy — it can only complete if the crypto runs on the blocking pool. Distinguishes "actually offloaded" from "merely refactored".
- [ ] T010 [US1] `#[ignore]`'d 50-client handshake-storm test (SC-001) in `async-opcua-server/tests/`: ≥50 concurrent channel-openers while one established session issues reads; assert the established session's p99 read latency **under the storm is ≤ 2× its p99 measured with no concurrent handshakes** — a fixed relative bound (not an absolute latency), so it cannot scale with handshake count. Manual, `taskset -c <core>` per repo benchmarking convention.

**Checkpoint**: Server-side crypto is off the request threads; the DoS lever is closed. MVP — stop and validate here.

---

## Phase 4: User Story 2 - No observable protocol change (Priority: P1)

**Goal**: Prove the offload changed *where* crypto runs, not *what* goes on the wire.

**Note**: US2 is a *verification gate over US1*, not a standalone increment — it has no implementation of its own and cannot be exercised until US1's server offloads (T005–T008) are in place. It stays P1 because byte-identical wire is a release gate, not a nice-to-have.

**Independent Test**: The full security-policy × mode × identity-token conformance matrix passes unchanged, and handshake/session message bytes match the T001 baseline.

### Tests for User Story 2

- [ ] T011 [P] [US2] All-policies handshake + session correctness test in `async-opcua-server/tests/`: RSA `Basic256Sha256` + `Aes256Sha256RsaPss` and ECC `NistP256`/`NistP384`, each in `Sign` and `SignAndEncrypt`, all establish a channel and activate a session (SC-005, FR-009).
- [ ] T012 [US2] Equivalence guard: confirm `async-opcua/tests/integration/conformance.rs` passes with no diff vs the T001 baseline (SC-002), and verify byte-identical handshake/session wire output for identical inputs (SC-004). OPC-10000-6 §6.7.2 (message security) + OPC-10000-4 §5.6.2/§5.7.2 (the OSC/Session service bytes that must not change).

**Checkpoint**: No conformant peer can tell the change happened.

---

## Phase 5: User Story 3 - Crypto failures stay contained (Priority: P2)

**Goal**: Contain the new failure mode (an offloaded unit dying) to a single connection, and include the client-side offload for symmetry (not a DoS lever — R7).

### Implementation for User Story 3

- [ ] T013 [US3] Site 3: offload the client OSC sign+encrypt + response decrypt onto `spawn_blocking` in `async-opcua-client/src/transport/stream.rs` (~:286 `encode_next_chunk` + inbound verify), reusing the owned-input cores and the same `JoinError` arm (FR-008). OPC-10000-4 §5.6.2 (OpenSecureChannel Service) / OPC-10000-6 §6.7.2 (message security — byte-identical). Lower priority — client opens one channel.

### Tests for User Story 3

- [ ] T014 [US3] Crypto-failure containment test in `async-opcua-server/tests/`: an offloaded crypto unit failing drops only its own connection (the server keeps serving other sessions), and a specific crypto fault (e.g. `BadSecurityChecksFailed` / `BadCertificateInvalid`) surfaces unchanged rather than being masked by a generic `JoinError`-mapped error (FR-005, C4).

**Checkpoint**: The offload's new failure mode is contained; client runtime no longer blocked by its own handshake crypto.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T015 [P] Verify the symmetric per-request path is untouched (C5/FR-007): the hot-path lock/callback tests (`async-opcua-server/tests/hot_path_*`) stay green, and confirm by inspection that no `spawn_blocking` was introduced on the symmetric (`else`) branch of `apply_security` / `verify_and_remove_security`.
- [ ] T016 Full pre-PR gate: `tools/ci-playbook.sh --ci` (incl. `clippy --workspace --all-targets --all-features` and the `-Dwarnings` no-default legs); confirm by inspection that the offload closures log no key material (Principle IV — the cloned `PrivateKey` lives transiently on the blocking thread and drops at closure end per R10); then retire the T086–T090 deferral note in feature-070's records and update `research.md`/memory to mark 071 done.

---

## Dependencies & Execution Order

- **Setup (T001)**: no dependencies.
- **Foundational (T002–T004)**: after T001; **BLOCKS all offload sites**. T002 and T003 are the same file (sequential); T004 after both.
- **US1 (T005–T010)**: after Foundational. T005–T008 are the four offload sites (largely independent files: tcp.rs, buffer.rs+tcp.rs, controller.rs, manager.rs — but T005/T006 both touch tcp.rs → sequential). Tests T009/T010 after the sites they exercise.
- **US2 (T011–T012)**: after US1's server sites land (it verifies them). Independently testable.
- **US3 (T013–T014)**: after Foundational; T013 (client) is independent of the server sites; T014 exercises the JoinError arm added in US1.
- **Polish (T015–T016)**: after all desired stories.

### Within each story
- Behavior-preserving refactor: the existing crypto + conformance tests are the standing net; new tests (T004, T009–T012, T014) prove the offload actually happened and stayed equivalent.
- Commit at the end of each user story (not per finding), per project cadence.

### Parallel opportunities
- T004 [P] alongside finalizing T002/T003 review.
- T011 [P] independent of T012 wiring.
- T015 [P] independent of the T016 gate.
- T005–T008 are mostly different files; only T005/T006 (both tcp.rs) must serialize.

---

## Implementation Strategy

### MVP (User Story 1 only)
1. T001 baseline → T002–T004 cores → T005–T008 server offloads → T009 structural proof.
2. **STOP and VALIDATE**: the DoS lever (T005, pre-auth inbound decrypt) is the core value; confirm the structural proof + storm test.

### Incremental delivery
1. Setup + Foundational → cores ready.
2. US1 → server crypto off the request threads (MVP: DoS protection).
3. US2 → prove byte-identical wire (release gate).
4. US3 → contain the failure mode + client offload for completeness.

---

## Notes
- One task per codex dispatch (do not batch); each wire-touching task carries its OPC UA Part/§.
- Byte-identical wire is the release gate — if any site cannot offload without changing output, document it and leave that site inline rather than half-offloading (constitution I).
- `spawn_blocking` is Tokio's sanctioned offload, not a lock (AGENTS.md); the symmetric hot path must gain no off-thread dispatch.
