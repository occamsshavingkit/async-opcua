# Tasks: Completeness Closeout

**Input**: Design documents from `/specs/057-completeness-closeout/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Existing test suites serve as regression safety net. No new test tasks — all four USs are verified by the existing 443 tests (core 89, server 306, nodes 48) plus compilation of new example crates.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Include exact file paths and OPC UA spec references where applicable

---

## Phase 1: Setup (Shared Dependency)

**Purpose**: Add `ureq` HTTP client dependency (needed by US1 OCSP fetch only; other USs have no new deps).

- [ ] T001 Add `ureq` (sync HTTP client) to `async-opcua-crypto/Cargo.toml` under default features, matching the existing dependency style. Research chose ureq for zero-async-dependency sync HTTP in certificate validation path.

---

## Phase 2: Foundational (Pre-Flight Baseline)

**Purpose**: Verify clean CI baseline before any code changes. All USs must start from green.

- [ ] T002 Run `cargo fmt --all -- --check` to verify formatting compliance
- [ ] T003 Run `cargo clippy --workspace --all-targets --all-features` to verify no lint warnings
- [ ] T004 Run `RUSTFLAGS="-D warnings" cargo check --workspace` to verify all crates compile warning-free
- [ ] T005 Run `cargo test -p async-opcua-crypto --lib && cargo test -p async-opcua-server --lib && cargo test -p async-opcua-core --lib && cargo test -p async-opcua-nodes --lib` to verify all existing tests pass

**Checkpoint**: Baseline green — user story implementation can now begin independently.

---

## Phase 3: User Story 1 — Live OCSP Revocation (Priority: P1)

**Goal**: OCSP live fetch for certificate revocation checking per OPC UA Part 4 §6.1.3 (Certificate Validation). The OCSP client fetches responses from AIA URLs, validates them per RFC 6960, and integrates into the existing ChainValidationContext. Three modes: Off (default, backward compatible), Soft (fall back to CRL), Strict (hard-fail). (FR-001 through FR-006)

**Independent Test**: Configure CertificateStore with `OcspFetchPolicy::Strict`, load a CA cert and an end-entity cert with AIA OCSP URL pointing to a local HTTP responder (test fixture), run `validate_certificate_chain` and verify fetch + validation succeeds. Change responder to return "revoked" and verify rejection.

### Implementation for User Story 1

- [ ] T006 [P] [US1] Define `OcspFetchPolicy` enum (Off, Soft, Strict) and `OcspFetchConfig` struct (policy, timeout, max_response_size) in `async-opcua-crypto/src/ocsp/config.rs`. Per OPC UA Part 4 §6.1.3 (online revocation checking) and contract `contracts/ocsp-fetch-policy.md`.
- [ ] T007 [P] [US1] Implement OCSP request codec in `async-opcua-crypto/src/ocsp/codec.rs`: encode a CertID (SHA-1 of issuer DN + issuer key + cert serial) into a DER-encoded OCSPRequest per RFC 6960 §4.1.1. Use existing `der` crate already in dependency tree.
- [ ] T008 [US1] Implement OCSP response codec in `async-opcua-crypto/src/ocsp/codec.rs`: decode DER-encoded OCSPResponse, extract responseStatus, producedAt, thisUpdate/nextUpdate, and the certStatus (good/revoked/unknown) per RFC 6960 §4.2. Depends on T007 (same module).
- [ ] T009 [P] [US1] Implement OCSP HTTP fetch in `async-opcua-crypto/src/ocsp/fetch.rs`: using `ureq`, POST the DER-encoded OCSPRequest to the responder URL (from AIA extension), enforce config timeout and max response size, return raw bytes or error. Per FR-005 (timeout and max response size enforcement).
- [ ] T010 [US1] Implement OCSP response validator in `async-opcua-crypto/src/ocsp/validate.rs`: verify response signature against responder cert, chain to trusted root, check thisUpdate/nextUpdate window, check nonce match if present, check responseStatus=successful. Per RFC 6960 §4.2.2.3 and OPC UA Part 4 §6.1.3.
- [ ] T011 [US1] Implement OCSP response cache in `async-opcua-crypto/src/ocsp/cache.rs`: `HashMap` keyed by `(issuer_name_hash, issuer_key_hash, serial_number)` with TTL based on nextUpdate field. Evict expired entries on lookup. Per research decision (RFC 6960 §2.2 validity window).
- [ ] T012 [P] [US1] Add `ocsp_fetch_config` field to `CertificateStore` in `async-opcua-crypto/src/certificate_store.rs` and public setter `set_ocsp_fetch_config()`. Default = Off (backward compatible). Per FR-003, FR-006.
- [ ] T013 [US1] Wire OCSP fetch into `validate_certificate_chain` in `async-opcua-crypto/src/certificate_store.rs`: after CRL check, if OCSP policy is Soft/Strict, check cache → fetch live → cache result → validate response. On Strict: fail connection on error/unknown/revoked. On Soft: only fail on revoked, else fall through to CRL. On Off: skip. Per FR-003, FR-004 and OPC UA Part 4 §6.1.3.
- [ ] T014 [US1] Verify all `async-opcua-crypto` tests pass: `cargo test -p async-opcua-crypto --lib`
- [ ] T015 [US1] Verify full workspace: `cargo test -p async-opcua-server --lib` (OCSP integration must not break existing cert validation tests)

**Checkpoint**: US1 complete — live OCSP fetching with three-mode policy, backward compatible default.

---

## Phase 4: User Story 2 — Multi-Cert Mixed Server (Priority: P2)

**Goal**: Allow each endpoint to specify its own certificate and private key, per OPC UA Part 4 §5.5.4.1 (the Application Instance Certificate is a component of each endpoint's security configuration). Backward compatible: endpoints without explicit certs inherit server-level defaults. Startup validation ensures every security-policy endpoint has a compatible cert. (FR-007 through FR-011)

**Independent Test**: Configure server with RSA cert at top level, ECC cert on the EccNistP256 endpoint, start server, verify RSA client connects to Basic256Sha256 endpoint using RSA cert and ECC client connects to EccNistP256 endpoint using ECC cert.

### Implementation for User Story 2

- [ ] T016 [P] [US2] Add `certificate_path: Option<PathBuf>` and `private_key_path: Option<PathBuf>` fields (both `#[serde(default)]`) to `ServerEndpoint` struct in `async-opcua-server/src/config/endpoint.rs`. Per OPC UA Part 4 §5.5.4.1 (certificate is per-endpoint security configuration component) and contract `contracts/endpoint-cert-config.md`.
- [ ] T017 [US2] Replace `server_certificate: RwLock<Option<X509>>` with `endpoint_certificates: RwLock<HashMap<EndpointIdentifier, Option<X509>>>` in `ServerInfo` in `async-opcua-server/src/info.rs`. An `EndpointIdentifier` key uniquely identifies an endpoint by (path, security_policy, security_mode). Depends on T016.
- [ ] T018 [US2] Implement per-endpoint cert loading at server startup in `async-opcua-server/src/server.rs`: for each endpoint in config, resolve cert_path (endpoint override else server default), load X509 DER and private key, insert into `endpoint_certificates` map keyed by the endpoint identifier. Per FR-007, FR-010 (backward compatible single-cert mode).
- [ ] T019 [US2] Implement startup validation in `async-opcua-server/src/server.rs`: after loading certs, iterate all security-policy endpoints (security_policy != "None"). If any endpoint has no cert in the map, or the cert's key type (RSA/EC) is incompatible with the policy, log an error and return Err with a diagnostic message naming the endpoint and policy. Per FR-009 and OPC UA Part 4 §5.5.4.1 (Application Instance Certificate is required per security configuration).
- [ ] T020 [US2] Update secure channel creation in `async-opcua-server/src/session/manager.rs`: replace `info.server_certificate.read()` with constructing the `EndpointIdentifier` for the current endpoint and looking up in `info.endpoint_certificates`. Update `server_certificate_as_byte_string()` in `info.rs` to accept an endpoint identifier parameter. Per FR-008 and OPC UA Part 4 §5.5.4.1.
- [ ] T021 [US2] Update all test fixtures that set `server_certificate` in `async-opcua-server/src/session/manager.rs` (tests module), `async-opcua-server/src/info.rs` (tests module), and `async-opcua-server/src/server_handle.rs`: replace `info.server_certificate.write() = Some(cert)` with inserting into `endpoint_certificates` map for the appropriate endpoint. Ensure `make_cert_and_key` test helper still works.
- [ ] T022 [US2] Verify `cargo test -p async-opcua-server --lib` — all 306 tests pass
- [ ] T023 [US2] Verify `cargo test -p async-opcua-core --lib` — core tests unaffected

