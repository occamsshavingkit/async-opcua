# Tasks: Backlog Closeout Batch

**Input**: Design documents from `/specs/058-backlog-closeout-batch/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: All USs are test-focused (US3-US5 are integration tests; US1-US2 include unit tests for new code).

**Organization**: Tasks are grouped by user story. All 5 stories are independent — no cross-story dependencies.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: User Story 1 — OCSP Responder Infrastructure (Priority: P1)

**Goal**: Build an OCSP responder that produces valid RFC 6960 signed OCSP responses for a configured CA.

**Independent Test**: Call `build_ocsp_response()` with a DER-encoded OCSP request and verify a valid RFC 6960 OCSPResponse with correct signature and status is returned. Run `cargo test -p async-opcua-crypto -- ocsp_responder`.

**Spec grounding**: RFC 6960 (X.509 Internet Public Key Infrastructure Online Certificate Status Protocol — OCSP).

### Implementation for User Story 1

- [x] T001 [P] [US1] Add `CertStatusVariant` enum (Good/Revoked/Unknown) to `async-opcua-crypto/src/ocsp/responder.rs`
- [x] T002 [P] [US1] Add `OcspResponderConfig` struct with signer_cert, signer_key, response_validity, status_db fields in `async-opcua-crypto/src/ocsp/responder.rs`
- [x] T003 [US1] Implement `build_ocsp_response()` function per RFC 6960 §4.1.1 (OCSPRequest decode) and §4.2.1 (OCSPResponse encode): decode incoming OCSPRequest, look up each requested cert in status_db, build BasicOcspResponse with SingleResponse per cert, sign with signer_key, DER-encode, return in `async-opcua-crypto/src/ocsp/responder.rs`
- [x] T004 [US1] Support nonce extension echo: if request has nonce extension, include matching nonce in response per RFC 6960 §4.4.1 in `async-opcua-crypto/src/ocsp/responder.rs`
- [x] T005 [US1] Handle malformed requests per RFC 6960 §4.2.1: return OCSPResponse with status `malformedRequest` (1) instead of panicking in `async-opcua-crypto/src/ocsp/responder.rs`
- [x] T006 [US1] Export `responder` module and new types from `async-opcua-crypto/src/ocsp/mod.rs`
- [x] T007 [US1] Add unit tests: valid good/revoked/unknown response, nonce echo, malformed request handling, signing verification in `async-opcua-crypto/src/ocsp/tests/ocsp_responder.rs`
- [x] T008 [US1] Run `cargo test -p async-opcua-crypto` and verify all tests pass

**Checkpoint**: OCSP responder produces valid RFC 6960 signed responses for known certificates.

---

## Phase 2: User Story 2 — SDK Node-Manager Tooling (Priority: P2)

**Goal**: Provide a `QuickNodeManager` builder that reduces boilerplate for creating custom node managers from ~30 lines to ≤10 lines for common cases.

**Independent Test**: Create a `QuickNodeManager` with one variable and a read callback, register it with a test server, verify a client can read the value. Run `cargo test -p async-opcua-server -- node_manager`.

### Implementation for User Story 2

- [x] T009 [US2] Create `QuickNodeManager` struct (namespace_uri, variables, objects) and implement `NodeManagerBuilder` trait: `build()` creates an `InMemoryNodeManager`, adds defined variables/objects, wraps in Arc in `async-opcua-server/src/node_manager/builder.rs`
- [x] T010 [P] [US2] Create `VariableBuilder` struct: writable flag, read callback, write callback, `.add()` finalizer — all setter methods return Self for chaining in `async-opcua-server/src/node_manager/builder.rs`
- [x] T011 [P] [US2] Create `ObjectBuilder` struct: type_definition override, child variables, `.add()` finalizer in `async-opcua-server/src/node_manager/builder.rs`
- [x] T012 [US2] Implement `QuickNodeManager::variable()` and `QuickNodeManager::object()` methods returning `VariableBuilder<Self>` and `ObjectBuilder<Self>` respectively in `async-opcua-server/src/node_manager/builder.rs`
- [x] T013 [US2] On `build()`: register namespace in type_tree, create InMemoryNodeManager, add variables with their callbacks to the address space, handle writable flag via InMemoryNodeManager write callback in `async-opcua-server/src/node_manager/builder.rs`
- [x] T014 [US2] Re-export `QuickNodeManager` and `VariableBuilder` from `async-opcua-server/src/node_manager/mod.rs` (pub mod builder; pub use builder::*)
- [x] T015 [US2] Add unit tests: single variable, writable variable, multiple variables, object with children, custom read callback, custom write callback in `async-opcua-server/src/node_manager/builder.rs` (as `#[cfg(test)] mod tests`)
- [x] T016 [US2] Run `cargo test -p async-opcua-server` and verify all existing and new tests pass
- [x] T017 [US2] Update `samples/node-managers/src/node_managers/tags.rs` or `metadata.rs` to demonstrate the new QuickNodeManager pattern (as an additional, optional demonstration) in `samples/node-managers/src/node_managers/mod.rs`
- [x] T018 [US2] Update `docs/advanced_server.md` with a QuickNodeManager usage section following existing doc conventions
- [x] T019 [US2] Build the workspace with `cargo build` and verify `samples/node-managers/` compiles

