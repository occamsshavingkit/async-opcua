# Tasks: Hot-Path and Lock Optimization

**Input**: Design documents from `/specs/063-hot-path-and-locks/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: No new tests required — all existing tests must continue to pass. Throughput verified via `tools/opcua-localhost-bench` baseline comparison.

**Organization**: Tasks are grouped by user story for independent implementation. All four stories are independent and can be done in parallel.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Include exact file paths in descriptions

---

## Phase 1: Benchmark Baseline

**Purpose**: Record pre-optimization throughput for before/after comparison.

- [X] T001 Record localhost-bench baseline (read + write, 3 runs each) per quickstart.md

---

## Phase 2: User Story 1 — Split AddressSpace Hot/Cold (Priority: P1)

**Goal**: Eliminate `parking_lot::RwLock::read()` overhead on every Read by exposing `DashMap<NodeId, NodeType>` directly. Cold fields (`references`, `browse_name_index`, `namespaces`) stay behind `RwLock<AddressSpaceCold>`.

**Independent Test**: Perf shows zero `parking_lot::RwLock::read` contention on read path. Localhost-bench throughput improves.

**Spec reference**: OPC-10000-4 §7.1 (Read service — address space access), §7.2 (Write service — cold field mutation).

### Implementation for User Story 1

- [X] T002 [US1] Define `AddressSpaceCold` struct with `references`, `browse_name_index`, `namespaces` fields in `async-opcua-server/src/address_space/mod.rs` — OPC-10000-4 §7.1, §7.2
- [X] T003 [US1] Refactor `AddressSpace` to hold `Arc<DashMap<NodeId, NodeType>>` + `RwLock<AddressSpaceCold>` instead of embedded fields in `async-opcua-server/src/address_space/mod.rs` — OPC-10000-4 §7.1
- [X] T004 [US1] Update all Read-path call-sites in `async-opcua-server/src/node_manager/memory/mod.rs` to use `address_space.node_map.get()` directly instead of `address_space.read().node_map.get()` — OPC-10000-4 §7.1
- [X] T005 [US1] Update AddNode/DeleteNode call-sites to acquire `address_space.cold.write()` instead of `address_space.write()` in `async-opcua-server/src/address_space/mod.rs` — OPC-10000-4 §7.2
- [X] T006 [US1] Update AddressSpace constructor and initialization to match new structure in `async-opcua-server/src/address_space/mod.rs` — OPC-10000-4 §7.1, §7.2
- [X] T007 [US1] Build and run `cargo test --all-features` to verify no regressions; fix any compilation errors from changed access patterns
- [X] T008 [US1] Profile read path with perf to confirm zero `RwLock::read` contention on read path

**Checkpoint**: AddressSpace split complete. Read path is lock-free for `node_map` access.

---

## Phase 3: User Story 2 — Cache Session Arc in Dispatch Context (Priority: P2)

**Goal**: Cache `SessionManager::find_by_token` result in per-request context to avoid repeated ~2.4% CPU hash-table lookups.

**Independent Test**: Perf shows `SessionManager::find_by_token` no longer in top CPU consumers per request.

**Spec reference**: OPC-10000-6 §6.4 (Session service — session token lookup).

### Implementation for User Story 2

- [X] T009 [US2] Identify the request dispatch context struct where session token lookup occurs in `async-opcua-server/src/session/controller.rs` — OPC-10000-6 §6.4
- [X] T010 [US2] Add `cached_session: Option<(NodeId, Arc<RwLock<Session>>)>` field to the request dispatch context — OPC-10000-6 §6.4.1
- [X] T011 [US2] Modify first `find_by_token` call-site to populate the cache; modify subsequent access sites to use cached value in `async-opcua-server/src/session/controller.rs` — OPC-10000-6 §6.4.1
- [X] T012 [US2] Build and run `cargo test --all-features` to verify no regressions; fix any compilation errors
- [X] T013 [US2] Profile to confirm `find_by_token` CPU time per request is eliminated (cached after first call)

**Checkpoint**: Session Arc cached. No redundant token lookups during request dispatch.

---

## Phase 4: User Story 3 — Replace Per-Request Timers With Shared Deadline Queue (Priority: P3)

**Goal**: Replace N individual `tokio::time::sleep_until` futures with a single `BTreeMap<Instant, Vec<RequestId>>` checked once per event loop tick, reducing ~2.8% `TimerEntry` overhead.

**Independent Test**: Perf shows `TimerEntry::drop`/`TimerEntry::reset` CPU overhead reduced by ≥50%.

**Spec reference**: OPC-10000-4 §5.7.2 (Service timeout handling).

### Implementation for User Story 3

- [X] T014 [US3] Define `DeadlineQueue` struct with `BTreeMap<Instant, Vec<RequestId>>` and `HashSet<RequestId>` for lazy cleanup in `async-opcua-server/src/session/controller.rs` — OPC-10000-4 §5.7.1, §5.7.2
- [X] T015 [US3] Implement `DeadlineQueue::push()`, `pop_expired()`, and `mark_completed()` methods in `async-opcua-server/src/session/controller.rs` — OPC-10000-4 §5.7.2
- [X] T016 [US3] Replace per-request `tokio::time::sleep_until` in `FuturesUnordered` with `DeadlineQueue` integration: push on dispatch, pop on tick, mark on completion in `async-opcua-server/src/session/controller.rs` — OPC-10000-4 §5.7.2
- [X] T017 [US3] Integrate deadline queue check into event loop `run()` — check `pop_expired` once per iteration in `async-opcua-server/src/session/controller.rs` — OPC-10000-4 §5.7.2
- [X] T018 [US3] Build and run `cargo test --all-features` to verify no regressions; run interop tests (`tools/ci-playbook.sh --ci`) to verify timeout behavior unchanged
- [X] T019 [US3] Profile to confirm `TimerEntry::drop`/`TimerEntry::reset` overhead is reduced by ≥50%

**Checkpoint**: Shared deadline queue operational. No per-request timer allocation.

---

## Phase 5: User Story 4 — Investigate and Resolve ArcSwap Debt Overhead (Priority: P4)

**Goal**: Profile `arc_swap::Debt::pay_all` (~2.5% CPU), identify which ArcSwap instances are responsible, and apply a targeted fix (plain `Arc` for startup-only data, or generation-counter pattern for rare-write config).

**Independent Test**: Perf shows `arc_swap::Debt::pay_all` CPU reduced by ≥50% OR a documented finding explains why it cannot be reduced.

**Spec reference**: OPC-10000-5 §6.3 (Server information model — ServerInfo access pattern).

### Implementation for User Story 4

- [X] T020 [US4] Profile with `perf record -g` to identify which `ArcSwap::load()` call-sites contribute to `Debt::pay_all` overhead; document finding in research.md
- [X] T021 [US4] For each identified ArcSwap instance, classify as startup-only (replace with plain `Arc`), rare-write (replace with `Arc` + generation counter), or concurrent (keep ArcSwap) — OPC-10000-5 §6.3
- [X] T022 [US4] Apply replacement: for startup-only instances, change `ArcSwap<T>` to `Arc<T>` and update all load sites in `async-opcua-core/src/comms/` and `async-opcua-server/src/` — OPC-10000-5 §6.3
- [X] T023 [US4] Apply replacement: for rare-write instances, implement `Arc` + `AtomicU64` generation counter pattern and update read/write sites in `async-opcua-core/src/comms/` and `async-opcua-server/src/` — OPC-10000-5 §6.3
- [X] T024 [US4] Build and run `cargo test --all-features` to verify no regressions; fix any compilation errors
- [X] T025 [US4] Profile to confirm `arc_swap::Debt::pay_all` CPU is reduced by ≥50%; if no viable replacement exists, document finding with rationale

**Checkpoint**: ArcSwap debt overhead minimized or documented as irreducible.

---

## Phase 6: Final Verification & Polish

**Purpose**: Aggregate verification, benchmark, and cleanup.

- [X] T026 Record final localhost-bench throughput (read + write, 3 runs each), compare to T001 baseline, and verify no allocation regressions with `dhall` / `valgrind --tool=massif` on demo-server under load
- [X] T027 Run full CI playbook `tools/ci-playbook.sh --ci` — all steps must pass
- [X] T028 Update TODO.md to mark completed hot-path/lock items as done

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Baseline)**: No dependencies — can start immediately
- **Phase 2 (US1)**: No dependencies on other stories — can start immediately
- **Phase 3 (US2)**: No dependencies on other stories — can start immediately
- **Phase 4 (US3)**: No dependencies on other stories — can start immediately
- **Phase 5 (US4)**: No dependencies on other stories — can start immediately (but profile before making changes)
- **Phase 6 (Final)**: Depends on all user stories being complete

### User Story Dependencies

All four user stories are **independent** — they touch different modules and data structures:
- US1: `address_space/mod.rs`, `node_manager/memory/mod.rs`
- US2: `session/controller.rs`
- US3: `session/controller.rs` (different code path from US2)
- US4: `comms/` (core crate), potentially `session/` (server crate)

### Within Each User Story

- Structural changes (struct definition) before refactoring call-sites
- Compilation fix iterations before benchmark verification
- Story complete and passing tests before profiling

### Parallel Opportunities

- All four user stories (Phase 2-5) can be implemented in parallel
- US2 and US3 both touch `controller.rs` but in different code paths (session caching vs deadline queue) — if parallel, coordinate to avoid merge conflicts
- Recommended: sequential (US1 → US2 → US3 → US4) for clean commit history and incremental profiling

---

## Parallel Example: User Story 1

```bash
# Phase 2 tasks within US1 are sequential (struct first, then call-sites)
# But US1, US2, US3, US4 can all be worked on in parallel if desired
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Benchmark baseline
2. Complete Phase 2: US1 (AddressSpace split) — this is the highest-impact change
3. **STOP and VALIDATE**: Profile, verify throughput improvement
4. Proceed to next story

### Incremental Delivery

1. Baseline recorded → comparison point established
2. US1 → Profile → Verify lock contention eliminated
3. US2 → Profile → Verify token lookup eliminated
4. US3 → Profile → Verify timer overhead reduced
5. US4 → Profile → Verify/ document ArcSwap debt
6. Final: Aggregate benchmark → Full CI pass → TODO.md updated

### Sequential Strategy

Recommended: implement one story at a time, profile to confirm improvement, then commit before moving to next. This gives clean bisect if any story causes a regression.

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story must be independently verifiable via perf/benchmark
- All existing tests (unit, integration, interop) must pass after each story
- Commit after each story completion for clean history
- If US4 (ArcSwap) investigation reveals no viable fix, document with rationale in research.md
