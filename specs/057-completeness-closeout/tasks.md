# Tasks: Completeness Closeout

**Input**: Design documents from `/specs/057-completeness-closeout/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Existing test suites serve as regression safety net. No new test tasks — all four USs are verified by the existing 443 tests (core 89, server 306, nodes 48) plus compilation of new example crates.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Include exact file paths and OPC UA spec references

---

## Phase 1: Setup (Shared Dependency)

**Purpose**: Add `ureq` HTTP client dependency (needed by US1 OCSP fetch only; other USs have no new deps).

- [x] T001 Add `ureq` (sync HTTP client, version 3.x) to `async-opcua-crypto/Cargo.toml` under default features. Research chose ureq for zero-async-dependency sync HTTP in the certificate validation path.

---

## Phase 2: Foundational (Pre-Flight Baseline)

**Purpose**: Verify clean CI baseline before any code changes.

- [x] T002 Run `cargo fmt --all -- --check` to verify formatting compliance
- [x] T003 Run `cargo clippy --workspace --all-targets --all-features` to verify no lint warnings
- [x] T004 Run `RUSTFLAGS="-D warnings" cargo check --workspace` to verify all crates compile warning-free
- [x] T005 Run `cargo test -p async-opcua-crypto --lib && cargo test -p async-opcua-server --lib && cargo test -p async-opcua-core --lib && cargo test -p async-opcua-nodes --lib` to verify all existing tests pass

**Checkpoint**: Baseline green — user story implementation can now begin independently.

---

## Phase 3: User Story 1 — Live OCSP Revocation (Priority: P1)

**Goal**: OCSP live fetch for certificate revocation checking per OPC UA Part 4 §6.1.3 (Certificate Validation). Three modes: Off (default, backward compatible), Soft (fall back to CRL), Strict (hard-fail). (FR-001 through FR-006)

**Independent Test**: Configure CertificateStore with `OcspFetchPolicy::Strict`, load a CA cert and an end-entity cert with AIA OCSP URL pointing to a local HTTP responder (test fixture), run `validate_certificate_chain` and verify fetch + validation succeeds. Change responder to return "revoked" and verify rejection.

### Implementation for User Story 1

- [x] T006 [P] [US1] Define `OcspFetchPolicy` enum (Off, Soft, Strict) and `OcspFetchConfig` struct (fields: policy, timeout: Duration, max_response_size: usize) in `async-opcua-crypto/src/ocsp/config.rs`. Per OPC UA Part 4 §6.1.3 (online revocation checking) and contract `contracts/ocsp-fetch-policy.md`.

- [x] T007 [P] [US1] Implement OCSP request encoding in `async-opcua-crypto/src/ocsp/codec.rs`: encode a CertID (SHA-1 of issuer DN + issuer key + cert serial number) into a DER-encoded OCSPRequest per RFC 6960 §4.1.1. Use the `der` crate already in the dependency tree.

- [x] T008 [US1] Implement OCSP response decoding in `async-opcua-crypto/src/ocsp/codec.rs`: decode DER-encoded OCSPResponse, extract responseStatus, producedAt, thisUpdate/nextUpdate, and the certStatus (good/revoked/unknown) per RFC 6960 §4.2. Same file as T007 — sequential.

- [x] T009 [P] [US1] Extract OCSP responder URL from a certificate's Authority Information Access (AIA) extension in `async-opcua-crypto/src/ocsp/aia.rs`. Parse the AIA extension (OID 1.3.6.1.5.5.7.1.1), find the first entry with accessMethod = id-ad-ocsp (1.3.6.1.5.5.7.48.1), and return the URI. Per OPC UA Part 4 §6.1.3 (certificate validation requires OCSP responder discovery). FR-001, RFC 5280 §4.2.2.1.

- [x] T010 [US1] Implement OCSP HTTP fetch in `async-opcua-crypto/src/ocsp/fetch.rs`: using `ureq`, POST the DER-encoded OCSPRequest (from T007) to the responder URL (from T009), enforce `OcspFetchConfig.timeout` and `max_response_size`, return raw bytes or `OcspError::FetchFailed`. Per FR-005 (timeout and max response size enforcement). Depends on T007 and T009.

- [x] T011 [US1] Implement OCSP response validator in `async-opcua-crypto/src/ocsp/validate.rs`: verify response signature against responder certificate, verify responder cert chains to a trusted root, check thisUpdate/nextUpdate validity window, check nonce match if present, check responseStatus = successful. Return `OcspResult { status: Good | Revoked | Unknown }`. Per RFC 6960 §4.2.2.3 and OPC UA Part 4 §6.1.3. Depends on T008 (needs decoded response).

- [x] T012 [US1] Implement OCSP response cache in `async-opcua-crypto/src/ocsp/cache.rs`: `HashMap` keyed by `(issuer_name_hash: Vec<u8>, issuer_key_hash: Vec<u8>, serial_number: Vec<u8>)` with each entry storing the DER response and `next_update` timestamp. On lookup, if `now < next_update` return cached; otherwise evict. Per research decision (RFC 6960 §2.2 validity window), FR-002.

- [x] T013 [P] [US1] Add `ocsp_fetch_config: Option<OcspFetchConfig>` field to `CertificateStore` in `async-opcua-crypto/src/certificate_store.rs` and public setter `pub fn set_ocsp_fetch_config(&mut self, config: OcspFetchConfig)`. When config is Some and policy != Off, OCSP is active. Default-construct as None (backward compatible). Per OPC UA Part 4 §6.1.3 (online revocation checking, default-off). FR-003, FR-006.

- [x] T014 [US1] Wire OCSP fetch into `CertificateStore::validate_certificate_chain` in `async-opcua-crypto/src/certificate_store.rs`. After the existing CRL check, if `ocsp_fetch_config` is Some and policy != Off: for each certificate in the chain, extract AIA URL (T009), check cache (T012), on miss fetch (T010), validate response (T011), cache result. On Strict: fail on error/unknown/revoked. On Soft: fail only on revoked, else fall through to CRL. Per FR-003, FR-004 and OPC UA Part 4 §6.1.3. Depends on T010, T011, T012, T013.

- [x] T015 [US1] Verify `cargo test -p async-opcua-crypto --lib` — all crypto tests pass with OCSP integration
- [x] T016 [US1] Verify `cargo test -p async-opcua-server --lib` — OCSP integration does not break existing cert validation tests. Per SC-001.

**Checkpoint**: US1 complete — live OCSP fetching with three-mode policy, backward compatible default.

---

## Phase 4: User Story 2 — Multi-Cert Mixed Server (Priority: P2)

**Goal**: Allow each endpoint to specify its own certificate and private key, per OPC UA Part 4 §5.5.4.1 (the Application Instance Certificate is a component of each endpoint's security configuration). Backward compatible: endpoints without explicit certs inherit server-level defaults. (FR-007 through FR-011)

**Independent Test**: Configure server with RSA cert at top level, ECC cert on the EccNistP256 endpoint, start server, verify RSA client connects to Basic256Sha256 endpoint and ECC client connects to EccNistP256 endpoint.

### Implementation for User Story 2

- [x] T017 [P] [US2] Add `certificate_path: Option<PathBuf>` and `private_key_path: Option<PathBuf>` fields (both `#[serde(default)]`) to `ServerEndpoint` struct in `async-opcua-server/src/config/endpoint.rs`. Per OPC UA Part 4 §5.5.4.1 and contract `contracts/endpoint-cert-config.md`.