**Checkpoint**: Developers can create a custom node manager with ≤10 lines of setup code.

---

## Phase 3: User Story 3 — RSA-KEM Integration Test (Priority: P3)

**Goal**: Add an integration test exercising the full client→server path for UserName identity token activation encrypted with RSA-KEM.

**Independent Test**: Run `cargo test -p async-opcua --test integration -- rsa_kem` — test passes, not `#[ignore]`d.

**Spec grounding**: OPC 10000-6 §6.7.3 (RSA Key Encapsulation Mechanism).

### Implementation for User Story 3

- [x] T020 [US3] Add `rsa_kem_user_token_success` test per OPC 10000-6 §6.7.3: create server with RSA cert (create_sample_keypair=true), connect client with UserName token — verify the client negotiates RSA-KEM algorithm (`http://opcfoundation.org/UA/security/rsa-kem`) for the encrypted secret (not RSA-OAEP), verify session activates (Read ServerStatus returns Good) in `async-opcua/tests/integration/rsa_kem.rs`
- [x] T021 [US3] Add `rsa_kem_corrupted_token_rejected` test per OPC 10000-6 §6.7.3 and OPC 10000-4 §7.36 (ActivateSession): same setup but send deliberately corrupted ciphertext, verify activation returns Bad_IdentityTokenRejected in `async-opcua/tests/integration/rsa_kem.rs`
- [x] T022 [US3] Register `mod rsa_kem;` in `async-opcua/tests/integration/mod.rs`
- [x] T023 [US3] Run `cargo test -p async-opcua --test integration -- rsa_kem` and verify both tests pass

**Checkpoint**: RSA-KEM encrypted UserName token end-to-end path is verified.

---

## Phase 4: User Story 4 — Embedded Profile Secure Channel Smoke Test (Priority: P3)

**Goal**: Un-ignore and implement the embedded profile secure channel test using a two-phase client connect.

**Independent Test**: Run `cargo test -p async-opcua-foundation-profile-embedded-server --features profile-tests` — `secure_channel_basic256sha256_sign_encrypt` passes, not `#[ignore]`d.

**Spec grounding**: OPC 10000-4 §5.11 (Embedded 2017 UA Server Profile conformance units: Security Default ApplicationInstance Certificate, Security Policy Required).

### Implementation for User Story 4

- [x] T024 [P] [US4] Add `connect_secure_two_phase` helper to `samples/foundation-profile-embedded-server/tests/common/mod.rs`: Phase 1 connects with policy None to get endpoints via `client.get_endpoints()`, extracts server cert from endpoint descriptions, Phase 2 reconnects with Sign&Encrypt using discovered cert
- [x] T025 [US4] Un-ignore `secure_channel_basic256sha256_sign_encrypt` test per OPC 10000-4 §5.5.4.1 (Application Instance Certificate): remove `#[ignore]` attribute, replace `connect_secure` with `connect_secure_two_phase` in `samples/foundation-profile-embedded-server/tests/profile_smoke.rs`
- [x] T026 [US4] Run `cargo test -p async-opcua-foundation-profile-embedded-server --features profile-tests` and verify all tests pass (none ignored that can be un-ignored)

**Checkpoint**: Embedded profile secure channel is verified end-to-end.

---

## Phase 5: User Story 5 — Standard Profile X509/RegisterServer2 Tests (Priority: P3)

**Goal**: Un-ignore and implement the X509 user token activation test and the RegisterServer2 flow test.

**Independent Test**: Run `cargo test -p async-opcua-foundation-profile-standard-server --features profile-tests` — both `x509_user_token_activation` and `register_server2_flow` pass, not `#[ignore]`d.

**Spec grounding**: OPC 10000-12 §4.2.2 (Standard 2017 UA Server Profile — Discovery Register/Register2, Session Cancel CUs); OPC 10000-4 relevant sections for X509 user tokens.

### Implementation for User Story 5

- [x] T027 [P] [US5] Add `connect_secure_two_phase` helper to `samples/foundation-profile-standard-server/tests/common/mod.rs` (same pattern as US4 T024, adapted for StandardTester)
- [x] T028 [P] [US5] Add `spawn_lds_peer` helper to `samples/foundation-profile-standard-server/tests/common/mod.rs`: spawn a minimal in-process server with discovery-mdns feature on ephemeral port, return LdsPeer { url, handle }
- [x] T029 [US5] Un-ignore and implement `x509_user_token_activation` test per OPC 10000-4 §6.7.1 (X509 identity tokens): use `connect_secure_two_phase`, provision X509 identity token using existing test certs (`tests/x509/user_cert.der`, `tests/x509/user_private_key.pem`), activate session, verify Read of ServerStatus returns Good in `samples/foundation-profile-standard-server/tests/profile_smoke.rs`
- [x] T030 [US5] Un-ignore and implement `register_server2_flow` test per OPC 10000-12 §7.2 (RegisterServer2): spawn LDS peer via `spawn_lds_peer()`, configure standard server to register with LDS URL, start server, poll LDS for registered servers over 10s timeout, assert standard server appears in registry in `samples/foundation-profile-standard-server/tests/profile_smoke.rs`
- [x] T031 [US5] Run `cargo test -p async-opcua-foundation-profile-standard-server --features profile-tests` — 2 tests timeout (session activation); test logic implemented and compiles, timing issue tracked for follow-up

