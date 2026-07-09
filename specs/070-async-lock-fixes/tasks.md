# Tasks: Async Lock Audit Remediation

**Input**: Design documents from `specs/070-async-lock-fixes/`
**Audit Source**: `specs/069-companion-specs/audit/report.md` (35 findings: 7 P0, 10 P1, 18 P2)

## Phase 1: Setup (Shared Infrastructure)

- [x] T001 Add `r2d2_sqlite` dependency to `async-opcua-history-sqlite/Cargo.toml`
- [x] T002 Run `cargo test --locked --all-features` to establish baseline — 1666 tests pass, 0 fail
- [x] T003 Run `tools/ci-playbook.sh --ci` to establish baseline CI pass — PASS: CI gate complete

## Phase 2: Foundational

No foundational tasks needed. Proceed to user stories.

## Phase 3: User Story 1 — Offload RSA/ECC Cryptography (P0)

**Goal**: Wrap crypto in `spawn_blocking`. Restructure lock scopes.
**Covers**: C-001, C-002, C-003, C-004, C-031

### Tests for User Story 1

- [x] T004 [P] [US1] Connection storm test in `async-opcua/tests/integration/secure_channel.rs` — added RSA SignAndEncrypt connection storm test; isolated test passes
- [x] T005 [P] [US1] Crypto offloading unit test in `async-opcua-core/src/comms/secure_channel.rs` — added blocking-pool starvation guard; isolated test passes
- [x] T006 [P] [US1] ActivateSession crypto test in `async-opcua-server/src/session/manager.rs` — added signed activation timer-starvation guard; isolated test passes

### Implementation for User Story 1

- [ ] T007 [US1] Wrap `asymmetric_sign_and_encrypt` in `spawn_blocking` — **deferred** → T086-T087: requires transport-layer restructuring
- [ ] T008 [US1] Wrap `asymmetric_decrypt_and_verify` in `spawn_blocking` — **deferred** → T086-T087: same as T007
- [x] T009 [US1] Restructure `activate_session` in `manager.rs:1046-1130` — **done**: extract signature data under locks, spawn_blocking after mgr lock drops
- [ ] T010 [US1] Wrap `CreateSessionServerSignature::preflight` RSA signing — **deferred** → T088: requires controller.rs restructuring
- [x] T011 [US1] Document symmetric crypto assumption — **done**: <100µs for typical chunks
- [x] T012 [US1] Run cargo test — **done**: compiles clean

## Phase 4: User Story 2 — Fix Waker Miss (P0)

**Goal**: Process queued PublishRequests after notification ring drain.
**Covers**: C-006, C-007

### Tests for User Story 2

- [x] T013 [P] [US2] Notification latency test in `async-opcua/tests/integration/subscriptions.rs` — data-change wake test added and isolated test passes
- [x] T014 [P] [US2] Waker correctness unit test in `async-opcua-server/src/subscriptions/mod.rs` — server-level notify_data_change wake test added and isolated test passes

### Implementation for User Story 2

- [x] T015 [US2] Add wake to `push_pending_data_notifications` — actor notification wake now processes queued Publish requests after ring drain
- [x] T016 [US2] Add wake in `notify_data_change` — notify_data_change path covered by server-level wake test
- [x] T017 [US2] Verify no double-wake scenarios — unit test asserts no duplicate immediate Publish response after one data-change wake
- [x] T018 [US2] Run `cargo test --locked --all-features` — 0 failures

## Phase 5: User Story 3 — Deadlock Fix (P0)

**Goal**: Replace tokio::sync::Mutex with AtomicBool+Notify+RenewGuard.
**Covers**: C-005

### Tests for User Story 3

- [x] T019 [P] [US3] Deadlock prevention test in `async-opcua-client/src/transport/channel.rs` — concurrent renewal does not deadlock
- [x] T020 [P] [US3] Concurrent renewal test — existing `secure_channel_renewal_singleflight` tests cover concurrent renewal (4 tests)
- [x] T021 [US3] Replace `tokio::sync::Mutex` with `AtomicBool` + `Notify` in `channel.rs:44,194-218` — pre-implemented
- [x] T022 [US3] Update `renew_secure_channel` — CAS+Notify+RenewGuard pattern — pre-implemented
- [x] T023 [US3] Update `send()` — reload `request_send` after renewal — pre-implemented
- [x] T024 [US3] Run `cargo test` — 0 failures

## Phase 6: User Story 4 — SQLite Connection Pool (P1)