**Checkpoint**: US2 complete — multi-cert mixed server with per-endpoint certs, backward compatible.

---

## Phase 5: User Story 3 — Delete LegacyCall Actor Variant (Priority: P3)

**Goal**: Replace the `LegacyCall(Box<dyn FnOnce(...)>)` dynamic-dispatch variant in `SubscriptionCommand` with dedicated statically-typed enum variants for each subscription management operation. Delete the `legacy()` helper and the `LegacyCall` variant. No behavioral changes. (FR-012 through FR-015)

**Independent Test**: `rgrep LegacyCall` returns zero results. `cargo test -p async-opcua-server --lib` passes all subscription tests.

### Implementation for User Story 3

- [ ] T024 [US3] Add dedicated enum variants to `SubscriptionCommand` in `async-opcua-server/src/subscriptions/actor.rs` for all management operations. Each variant carries input parameters and a `oneshot::Sender<R>` for its typed return value. Group by category per contract `contracts/subscription-commands.md`:

  **Management** (T024a–T024e): `CreateSubscription`, `ModifySubscription`, `DeleteSubscriptions`, `SetPublishingMode`, `Republish`
  **Monitored Items** (T024f–T024j): `CreateMonitoredItems`, `ModifyMonitoredItems`, `DeleteMonitoredItems`, `SetMonitoringMode`, `SetTriggering`
  **Queries** (T024k–T024q): `SubscriptionIds`, `MonitoredItemRefs`, `SubscriptionAndItemData`, `MonitoredItemCount`, `MonitoredItemNodeIds`, `AvailableSequenceNumbers`, `SubscriptionDiagnostics`
  **State** (T024r–T024w): `UpdateOwner`, `ApplyRevalidatedValues`, `MarkTransferring`, `CloneForTransfer`, `InsertForTransfer`, `UserTokenMatches`
  Each variant goes in `actor.rs:24-39` (existing `SubscriptionCommand` enum block).

