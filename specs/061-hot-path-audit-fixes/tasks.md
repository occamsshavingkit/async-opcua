# Tasks: Hot Path Audit Fixes

**Input**: Design documents from `/specs/061-hot-path-audit-fixes/`
**Prerequisites**: plan.md, spec.md, research.md

**Organization**: Tasks grouped by user story. US1 (per-message), US2 (startup), and US3 (per-request) are P1 and independent — different files/crates, can run in parallel. US4-US5 are P2.

**OPC UA Spec Citations**: Not applicable. Pure internal optimization — no protocol behavior change.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: User Story 1 — DecodingOptions Arc (Priority: P1)

**Goal**: Replace `DecodingOptions` struct clone with `Arc` sharing on every encode/decode.

**Independent Test**: `cargo test -p async-opcua-types` — all encoding tests produce identical byte output.

### Implementation

- [ ] T001 [P] [US1] Change `Context.options` field from `DecodingOptions` to `Arc<DecodingOptions>` in `async-opcua-types/src/encoding.rs` (~line 318 in `Context` struct definition). Update `Context::new()` to accept/construct `Arc<DecodingOptions>`.
- [ ] T002 [US1] Update `ContextOwned::context()` in `async-opcua-types/src/type_loader/mod.rs` (line 254-261) to return `Arc::clone(&self.options)` instead of `self.options.clone()`.
- [ ] T003 [US1] Update all call sites that construct `Context` directly — search for `Context {` or `Context::new(` across the workspace — to wrap `DecodingOptions` in `Arc::new()`.
- [ ] T004 [US1] Run `cargo test -p async-opcua-types` and `cargo test --locked --all-features` to verify encoding/decoding produces identical output. Run `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings`.

**Checkpoint**: `DecodingOptions` shared via `Arc`, not cloned. All encoding tests pass.

---

## Phase 2: User Story 2 — Type Tree Build Once (Priority: P1)

**Goal**: Build type tree and browse name index exactly once after all node managers initialize.

**Independent Test**: `cargo test -p async-opcua-server` — server init tests pass. Type tree correctness verified by existing browse/address-space tests.

### Implementation

- [ ] T005 [US2] In `async-opcua-server/src/node_manager/memory/mod.rs` (lines 828-839), remove `load_into_type_tree()`, `ensure_browse_name_index()`, and `publish_type_tree_snapshot()` calls from `InMemoryNodeManager::init()`. Keep the `self.inner.init()` call and namespace setup — only remove the three redundant rebuild calls.
- [ ] T006 [P] [US2] In `async-opcua-server/src/node_manager/memory/core.rs`, remove any equivalent type-tree rebuild logic from `CoreNodeManagerImpl::init()` if present (check line ~171-189).
- [ ] T007 [US2] In `async-opcua-server/src/server.rs::initialize_node_managers()` (lines 674-693), after the `for mgr in self.node_managers.iter()` loop completes: iterate all managers' address spaces, load into `type_tree`, build the browse name index once across all managers' aggregate nodes, and call `publish_type_tree_snapshot` exactly once.
- [ ] T008 [US2] Run `cargo test -p async-opcua-server` and `cargo test --locked --all-features`. Verify no test regressions, especially browse and address-space tests.

**Checkpoint**: Type tree built once, not N times. All server init tests pass.

---

## Phase 3: User Story 3 — RequestContext Caching (Priority: P1)

**Goal**: Cache `Arc<RequestContextInner>` on `SessionActor` to avoid per-Read/Write allocation.

**Independent Test**: `cargo test -p async-opcua-server -- session`

### Implementation

- [ ] T009 [US3] Add `cached_context: Option<Arc<RequestContextInner>>` and a version counter (e.g., `context_version: u64`) to `SessionActor` in `async-opcua-server/src/session/actor.rs`.
- [ ] T010 [US3] Refactor `SessionActor::request_context()` (~line 223-253) to: increment version on token change → if cached version matches, return `Arc::clone(&cached)` → otherwise build new context, cache it, and return.
- [ ] T011 [US3] Run `cargo test -p async-opcua-server -- session` and `cargo test --locked --all-features`. Verify: (a) Read and Write tests pass with cached context, (b) session re-activation with a new user token correctly invalidates and rebuilds the cached context, and (c) the server rejects requests that arrive between the cache invalidation and rebuild.

**Checkpoint**: Per-request `Arc<RequestContextInner>` allocation replaced with `Arc::clone()` of cached context.

---