- [x] T018 [US2] Replace `server_certificate: RwLock<Option<X509>>` with `endpoint_certificates: RwLock<HashMap<EndpointIdentifier, Option<(X509, PrivateKey)>>>` in `ServerInfo` in `async-opcua-server/src/info.rs`. The map key is `EndpointIdentifier` (path, security_policy, security_mode); the value stores both the X509 cert and its private key. Per OPC UA Part 4 §5.5.4.1 (Application Instance Certificate is per-endpoint security configuration component). FR-007. Depends on T017.

- [x] T019 [US2] Implement per-endpoint cert loading at server startup in `async-opcua-server/src/server.rs`: for each endpoint in config, resolve cert_path (endpoint override else server default), load X509 DER and private key, insert into `endpoint_certificates` map keyed by the endpoint identifier. Per OPC UA Part 4 §5.5.4.1. FR-007, FR-010. Depends on T018.

- [x] T020 [US2] Implement startup validation in `async-opcua-server/src/server.rs`: after loading certs, iterate all security-policy endpoints (security_policy != "None"). If any such endpoint has no cert in the map, or the cert's key type (RSA/EC) is incompatible with the endpoint's security policy, return an error with a diagnostic message: `"Endpoint {path} uses security policy {policy} but no compatible certificate is configured."` Per FR-009 and OPC UA Part 4 §5.5.4.1. Depends on T019.

