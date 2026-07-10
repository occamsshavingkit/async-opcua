---
description: "Task list for feature 072 — Hot-Path Per-Request Throughput"
---

# Tasks: Hot-Path Per-Request Throughput

**Input**: Design documents from `/specs/072-hot-path-throughput/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Included — the spec explicitly requires fast-path panic-isolation, read equivalence, and
conformance byte-identity (FR-002/FR-004, contracts C2/C3).

**Golden rule (contracts C1)**: measure before, measure after, keep only what the numbers justify. A perf
change that regresses or is neutral-for-a-perf-goal is reverted (safety cleanups may still land, noted).

**Organization**: by user story. US1 (P1) is the MVP; US2 (P2) is measure-first and gated.

## Format: `[ID] [P?] [Story] Description`
- **[P]**: parallelizable (different files, no incomplete dependency)
- **[Story]**: US1 / US2 (setup/foundational/polish carry no story label)

---

## Phase 1: Setup

- [ ] T001 Confirm the bench harness + CPU-isolation procedure per `~/scratch/OPCUA-BENCHMARK.md` (isolated pinned server core, clients elsewhere) and record the exact pinned-core invocation in `specs/072-hot-path-throughput/research.md` under "measurement instrument".

---

## Phase 2: Foundational — Task 0 HEAD re-baseline (BLOCKING; contracts C1 G-baseline)

**No code change in Phase 3+ may be committed until this phase is complete and recorded.** The existing
profiles are June-30 vintage (pre-062/063) and cannot be trusted.

- [ ] T002 Capture HEAD single-client single-core Read + Write throughput (median of ≥3, pinned core) via `tools/opcua-localhost-bench`; record `single_core_read_ops_s` / `single_core_write_ops_s` in `research.md`.
- [ ] T003 [P] Capture HEAD single-thread `perf record -e cycles:u` Read self-time profile; record the top self-time buckets in `research.md` (confirm/refresh the ~23% plumbing / ~9.5% chunk / clock_gettime picture at HEAD).
- [ ] T004 [P] Capture HEAD multi-core concurrency sweep (clients 1,2,4,8,16,24,32 on N cores, `#[tokio::main]` multi-thread server); record `sweep`, `per_core_efficiency`, `plateau_point` in `research.md`.
- [ ] T005 [P] Capture the MISSING HEAD multi-thread `perf c2c`/HITM + off-CPU/wakeup profile; record `c2c_hitm_profile` and `offcpu_profile` in `research.md` (US2 depends on this).

**Checkpoint**: baselines recorded → US1/US2 may begin.

---

## Phase 3: User Story 1 — Single-core per-request reduction (Priority: P1) 🎯 MVP

**Goal**: cut trimmable per-request work so single-client single-core throughput comes close to peers.
**Independent test**: single-client Read (+Write) throughput on one pinned core improves vs the T002 baseline
with no correctness/protocol change.

### Stage 1 — safe cuts (each measured independently; all should land)

- [ ] T006 [P] [US1] **S1a**: replace `last_service_request: ArcSwap<Instant>` with an `AtomicU64` of monotonic nanos in `async-opcua-server/src/session/instance.rs` (field ~:113; `validate_timed_out` ~:224-227) — read the clock once, no per-request `Arc` alloc. Preserve the timeout-comparison semantics (data-model "session-activity timestamp").
- [ ] T007 [US1] **S1a gate**: bench single-client Read+Write vs T002 baseline (pinned core); record before/after in `research.md`; keep iff improve/neutral (contracts C1 G-us1), else revert.
- [ ] T008 [P] [US1] **S1b**: thread `&DecodingOptions` into `ChunkInfo::new` (`async-opcua-core/src/comms/message_chunk_info.rs` ~:42-79) to remove the internal `decoding_options()` RwLock+clone (~:45); make `chunk_info()` a `OnceLock<Arc<ChunkInfo>>` returning a cheap `Arc` clone (`message_chunk.rs` ~:151,:434-445); collapse the 3 header parses in `SecureChannel::apply_security` (`secure_channel.rs` ~:883,:896,:905) into one `chunk_info()`.
- [ ] T009 [US1] **S1b gate + correctness**: `cargo test -p async-opcua-core` (chunk/secure-channel tests green, byte-identical `ChunkInfo`); bench vs baseline; keep iff improve/neutral, record.
- [ ] T010 [P] [US1] **S1c**: gate the always-on actor timing (`async-opcua-server/src/session/actor.rs` ~:133 and `record_message_processed` ~:197) behind `#[cfg(feature = "diagnostics")]`, matching `response_metrics` (`controller.rs:367`).
- [ ] T011 [US1] **S1c gate**: verify no protocol-visible diagnostic value changes; bench vs baseline (default features = diagnostics-off path); record.
- [ ] T012 [P] [US1] **S1d**: in `async-opcua-server/src/session/controller.rs` (~:289-297) rebuild the two `sleep_until` timers only when the earliest deadline changes; reuse one `Instant::now()` per loop turn (~:309/:865).
- [ ] T013 [US1] **S1d gate + correctness**: existing timeout/cancellation tests green (deadlines fire at the same wall-clock time); bench vs baseline; record.