- [ ] T025 [US3] Add typed send methods to `SubscriptionActorHandle` in `async-opcua-server/src/subscriptions/actor.rs` for each new variant. Each method constructs the variant, sends on `self.commands`, and awaits the oneshot reply. This replaces the `legacy()` method. Use existing `enqueue_publish_request` as a pattern reference at `actor.rs:71-88`.
- [ ] T026 [US3] Add match arms in `SubscriptionActor::run()` in `async-opcua-server/src/subscriptions/actor.rs` for each new variant. Each arm drains the ring, calls the corresponding `SessionSubscriptions` method, and sends the response. Use existing `EnqueuePublish` arm at `actor.rs:124-136` as a pattern reference. No wildcard arms — every variant explicitly matched for compiler exhaustiveness.
- [ ] T027 [US3] Update all 27 `legacy()` call sites in `async-opcua-server/src/subscriptions/mod.rs` to use the new typed send methods from T025. One call site at a time, preserving the exact behavioral semantics. Verify `cargo check` after each batch to catch type mismatches early.
- [ ] T028 [US3] Update the 2 `legacy()` call sites in `async-opcua-server/src/node_manager/memory/core.rs:1202,1220` to use the new typed send methods.
- [ ] T029 [US3] Delete the `LegacyCall` variant from `SubscriptionCommand` enum and delete the `legacy()` method from `SubscriptionActorHandle` in `actor.rs`. Remove the `LegacyCall` match arm from `run()`.
- [ ] T030 [US3] Verify `cargo test -p async-opcua-server --lib` — all 306 subscription tests pass with identical behavior (FR-015, SC-004)
- [ ] T031 [US3] Verify `rgrep LegacyCall` returns zero results in `async-opcua-server/src/` (FR-013, SC-003)