- [x] T021 [US2] Update secure channel creation in `async-opcua-server/src/session/manager.rs`: replace `info.server_certificate.read()` calls with constructing the `EndpointIdentifier` for the current connection and looking up in `info.endpoint_certificates`. Update the `server_certificate` variable bindings in functions: `create_secure_channel_impl` (line ~345), `activate_session` (line ~440), and test helper functions. Per FR-008 and OPC UA Part 4 §5.5.4.1. Depends on T019.

- [x] T022 [US2] Update `ServerInfo::server_certificate_as_byte_string()` in `async-opcua-server/src/info.rs` to accept an `EndpointIdentifier` parameter. Look up in `endpoint_certificates` map instead of reading the old single-cert field. Update all callers (in `info.rs` line ~915, `server.rs` line ~824, `manager.rs` lines ~365, ~776). Remove the old `server_certificate` field. Per OPC UA Part 4 §5.5.4.1 (certificate is per-endpoint security configuration component). FR-008. Depends on T018.

- [x] T023 [P] [US2] Update test fixtures in `async-opcua-server/src/session/manager.rs` tests module (~8 fixture sites at lines ~1968, ~2028, ~2126, ~2192, ~2433): replace `*handle.info().server_certificate.write() = Some(cert)` with inserting into `info.endpoint_certificates.write()[endpoint_id] = Some((cert, key))`. Add the appropriate endpoint identifier for each test.

- [x] T024 [P] [US2] Update test fixtures in `async-opcua-server/src/info.rs` tests module (~6 fixture sites around line ~820, ~1021, ~1685): same pattern as T023. Replace single-cert writes with endpoint_certificates map inserts.

- [x] T025 [P] [US2] Update `ServerHandle::set_certificate` in `async-opcua-server/src/server_handle.rs` (line ~152): replace `self.info.server_certificate.write()` with inserting into `self.info.endpoint_certificates`. Accept an optional endpoint identifier parameter; if None, insert for all existing endpoint keys.

- [x] T026 [US2] Update WSS (opc.wss) transport in `async-opcua-server/src/transport/tcp.rs` (or equivalent WSS path): ensure the per-endpoint certificate resolution works identically for WebSocket connections. The transport layer reads the `EndpointIdentifier` and looks up in `endpoint_certificates` — same as opc.tcp. Per OPC UA Part 4 §5.5.4.1 (cert selection is per-endpoint, transport-agnostic). FR-011. Depends on T021, T022.

- [x] T027 [US2] Verify `cargo test -p async-opcua-server --lib` — all 306 tests pass. Per SC-002.
- [x] T028 [US2] Verify `cargo test -p async-opcua-core --lib` — core tests unaffected

**Checkpoint**: US2 complete — multi-cert mixed server with per-endpoint certs, backward compatible, WSS parity.

---

## Phase 5: User Story 3 — Delete LegacyCall Actor Variant (Priority: P3)

**Goal**: Replace `LegacyCall(Box<dyn FnOnce(...)>)` dynamic-dispatch in `SubscriptionCommand` with dedicated statically-typed enum variants. Delete the `legacy()` helper and `LegacyCall` variant. No behavioral changes. (FR-012 through FR-015)