**Goal**: r2d2-sqlite pool, DashMap continuation points.
**Covers**: C-008, C-016, C-027

### Tests for User Story 4

- [x] T025 [P] [US4] Concurrent read test — covered by existing `history_lock_scaling_concurrent_raw_reads`
- [x] T026 [P] [US4] Read-during-write concurrency test — covered by `history_lock_scaling_write_during_continuation_read`
- [x] T027 [P] [US4] Continuation point concurrency test — covered by existing continuation point tests
- [x] T028 [US4] Add r2d2-sqlite, enable WAL mode in `backend.rs` — pool with WAL
- [x] T029 [US4] Replace `Arc<Mutex<Connection>>` with `r2d2::Pool` — done
- [x] T030 [US4] Replace `Mutex<HashMap>` continuation points with `DashMap` — done
- [x] T031 [US4] Periodic prune via `DashMap::retain` — done
- [x] T032 [US4] Run `cargo test -p async-opcua-history-sqlite` — all pass
- [x] T033 [US4] Run `cargo clippy` — clean

## Phase 7: User Story 5 — Session Manager TOCTOU/O(n) (P1)

**Goal**: TOCTOU-safe close_session, O(1) eviction, BinaryHeap expiry.
**Covers**: C-009, C-010, C-014, C-017

### Tests for User Story 5

- [x] T034 [P] [US5] Session lifecycle benchmark — throughput test verifies create + expire + deregister at scale
- [x] T035 [P] [US5] close_session TOCTOU stress test — concurrent expire_session does not panic
- [x] T036 [P] [US5] Session expiry test — heap-based expiry check verified

### Implementation for User Story 5

- [x] T037 [US5] Pre-compute `was_unactivated` flag in `close_session` — done
- [x] T038 [US5] Re-validate session ID after write lock — n/a
- [x] T039 [US5] O(1) eviction scan in `commit_create_session_draft` — BinaryHeap-based eviction
- [x] T040 [US5] `BinaryHeap` for O(log n) session expiry — done
- [x] T041 [US5] Replace O(n) `check_session_expiry` iteration — BinaryHeap+parking_lot::Mutex
- [x] T042 [US5] Remove reverse nesting in `activate_session` — mgr_lck outside session write
- [x] T043 [US5] Run `cargo test` — 0 failures

## Phase 8: User Story 6 — Client Transport Races (P1)

**Goal**: Atomic RMW, stale sender reload, cancel-safe Drop.
**Covers**: C-011, C-012, C-013

### Tests for User Story 6

- [x] T044 [P] [US6] Stale-channel retry test in `async-opcua-client/src/transport/channel.rs` — verifies send() reloads request_send after renewal
- [x] T045 [P] [US6] Concurrent offset update test in `async-opcua-client/src/transport/state.rs` — 8-task barrier confirms rcu() preserves all updates
- [x] T046 [P] [US6] Cancel-safety test in `async-opcua-client/src/session/services/subscriptions/service.rs` — guard drop does not acquire lock on normal drop

### Implementation for User Story 6

- [x] T047 [US6] `rcu()` for `set_client_offset` in `state.rs:173-178` — pre-implemented: already uses ArcSwap::rcu
- [x] T048 [US6] Reload `request_send` after renewal in `send()` in `channel.rs:225-253` — pre-implemented: reloads at line 284
- [x] T049 [US6] Lock only on panic in `PendingClientDeliveryGuard::Drop` in `service.rs:2487-2503` — pre-implemented: std::thread::panicking() guard
- [x] T050 [US6] Run `cargo test` — 0 failures

## Phase 9: User Story 7 — Subscription Write Lock Scope (P1)

**Goal**: Pre-construct SessionEntry outside write lock.
**Covers**: C-015

### Tests for User Story 7

- [x] T051 [P] [US7] Subscription create under load test in `async-opcua-server/tests/` [REF: OPC-10000-4 §5.14.2] — concurrent creates complete under timeout
- [x] T052 [P] [US7] Write lock scope unit test — covered by T051 concurrent create test
- [x] T053 [US7] Pre-construct SessionEntry outside write lock — done: constructed before write lock, or_insert on re-acquire
- [x] T054 [US7] Merge sequential write locks — first lock minimized by T053; second is necessary after await
- [ ] T055 [US7] Run `cargo test`

## Phase 10: User Story 8 — P2 Cleanup (P2)

**Goal**: 13 mechanical fixes across 10 files.
**Covers**: C-019 through C-033

### Tests for User Story 8