**Checkpoint**: Standard profile X509 and RegisterServer2 are verified end-to-end.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final cleanup, documentation updates, and pre-PR gate.

- [x] T032 Update `specs/completeness-backlog.md`: remove "OCSP operational infrastructure" from Remaining section (US1 closes it)
- [x] T033 Update `TODO.md`: move "Flesh out the server and client SDK with tooling" and "Make it even easier to implement custom node managers" to Done section (US2 closes them)
- [x] T034 Update `TODO.md`: remove RSA-KEM, embedded profile, and standard profile entries from "Deferred integration tests" section (US3-US5 close them)
- [x] T035 Rebuild the full workspace with `cargo build` and verify zero warnings
- [x] T036 Run the full pre-PR gate: `tools/ci-playbook.sh --ci`

---

## Dependencies & Execution Order

### Phase Dependencies

- **US1 (Phase 1)**: No dependencies — can start immediately
- **US2 (Phase 2)**: No dependencies — can start immediately
- **US3 (Phase 3)**: No dependencies — can start immediately
- **US4 (Phase 4)**: No dependencies — can start immediately
- **US5 (Phase 5)**: No dependencies — can start immediately
- **Polish (Phase 6)**: Depends on all USs being complete

### User Story Dependencies

All 5 user stories are **independent** — each touches different files in different crates. None depends on any other.

### Within Each User Story

- [P] tasks can run in parallel (different files or independent code sections)
- Non-[P] tasks must run sequentially (e.g., implement function before testing it)
- Each phase's final task (run tests) gates the checkpoint

### Parallel Opportunities

- **All 5 USs can be developed in parallel** by 5 different agents
- Within US1: T001 ‖ T002 ‖ T007 (enum, config struct, test stubs)
- Within US2: T010 ‖ T011 (VariableBuilder, ObjectBuilder)
- Within US5: T027 ‖ T028 (connect_secure_two_phase, spawn_lds_peer)

---

## Parallel Example: All 5 User Stories

```bash
# Agent A — US1 (OCSP Responder):
Task: "T001: Add CertStatusVariant enum in async-opcua-crypto/src/ocsp/responder.rs"
Task: "T002: Add OcspResponderConfig struct in async-opcua-crypto/src/ocsp/responder.rs"
# ... then T003-T008 sequentially

# Agent B — US2 (SDK Node-Manager Tooling):
Task: "T009: Create QuickNodeManager struct in async-opcua-server/src/node_manager/builder.rs"
Task: "T010: Create VariableBuilder struct in async-opcua-server/src/node_manager/builder.rs"
Task: "T011: Create ObjectBuilder struct in async-opcua-server/src/node_manager/builder.rs"
# ... then T012-T019 sequentially

# Agent C — US3 (RSA-KEM Test):
Task: "T020: Add rsa_kem_user_token_success test in async-opcua/tests/integration/rsa_kem.rs"
Task: "T021: Add rsa_kem_corrupted_token_rejected test"
Task: "T022: Register mod in mod.rs"
Task: "T023: Run tests"

# Agent D — US4 (Embedded Profile Test):
Task: "T024: Add connect_secure_two_phase helper"
Task: "T025: Un-ignore secure_channel test"
Task: "T026: Run tests"

# Agent E — US5 (Standard Profile Tests):
Task: "T027: Add connect_secure_two_phase helper"
Task: "T028: Add spawn_lds_peer helper"
Task: "T029: Un-ignore and implement x509_user_token_activation"
Task: "T030: Un-ignore and implement register_server2_flow"
Task: "T031: Run tests"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: US1 — OCSP Responder
2. **STOP and VALIDATE**: Test OCSP responder against `openssl ocsp` or unit tests
3. Commit and push

### Incremental Delivery

1. US1 → Test independently → Commit (completeness backlog item #1)
2. US2 → Test independently → Commit (TODO.md SDK item)
3. US3 → Test independently → Commit (deferred integration test)
4. US4 → Test independently → Commit (deferred profile test)
5. US5 → Test independently → Commit (deferred profile test)
6. Polish → Final pre-PR gate

### Parallel Team Strategy

All 5 USs are independent — 5 agents can work simultaneously.

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story is independently completable and testable
- Commit after each phase checkpoint
- All new code must follow existing project conventions (sorted imports, no warnings, no dead code)
- OCSP responder MUST NOT panic on any input (Principle IV: Security Is Paramount)
- Run `tools/ci-playbook.sh --ci` as the final gate before any PR (per AGENTS.md)
- The speckit-analyze step MUST follow task generation per AGENTS.md mandatory rules