**Independent Test**: `rgrep LegacyCall` returns zero results. `cargo test -p async-opcua-server --lib` passes all 306 tests.

### Implementation for User Story 3

- [x] T029 [US3] Add **management operation** enum variants to `SubscriptionCommand` in `async-opcua-server/src/subscriptions/actor.rs:24-39`. Five variants:

  `CreateSubscription { request: CreateSubscriptionRequest, info: SubscriptionInfo, response: oneshot::Sender<Result<u32, StatusCode>> }`
  `ModifySubscription { request: ModifySubscriptionRequest, info: SubscriptionInfo, response: oneshot::Sender<Result<(), StatusCode>> }`
  `DeleteSubscriptions { ids: Vec<u32>, response: oneshot::Sender<Result<(), StatusCode>> }`
  `SetPublishingMode { request: SetPublishingModeRequest, response: oneshot::Sender<Result<(), StatusCode>> }`
  `Republish { request: RepublishRequest, response: oneshot::Sender<Result<(), StatusCode>> }`

  Per contract `contracts/subscription-commands.md`.

- [x] T030 [US3] Add **monitored-item operation** variants to `SubscriptionCommand` in `async-opcua-server/src/subscriptions/actor.rs`. Five variants:

  `CreateMonitoredItems { sub_id: u32, requests: Vec<MonitoredItemCreateRequest>, response: oneshot::Sender<Result<Vec<MonitoredItemCreateResult>, StatusCode>> }`
  `ModifyMonitoredItems { sub_id: u32, requests: Vec<MonitoredItemModifyRequest>, response: oneshot::Sender<Result<Vec<MonitoredItemModifyResult>, StatusCode>> }`
  `DeleteMonitoredItems { sub_id: u32, items: Vec<u32>, response: oneshot::Sender<Result<(), StatusCode>> }`
  `SetMonitoringMode { sub_id: u32, mode: MonitoringMode, items: Vec<u32>, response: oneshot::Sender<Result<(), StatusCode>> }`
  `SetTriggering { sub_id: u32, triggering_item_id: u32, links_to_add: Vec<u32>, links_to_remove: Vec<u32>, response: oneshot::Sender<Result<(), StatusCode>> }`

  Per contract `contracts/subscription-commands.md`.

- [x] T031 [US3] Add **read-only query** variants to `SubscriptionCommand` in `async-opcua-server/src/subscriptions/actor.rs`. Seven variants:

  `SubscriptionIds { response: oneshot::Sender<Vec<u32>> }`
  `MonitoredItemRefs { response: oneshot::Sender<Vec<MonitoredItemRef>> }`
  `SubscriptionAndItemData { response: oneshot::Sender<(Vec<u32>, Vec<MonitoredItemRef>)> }`
  `MonitoredItemCount { sub_id: u32, response: oneshot::Sender<Option<usize>> }`
  `MonitoredItemNodeIds { sub_id: u32, ids: Vec<u32>, response: oneshot::Sender<Vec<Option<NodeId>>> }`
  `AvailableSequenceNumbers { sub_id: u32, response: oneshot::Sender<Option<Vec<u32>>> }`
  `SubscriptionDiagnostics { response: oneshot::Sender<SubscriptionDiagnosticsDataType> }`

  Per contract `contracts/subscription-commands.md`.

- [x] T032 [US3] Add **state mutation** variants to `SubscriptionCommand` in `async-opcua-server/src/subscriptions/actor.rs`. Six variants:

  `UpdateOwner { key: SecurityToken, type_tree_for_user: Arc<dyn TypeTreeForUserStatic>, response: oneshot::Sender<()> }`
  `ApplyRevalidatedValues { values: HashMap<u32, DataValue>, response: oneshot::Sender<Vec<StatusCode>> }`
  `MarkTransferring { sub_id: u32, response: oneshot::Sender<Result<(), StatusCode>> }`
  `CloneForTransfer { sub_id: u32, response: oneshot::Sender<Option<(Subscription, Vec<Notification>)>> }`
  `InsertForTransfer { sub: Subscription, notifs: Vec<Notification>, response: oneshot::Sender<Result<(), StatusCode>> }`
  `UserTokenMatches { key: SecurityToken, response: oneshot::Sender<bool> }`

  Per contract `contracts/subscription-commands.md`.