**Checkpoint**: US3 complete — LegacyCall removed, all subscription operations use statically-typed variants.

---

## Phase 6: User Story 4 — Bad Ideas Example Servers (Priority: P4)

**Goal**: Add four example servers demonstrating SDK flexibility: chat server (cactuaroid/OpcUaChatServer model), chaos server, filesystem bridge, reverse bridge. Each must compile, start without panicking, and be browsable. (FR-016 through FR-022)

**Independent Test**: `cargo check -p samples-chat-server -p samples-chaos-server -p samples-filesystem-bridge -p samples-reverse-bridge` passes. Each `cargo run` starts and logs binding info.

### Implementation for User Story 4

- [ ] T032 [P] [US4] Create `samples/chat-server/` crate: `Cargo.toml` depending on `async-opcua` (path = "../../async-opcua"), `src/main.rs` implementing the `cactuaroid/OpcUaChatServer` information model per contract `contracts/chat-server-model.md`. Register types: ChatLog structure (At/DateTime, Name/String, Content/String), ChatLogType variable type, ChatLogEventType object type (extends BaseEventType with ChatLog property), ChatLogsType object type (SupportsEvents, HasNotifier→Server), instantiate ChatLogs under ObjectsFolder with Post method and PostCount variable. Post method handler: create ChatLog entry, increment PostCount, fire ChatLogEventType event. Per FR-019.

- [ ] T033 [P] [US4] Create `samples/chaos-server/` crate: `Cargo.toml` depending on `async-opcua`, `src/main.rs` with an address space where nodes randomly change type, value, or status code at runtime. Use a background task with `tokio::spawn` to periodically mutate a subset of nodes. Per FR-016.

- [ ] T034 [P] [US4] Create `samples/filesystem-bridge/` crate: `Cargo.toml` depending on `async-opcua` + `notify`, `src/main.rs` mirroring the filesystem as an OPC UA hierarchy (directories → Object nodes, files → Variable nodes with contents as values). Accept `--root <path>` CLI argument for the root directory. Per FR-017.

- [ ] T035 [P] [US4] Create `samples/reverse-bridge/` crate: `Cargo.toml` depending on `async-opcua` (both server and client halves), `src/main.rs` connecting to a source OPC UA server via client, subscribing to all accessible variables, and exposing mirrored values as Variables in its own address space. Accept `--source <url>` CLI argument. Per FR-018.

- [ ] T036 [US4] Add `README.md` to each example server crate explaining what it demonstrates and how to run it. Per FR-021.

- [ ] T037 [US4] Verify all example crates compile: `cargo check -p samples-chat-server -p samples-chaos-server -p samples-filesystem-bridge -p samples-reverse-bridge` (FR-020)

**Checkpoint**: US4 complete — four bad ideas servers compile and demonstrate SDK flexibility.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Full workspace verification. Update completeness-backlog.md to reflect all items complete.