## Phase 4: User Story 4 — SecurityPolicy Caching (Priority: P2)

**Goal**: Cache resolved `SecurityPolicy` on `SecureChannel` to avoid per-chunk string matching.

**Independent Test**: `cargo test -p async-opcua-core -- secure_channel`

### Implementation

- [ ] T012 [US4] Add `validated_security_policy: Option<SecurityPolicy>` field to `SecureChannel` in `async-opcua-core/src/comms/secure_channel.rs`. Set it in `set_security_policy()` and validate once there (replacing `expect_supported_security_policy()`'s per-operation check with a one-time validation during assignment).
- [ ] T013 [P] [US4] In `async-opcua-core/src/comms/security_header.rs::SecurityHeader::decode_from_stream()`, use the cached `SecurityPolicy` from the `SecureChannel` for validation instead of calling `SecurityPolicy::from_uri()`. The cached value comparison replaces the URI string-to-enum conversion.
- [ ] T014 [US4] Remove `expect_supported_security_policy()` method from `SecureChannel` (lines ~2089-2098) and replace its call sites in `symmetric_sign_and_encrypt()` and `symmetric_decrypt_and_verify()` with a simple boolean flag check or `Option::is_some()` check on `validated_security_policy`.
- [ ] T015 [US4] Run `cargo test -p async-opcua-core` and `cargo test --locked --all-features`. Verify all secure channel and encryption tests pass.

**Checkpoint**: `SecurityPolicy::from_uri()` called at most once per channel. Per-chunk validation is a cached comparison.

---

## Phase 5: User Story 5 — Parallel Certificate Loading (Priority: P2)

**Goal**: Parallelize endpoint certificate and private key file I/O during server startup.

**Independent Test**: `cargo test -p async-opcua-crypto -- certificate_store` and server startup tests.

### Implementation

- [ ] T016 [US5] Add `read_cert_async(path: &Path) -> Result<X509>` and `read_pkey_async(path: &Path) -> Result<PrivateKey>` to `CertificateStore` in `async-opcua-crypto/src/certificate_store.rs` using `tokio::fs::read`. These are async wrappers around the existing sync read logic.
- [ ] T017 [US5] In `async-opcua-server/src/server.rs` (~lines 335-390), collect cert+key read futures for all endpoints into a single `Vec`, pair each endpoint's cert and key into a `tokio::join!` future, and run all endpoint futures concurrently via `futures::future::join_all`. If the build path is synchronous, bridge with `tokio::runtime::Handle::current().block_on()`.
- [ ] T018 [US5] Run `cargo test -p async-opcua-crypto` and `cargo test -p async-opcua-server`. Verify certificate loading works correctly for single and multiple endpoint configurations.

**Checkpoint**: Certificate loading parallelized. Same certs loaded, same error handling.

---

## Phase 6: Polish & Verification

- [ ] T019 Run full CI playbook via `tools/ci-playbook.sh --ci` — all gates must pass
- [ ] T020 Run `RUSTFLAGS="-D warnings" cargo check --no-default-features -p async-opcua -p async-opcua-types -p async-opcua-nodes -p async-opcua-server` to verify no-default-features builds
- [ ] T021 Run benchmark: `cargo build --release --bin async-opcua-localhost-bench && taskset -c 11 ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5` (3 runs, report median) to measure throughput impact
- [ ] T022 [P] Update `specs/SESSION-HANDOFF.md` with feature 061 summary

---

## Dependencies

```
Phase 1 (US1): T001 (parallel with T005, T009) → T002 → T003 → T004
Phase 2 (US2): T005, T006 (parallel) → T007 → T008
Phase 3 (US3): T009 → T010 → T011
Phase 4 (US4): T012 → T013 → T014 → T015
Phase 5 (US5): T016 → T017 → T018

Phase 6: T019 → T020, T021, T022 (parallel after all preceding)
```

**US1, US2, US3 are independent** — different crates/files, zero shared state. Can all start in parallel.
**US4** is independent from US1-3 (different crate: async-opcua-core).
**US5** is independent from US1-4.

## Parallel Execution Opportunities

```
Agent A: T001 → T002 → T003 → T004 (US1 — types crate)
Agent B: T005, T006 → T007 → T008 (US2 — server crate)
Agent C: T009 → T010 → T011 (US3 — server/actor.rs)
Agent D: T012, T013 → T014 → T015 (US4 — core crate)
Agent E: T016 → T017 → T018 (US5 — crypto + server crate)
```

All five phases can execute concurrently.