- [x] T033 [US3] Add typed send methods to `SubscriptionActorHandle` in `async-opcua-server/src/subscriptions/actor.rs` for each variant from T029–T032. Each method constructs the variant, sends via `self.commands.send(...)`, and awaits `reply_rx`. Use existing `enqueue_publish_request` at `actor.rs:71-88` as pattern reference. Depends on T029–T032.

- [x] T034 [US3] Add match arms in `SubscriptionActor::run()` in `async-opcua-server/src/subscriptions/actor.rs` for each variant from T029–T032. Each arm: drain the ring, call the corresponding `SessionSubscriptions` method, send the response via oneshot. Use existing `EnqueuePublish` arm at `actor.rs:124-136` as pattern. No wildcard arms — compiler exhaustiveness required. Depends on T033.

- [x] T035 [US3] Update the 5 management-operation `legacy()` call sites in `async-opcua-server/src/subscriptions/mod.rs` (lines ~917-1017: create_subscription, modify_subscription, set_publishing_mode, republish, delete_subscriptions) to use the typed send methods from T033. Depends on T033.

- [x] T036 [US3] Update the 5 monitored-item `legacy()` call sites in `async-opcua-server/src/subscriptions/mod.rs` (lines ~1333-1547: create_monitored_items, modify_monitored_items, set_monitoring_mode, delete_monitored_items, set_triggering) to use the typed send methods from T033. Depends on T033.

- [x] T037 [US3] Update the 7 query `legacy()` call sites in `async-opcua-server/src/subscriptions/mod.rs` (lines ~545, ~562, ~586, ~881, ~1445, ~1609, ~1629) to use the typed send methods from T033. Depends on T033.

- [x] T038 [US3] Update the ~10 state-mutation and transfer `legacy()` call sites in `async-opcua-server/src/subscriptions/mod.rs` (lines ~618, ~636, ~660, ~689, ~1393, ~1519, ~1577, ~1698, ~1709, ~1721, ~1744, ~1774, ~1794) to use the typed send methods from T033. Depends on T033.

- [x] T039 [P] [US3] Update the 2 `legacy()` call sites in `async-opcua-server/src/node_manager/memory/core.rs` (lines 1202, 1220) to use the typed send methods from T033. Different file than T035–T038 — can run in parallel with them.

- [x] T040 [US3] Delete the `LegacyCall` variant from `SubscriptionCommand` enum, delete the `legacy()` method from `SubscriptionActorHandle`, and remove the `LegacyCall` match arm from `run()` — all in `async-opcua-server/src/subscriptions/actor.rs`. Depends on T035–T039 (all call sites migrated).

- [x] T041 [US3] Verify `cargo test -p async-opcua-server --lib` — all 306 tests pass with identical behavior (FR-014, FR-015, SC-004)
- [x] T042 [US3] Verify `rgrep LegacyCall async-opcua-server/src/` returns zero results (FR-013, SC-003)

**Checkpoint**: US3 complete — LegacyCall removed, all subscription operations statically-typed.

---

## Phase 6: User Story 4 — Bad Ideas Example Servers (Priority: P4)

**Goal**: Add four example servers: chat server (cactuaroid/OpcUaChatServer model), chaos server, filesystem bridge, reverse bridge. Each compiles, starts, and is browsable. (FR-016 through FR-022)

**Independent Test**: `cargo check -p samples-chat-server -p samples-chaos-server -p samples-filesystem-bridge -p samples-reverse-bridge` passes. Each `cargo run` starts and logs binding info. Browse output confirms address space is navigable.

### Implementation for User Story 4