### Stage 2 — read fast-path bypassing the SessionActor (gated big lever; contracts C3)

- [ ] T014 [US1] **S2 refactor**: extract `SessionActor::read` (`async-opcua-server/src/session/actor.rs` ~:267-329) into a free function `(RequestContext, NodeManagers, nodes, max_age, tsr, diagnostics) -> Result<Vec<DataValue>, StatusCode>`, reusing `invoke_service_concurrently_mut` (`session/services/mod.rs:62`); the actor path calls the free function (behavior unchanged) to prove the extraction.
- [ ] T015 [US1] **S2 fast path**: in `MessageHandler::read` (`async-opcua-server/src/session/message_handler.rs` ~:730), route pure Value-attribute reads straight to the free function via `request_context_from_parts` (~:169), skipping the `mpsc` send (~:810) + `oneshot` (~:809/824) + actor wakeup; still return `AsyncMessage(JoinHandle)`; **wrap the read in `AssertUnwindSafe(...).catch_unwind()`** exactly as `actor.rs:323` (contracts C3.2). Non-Value reads and writes keep the actor path.
- [ ] T016 [P] [US1] **Test (C3.2)**: `fast_path_read_panic_is_isolated` — a node manager rigged to panic on read yields `BadInternalError` for that request and a subsequent request on the same connection succeeds (connection not closed).
- [ ] T017 [P] [US1] **Test (C3.1)**: `fast_path_read_matches_actor_path` — all attributes × all security policies read identically on the fast path and the actor path (OPC-10000-4 §5.10 Read semantics unchanged).
- [ ] T018 [P] [US1] **Test (C3.3)**: a fast-path read past its deadline is still aborted (cancellation preserved).
- [ ] T019 [US1] **S2 GATE (C1 G-us1-stage2)**: bench single-client + a small aggregate run vs baseline; **keep S2 only if it clears ≥1.5× (SC-001) or a material single-client+aggregate win**; otherwise revert T014-T018. Record the decision + numbers in `research.md`.

**Checkpoint (US1 done)**: run the equivalence guard — `cargo test -p async-opcua --test integration_tests --features all,json,xml,legacy-crypto,wss,pubsub,history conformance::` (byte-identical, contracts C2) — and `cargo test -p async-opcua-server`. US1 is independently shippable here.

---

## Phase 4: User Story 2 — Multi-core linear scaling (Priority: P2, measure-first)

**Goal**: stay near-linear for more cores before the plateau.
**Independent test**: concurrency-sweep per-core efficiency degrades less / plateau moves higher vs the
T004 baseline, each change backed by a `perf c2c` HITM drop (contracts C1 G-us2).