- [ ] T038 Run `cargo fmt --all -- --check` to verify formatting across workspace
- [ ] T039 Run `cargo clippy --workspace --all-targets --all-features` to verify no new lint warnings
- [ ] T040 Run `RUSTFLAGS="-D warnings" cargo check --workspace` to verify all crates compile warning-free
- [ ] T041 Run full test suite: `cargo test -p async-opcua-crypto --lib && cargo test -p async-opcua-server --lib && cargo test -p async-opcua-core --lib && cargo test -p async-opcua-nodes --lib` — all 443+ tests pass
- [ ] T042 Update `specs/completeness-backlog.md`: move OCSP live fetching, multi-cert mixed server, LegacyCall removal, and bad ideas servers to the "Done" section. Add entry for feature 057. Per SC-006.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 — no dependencies
- **Foundational (Phase 2)**: T002–T005 — depends on T001 (ureq dep for check) — BLOCKS all user stories
- **US1 (Phase 3)**: T006–T015 — depends on Phase 2 baseline
- **US2 (Phase 4)**: T016–T023 — depends on Phase 2 baseline; independent of US1, US3, US4
- **US3 (Phase 5)**: T024–T031 — depends on Phase 2 baseline; independent of US1, US2, US4
- **US4 (Phase 6)**: T032–T037 — depends on Phase 2 baseline; independent of US1–US3
- **Polish (Phase 7)**: T038–T042 — depends on all user stories

### User Story Dependencies

All four user stories are **fully independent** — none depends on another. They touch different crates or different modules:

- **US1**: `async-opcua-crypto/src/ocsp/` (new module) + `certificate_store.rs`
- **US2**: `async-opcua-server/src/config/endpoint.rs`, `info.rs`, `server.rs`, `session/manager.rs`
- **US3**: `async-opcua-server/src/subscriptions/actor.rs`, `mod.rs`, `memory/core.rs`
- **US4**: `samples/chat-server/`, `samples/chaos-server/`, `samples/filesystem-bridge/`, `samples/reverse-bridge/` (new crates)

### Within Each User Story

- US1: T006, T007, T009, T012 are [P] (different files). T008 depends on T007 (same module). T010 depends on T007+T008 (validation needs codec). T011 is [P]. T013 depends on T010+T011+T012.
- US2: T016 is [P] (endpoint.rs). T017 depends on T016. T018, T019 depend on T017. T020 depends on T018. T021 depends on T020.
- US3: T024 (add variants) must complete first. T025 depends on T024. T026 depends on T025. T027–T028 depend on T025. T029 depends on T027+T028. T030–T031 are verification.
- US4: T032, T033, T034, T035 are all [P] (different crates, zero shared files). T036 depends on all four. T037 depends on T036.

### Parallel Opportunities

- All four user stories (Phase 3–6) can be implemented in parallel by different agents
- Within US1: T006, T007, T009, T012 can start in parallel
- Within US4: T032, T033, T034, T035 can all start in parallel

---

## Parallel Example: All Four User Stories

```bash
# Launch all four independently (different crates/modules):
Task: "T006-T015: US1 OCSP live fetch in async-opcua-crypto"
Task: "T016-T023: US2 multi-cert server in async-opcua-server/config + info + server + session"
Task: "T024-T031: US3 LegacyCall removal in async-opcua-server/subscriptions"
Task: "T032-T037: US4 example servers in samples/"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: T001 (add ureq dep)
2. Complete Phase 2: T002–T005 (baseline verification)
3. Complete Phase 3: T006–T015 (OCSP live fetch)
4. **STOP and VALIDATE**: `cargo test -p async-opcua-crypto --lib` green
5. Commit US1 as a single PR

### Incremental Delivery (Sequential)

1. T001 → T002–T005 → T006–T015 → **US1 done**
2. T016–T023 → **US2 done**
3. T024–T031 → **US3 done**
4. T032–T037 → **US4 done**
5. T038–T042 → Full workspace verification → **All four USs done**

### Parallel Team Strategy

With multiple agents:

1. One agent completes Phase 1 + 2 (baseline)
2. Once baseline green, four agents take US1–US4 in parallel
3. Final agent runs Phase 7 polish

---

## Notes

- [P] tasks = different files or independent code paths, no dependencies — these ARE parallelizable
- All four user stories are independent and touch different crates/modules
- No new tests required — behavioral preservation verified by existing test suite
- Each task with OPC UA behavior change includes the spec reference (Part 4 §N.N.N, Part 6 §N.N, RFC 6960 §N.N)
- Each task includes exact file paths for unambiguous delegation
- Commit after each completed user story