- [x] T043 [P] [US4] Create `samples/chat-server/` crate scaffolding: `Cargo.toml` depending on `async-opcua` (path = "../../async-opcua"), `src/main.rs` with server setup (boilerplate: config, builder, run). Register the chat information model types per contract `contracts/chat-server-model.md`: ChatLog structure (DataType, fields: At/DateTime, Name/String, Content/String), ChatLogType variable type (DataType=ChatLog), ChatLogEventType object type (extends BaseEventType, property: ChatLog of type ChatLogType), ChatLogsType object type (BaseObjectType, SupportsEvents=true, HasNotifier→Server). Instantiate ChatLogs object under ObjectsFolder with PostCount variable (UInt32, initial 0). Per FR-019.

- [x] T044 [US4] Implement chat server Post method handler in `samples/chat-server/src/main.rs`: register `Post` method on ChatLogs object with inputs (Name: String, Content: String). Handler creates ChatLog { At: now(), Name, Content }, increments PostCount, fires ChatLogEventType event with the ChatLog property set. Returns Good status code. Per contract §Runtime Behavior. Depends on T043.

- [x] T045 [P] [US4] Create `samples/chaos-server/` crate: `Cargo.toml` depending on `async-opcua`, `src/main.rs` with an address space where nodes randomly change type, value, or status code at runtime. Use `tokio::spawn` background task to periodically select random nodes (from a pre-built list) and mutate them. Per FR-016.

- [x] T046 [P] [US4] Create `samples/filesystem-bridge/` crate: `Cargo.toml` depending on `async-opcua` + `notify` (for live filesystem watching), `src/main.rs` mirroring the filesystem as an OPC UA hierarchy. Directories → Object nodes, files → Variable nodes (data type ByteString for binary, String for text). Accept `--root <path>` CLI argument. Use `notify::Watcher` to update nodes on filesystem changes. Per FR-017.

- [x] T047 [P] [US4] Create `samples/reverse-bridge/` crate: `Cargo.toml` depending on `async-opcua` (both server and client sides), `src/main.rs` connecting to a source OPC UA server via `Client`, browsing all variables, creating monitored items for each, and exposing mirrored values as Variables in its own address space. Accept `--source <url>` CLI argument. Per FR-018.

- [x] T048 [US4] Add `README.md` to each of the four example server crates. Include: what the example demonstrates, how to run it (`cargo run`), expected browse output, and any CLI arguments. Per FR-021. Depends on T043–T047.

- [x] T049 [US4] Verify all example crates compile: `cargo check -p samples-chat-server -p samples-chaos-server -p samples-filesystem-bridge -p samples-reverse-bridge`. Then, for each server, start it (`cargo run &`), verify it binds and logs, and confirm the address space is browsable via at least the root Objects folder. Per FR-020, FR-022, SC-005. Depends on T048.

**Checkpoint**: US4 complete — four bad ideas servers compile, start, and demonstrate SDK flexibility.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Full workspace verification. Update completeness-backlog.md.

- [x] T050 Run `cargo fmt --all -- --check` to verify formatting across workspace
- [x] T051 Run `cargo clippy --workspace --all-targets --all-features` to verify no new lint warnings
- [x] T052 Run `RUSTFLAGS="-D warnings" cargo check --workspace` to verify all crates compile warning-free
- [x] T053 Run full test suite: `cargo test -p async-opcua-crypto --lib && cargo test -p async-opcua-server --lib && cargo test -p async-opcua-core --lib && cargo test -p async-opcua-nodes --lib` — all 443+ tests pass
- [x] T054 Update `specs/completeness-backlog.md`: move OCSP live fetching, multi-cert mixed server, LegacyCall removal, and bad ideas servers to the "Done" section. Add entry for feature 057. Per SC-006.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 — no dependencies
- **Foundational (Phase 2)**: T002–T005 — depends on T001 (ureq dep for check) — BLOCKS all user stories
- **US1 (Phase 3)**: T006–T016 — depends on Phase 2 baseline
- **US2 (Phase 4)**: T017–T028 — depends on Phase 2 baseline; independent of US1, US3, US4
- **US3 (Phase 5)**: T029–T042 — depends on Phase 2 baseline; independent of US1, US2, US4
- **US4 (Phase 6)**: T043–T049 — depends on Phase 2 baseline; independent of US1–US3
- **Polish (Phase 7)**: T050–T054 — depends on all user stories