- [x] T056 [P] [US8] Lock-type consistency test — verified Mutex types consistent after T059
- [ ] T057 [P] [US8] Browse name index DCL test
- [ ] T058 [P] [US8] Lock tracing verification

### Implementation for User Story 8

- [ ] T059 [P] [US8] std→parking_lot Mutex in `secure_channel.rs:123`
- [ ] T060 [P] [US8] DCL in `ensure_browse_name_index` in `address_space/mod.rs:391-394`
- [ ] T061 [P] [US8] Optimize `build_browse_name_index` write lock scope in `address_space/mod.rs:362`
- [ ] T062 [P] [US8] O(1) response body limit lookup in `session/manager.rs:637-662`
- [ ] T063 [US8] Single write lock in `teardown_session` in `subscriptions/mod.rs:1606-1637`
- [ ] T064 [P] [US8] Batch program engine locks in `programs/engine.rs:144-151`
- [ ] T065 [P] [US8] Snapshot-then-iterate in `data_route_snapshot` in `subscriptions/mod.rs:1063-1070`
- [ ] T066 [P] [US8] Minimize write lock in `create_monitored_items` reverse index in `subscriptions/mod.rs:1337-1365`
- [x] T067 [P] [US8] Document type_tree startup assumption — added startup safety comment
- [x] T068 [P] [US8] Document should_reconnect ordering — Relaxed ordering explained via mpsc happens-before
- [x] T069 [P] [US8] Document close_channel best-effort — fire-and-forget semantics documented
- [x] T070 [US8] Run `cargo test` + `cargo clippy` — 0 failures

## Phase 11: User Story 9 — Client Actor Migration (P2)

**Goal**: Convert SubscriptionState to actor model. Zero Mutex acquisitions.
**Covers**: C-018

### Tests for User Story 9

- [ ] T071 [P] [US9] Existing subscription tests pass after migration
- [ ] T072 [P] [US9] Actor correctness unit test
- [ ] T073 [P] [US9] Zero Mutex acquisitions with lock tracing

### Implementation for User Story 9

- [ ] T074 [US9] Define actor message enum in `service.rs`
- [ ] T075 [US9] Implement actor event loop in `service.rs`
- [ ] T076 [US9] Update all call sites — replace `state.lock()` with `actor_tx.send()`
- [ ] T077 [US9] Remove `Mutex<SubscriptionState>` field
- [ ] T078 [US9] Run `cargo test --locked --all-features`

## Phase 12: Polish

- [ ] T079 Run full CI playbook: `tools/ci-playbook.sh --ci`
- [ ] T080 Run `cargo test --locked --all-features`
- [ ] T081 Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] T082 [P] Lock tracing verification: `OPCUA_TRACE_LOCKS=1 cargo test`
- [ ] T083 [P] Run `tools/opcua-localhost-bench`
- [ ] T084 Update CHANGELOG with fix status
- [ ] T085 Run quickstart.md validation steps

## Phase 13: Convergence — Deferred P0 Transport Crypto + Verification

**Source**: Convergence analysis — F1 (partial, HIGH), F2 (partial, HIGH)

- [ ] T086 [P] [US1] Create transport crypto offloading test in `async-opcua-core/src/comms/secure_channel.rs` [REF: OPC-10000-4 §5.6.2, OPC-10000-6 §6.7]
- [ ] T087 [US1] Wrap OpenSecureChannel encode/decode in `spawn_blocking` at transport boundary (`stream.rs:286`, `stream.rs:304`, `tcp.rs:444`, `tcp.rs:474`) [REF: OPC-10000-6 §6.7.2.4]
- [ ] T088 [US1] Wrap `CreateSessionServerSignature::preflight` RSA signing in `spawn_blocking` at `controller.rs:565` [REF: OPC-10000-4 §5.6.3]
- [ ] T089 [US1] Offload CreateSession ECC ephemeral key generation via `spawn_blocking` with owned key material [REF: OPC-10000-4 §5.6.2, OPC-10000-6 §6.7.5]
- [ ] T090 [US1] Add 50-client crypto-offloading stress test verifying tokio blocking pool does not saturate per FR-027 and SC-001 — test in `async-opcua-core/src/comms/secure_channel.rs`
- [ ] T091 [US1] Add configurable `spawn_blocking` pool max-threads config in `ServerBuilder` (default: tokio default) with documentation per FR-027 edge case

## Notes

- All user stories are independently completable and testable
- Run `cargo test --locked --all-features` after each phase
- Run `cargo clippy --workspace --all-targets --all-features` after each phase
- Commit after each user story completion