- [ ] T020 [US2] **Measurement analysis (gate opener)**: from the T005 `c2c`/off-CPU profile, identify the specific confirmed contention lines/costs and record in `research.md` which US2 changes below are justified. Do NOT implement a US2 change whose contention this analysis does not confirm.
- [ ] T021 [P] [US2] Pool the per-request fan-out `Vec`s (`session/services/mod.rs` ~:77-79, `session/actor.rs` ~:275/:328, `node_manager/memory/mod.rs` ~:862/:881, `simple.rs` ~:246) using the existing per-session `DataChangeNotificationVecPool` pattern (`subscriptions/subscription.rs:25`) — only if justified by T020.
- [ ] T022 [US2] Gate T021: multi-core sweep + `perf c2c` before/after; keep iff per-core efficiency improves AND allocation/contention drops; record.
- [ ] T023 [US2] Reduce the outer-lock **double** acquisition per Value read: route pure Value reads through the lock-free `node_map` + `TypeTreeSnapshot` (`info.rs:117/213`) without the outer `RwLock<AddressSpace>`, **keeping the outer barrier for `browse`/structural writes** (`node_manager/memory/mod.rs` ~:864, `simple.rs` ~:247) — correctness-preserving (contracts C4/FR-006).
- [ ] T024 [US2] **Test (FR-006)**: concurrent `AddNodes`/`DeleteNodes` (OPC-10000-4 §5.7 NodeManagement) vs `Browse`/Read (§5.8/§5.10) observes no torn `node_map`↔`cold` state (the outer barrier still holds for structural writes).
- [ ] T025 [US2] Gate T023: multi-core sweep + `perf c2c` HITM on the address-space line before/after; keep iff efficiency improves AND HITM drops AND T024 green; record.
- [ ] T026 [P] [US2] Convert `read_cbs`/`write_cbs` `RwLock<HashMap>` → `DashMap` (`node_manager/memory/simple.rs` ~:170) — only if T020 confirms the callback-map line contends.
- [ ] T027 [US2] Gate T026: sweep + `c2c` before/after; keep iff it moves the number; record.

---

## Phase 5: Polish & Cross-Cutting

- [ ] T028 [P] Consolidate all before/after numbers into a `docs/hot-path-072-report.md` (SC-006): per-change deltas, final single-core throughput, final sweep/plateau, and which gated items were kept vs reverted.
- [ ] T029 Verify the full equivalence + correctness set: conformance byte-identical (SC-004), `cargo test --workspace` green (SC-005), no new `clippy` warnings, **FR-007** (no new locks/mutexes/blocking primitives — `clippy::await_holding_lock` clean; only equal-or-cheaper replacements), and **FR-008** (security/crypto behavior unchanged — `security_tests` + conformance green).
- [ ] T030 Full pre-PR gate: `tools/ci-playbook.sh --ci` (must pass before opening the PR on the fork).

---

## Dependencies & order

- **Phase 2 (T002–T005) BLOCKS everything** (contracts C1 G-baseline).
- **US1 (Phase 3)** is the MVP and independent of US2. Stage 1 (T006–T013) before Stage 2 (T014–T019); each Stage-1 cut is independent ([P] across different files) but each pairs with its own gate task.
- **US2 (Phase 4)** depends on T005 (the c2c/off-CPU baseline) and T020 (analysis) before any US2 change; T023 also benefits from S2 (T015) being decided first (the value-read path is where the lock reduction applies).
- **Polish (Phase 5)** last.

## Parallel opportunities

- Phase 2: T003/T004/T005 run in parallel (different captures).
- US1 Stage 1: the *edits* T006/T008/T010/T012 are different files ([P]); their gate tasks are sequential per edit.
- US1 Stage 2 tests: T016/T017/T018 [P].
- US2: T021 and T026 are independent of T023 ([P]) once T020 justifies them.

## Implementation strategy (MVP first)

1. **MVP = US1 Stage 1** (T001–T013): the safe cuts. Ship on their own if desired — measurable single-core win, zero risk.
2. **US1 Stage 2** (T014–T019): the read fast-path, kept only if it clears the gate.
3. **US2** (T020–T027): measure-first, each item gated on a HITM number.
4. Commit per user story (and per gated Stage/US item where a revert boundary matters).

## Notes
- Every gate task records before/after numbers in `research.md` (SC-006); a regressing perf change is reverted, not kept.
- No new locks/mutexes/blocking primitives (contracts C5 / AGENTS.md) — S1a/S1b/US2 *replace* existing sync with equal-or-cheaper. If any new synchronization becomes unavoidable, run the `audit-locks` skill first.
- Line numbers are HEAD-approximate; confirm against current code when implementing.