### User Story Dependencies

All four user stories are **fully independent** — none depends on another:

| US | Crate(s) | Files touched |
|----|----------|---------------|
| US1 | `async-opcua-crypto` | `src/ocsp/` (new), `certificate_store.rs` |
| US2 | `async-opcua-server` | `config/endpoint.rs`, `info.rs`, `server.rs`, `session/manager.rs`, `server_handle.rs`, `transport/tcp.rs` |
| US3 | `async-opcua-server` | `subscriptions/actor.rs`, `subscriptions/mod.rs`, `memory/core.rs` |
| US4 | `samples/` | 4 new crates |

### Within Each User Story

**US1**:
- T006, T007, T009, T013 are [P] (different files)
- T008 depends on T007 (same module `codec.rs`)
- T010 depends on T007 and T009 (fetch needs request codec + AIA URL extraction)
- T011 depends on T008 (validator needs decoded response)
- T012 is [P] (cache is independent)
- T014 depends on T010, T011, T012, T013 (integration needs all pieces)
- T015 → T016 (sequential verification)

**US2**:
- T017 is [P]
- T018 depends on T017 (needs ServerEndpoint fields)
- T019, T020 depend on T018
- T021 depends on T019 (needs endpoint_certificates populated)
- T022 depends on T018 (needs endpoint_certificates map)
- T023, T024, T025 are all [P] (different files, test fixtures)
- T026 depends on T021, T022 (needs cert lookup + function signature)
- T027 → T028 (sequential verification)

**US3**:
- T029, T030, T031, T032 are sequential (all modify same enum in `actor.rs` — same file constraint)
- T033 depends on T029–T032 (needs all variants defined)
- T034 depends on T033 (needs send methods)
- T035, T036, T037, T038 are sequential within mod.rs (same file), but T039 is [P] (different file: core.rs)
- T040 depends on T035–T039 (all call sites migrated)
- T041 → T042 (sequential verification)

**US4**:
- T043, T045, T046, T047 are all [P] (different crates)
- T044 depends on T043 (extends same file `main.rs`)
- T048 depends on T043–T047 (needs all crates created)
- T049 depends on T048 (verification needs READMEs + complete crates)

### Parallel Opportunities

- Within US1: T006, T007, T009, T012, T013 can start in parallel
- Within US2: T023, T024, T025 can start in parallel
- Within US3: T039 can run parallel to T035–T038 (different file)
- Within US4: T043, T045, T046, T047 can all start in parallel
- All four user stories can proceed in parallel after Phase 2

---

## Parallel Example: All Four User Stories

```bash
# Launch all four independently (different crates/modules):
Task: "T006-T016: US1 OCSP live fetch in async-opcua-crypto"
Task: "T017-T028: US2 multi-cert server in async-opcua-server"
Task: "T029-T042: US3 LegacyCall removal in async-opcua-server/subscriptions"
Task: "T043-T049: US4 example servers in samples/"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete T001 (ureq dep)
2. Complete T002–T005 (baseline)
3. Complete T006–T016 (OCSP)
4. **STOP and VALIDATE**: `cargo test -p async-opcua-crypto --lib` green
5. Commit US1 as a single PR

### Incremental Delivery

1. T001 → T002–T005 → T006–T016 → **US1 done**
2. T017–T028 → **US2 done**
3. T029–T042 → **US3 done**
4. T043–T049 → **US4 done**
5. T050–T054 → Full workspace verification → **All four USs done**

### Parallel Team Strategy

1. One agent completes Phase 1 + 2 (baseline)
2. Once baseline green, four agents take US1–US4 in parallel
3. Final agent runs Phase 7 polish

---

## Notes

- [P] tasks = different files, no dependencies — these ARE parallelizable
- All four user stories are independent and touch different crates/modules
- No new tests required — behavioral preservation verified by existing test suite
- Each task with OPC UA behavior change includes the spec reference
- Each task includes exact file paths for unambiguous MCP delegation
- Commit after each completed user story
