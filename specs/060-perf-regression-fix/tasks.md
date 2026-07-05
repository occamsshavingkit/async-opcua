# Tasks: Performance Regression Fix — Localhost Benchmark

**Input**: Design documents from `/specs/060-perf-regression-fix/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, contracts/profiling.md

**Tests**: No new tests required. Existing test suite (`cargo test --locked --all-features`) serves as the regression guard for all changes. The benchmark (`tools/opcua-localhost-bench`) provides throughput measurement after each fix.

**Organization**: Tasks are grouped by user story. US1 gates US2 and US3 (profile first to confirm mechanism). US2 and US3 are independent (different files). US4 is P2 and runs after US2+US3.

**OPC UA Spec Citations**: Not applicable. This feature addresses indirect compilation effects — no OPC UA specification behavior is changed.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Include exact file paths in descriptions

---

## Phase 1: Setup — Establish Baseline

**Goal**: Record the pre-fix benchmark throughput at HEAD so every subsequent fix can be measured against it.

- [x] T001 Build release binary at HEAD and record baseline benchmark throughput (both read and write, 3 runs each, report median) using `cargo build --release --bin async-opcua-localhost-bench && ./target/release/async-opcua-localhost-bench run --op read`
- [x] T002 [P] Build release binary at pre-059 baseline (commit `983b222a7`) and record benchmark throughput (both read and write, 3 runs each, report median) using `git worktree` or `git checkout` + same build/bench steps

---

## Phase 2: User Story 1 — Profile to Confirm Root Cause (Priority: P1)

**Goal**: Run `perf stat` with hardware performance counters on both HEAD and pre-059 baseline builds to confirm the mechanism driving the 27% regression (i-cache pressure, de-inlining, or `.text` layout disruption).

**Independent Test**: Run `perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses` on both builds and confirm at least one counter differs by >5%.

**Spec grounding**: FR-001, FR-002, FR-003; contracts/profiling.md

### Implementation for User Story 1

- [x] T003 [US1] Run `perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses` on the HEAD (commit `765434c3b`) release binary of `async-opcua-localhost-bench` in read mode, 3 runs, record raw counters and benchmark `ok` count for per-request normalization in `docs/perf-analysis.md`
- [x] T004 [US1] Run `perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses` on the pre-059 baseline (commit `983b222a7`) release binary of `async-opcua-localhost-bench` in read mode, 3 runs, record raw counters and benchmark `ok` count for per-request normalization in `docs/perf-analysis.md`
- [x] T005 [US1] Compare HEAD vs. baseline per-request metrics (instructions/req, cache-misses/req, branch-misses/req, L1i-misses/req), document the comparison table with deltas in `docs/perf-analysis.md`, and record CPU model/cache topology via `lscpu`

**Checkpoint**: Profiling confirms at least one statistically significant regression metric. The mechanism is identified, validating or adjusting the fix strategy.

---

## Phase 3: User Story 2 — Roll Back VIEW-03 Refactoring (Priority: P1)

**Goal**: Inline `strip_result_mask_fields()` back into `BrowseNode::add()` and `BrowseNode::add_unchecked()` to restore the pre-059 compilation-unit layout. Preserve the result mask stripping behavior from the VIEW-03 compliance fix.

**Independent Test**: `cargo test -p async-opcua-server -- node_manager` verifies result mask filtering still works after the inline revert.

**Spec grounding**: FR-004, FR-005, FR-006

### Implementation for User Story 2

- [x] T006 [US2] Inline `strip_result_mask_fields()` back into its two call sites in `async-opcua-server/src/node_manager/view.rs`: remove the standalone `strip_result_mask_fields()` method (lines 423-458) and expand the inline field-clearing code at `add_unchecked()` (line 308-311) and `add()` (lines 403-421), preserving the same result mask filtering logic for all five fields (browse_name, display_name, node_class, reference_type_id, type_definition)
- [x] T007 [US2] Run `cargo test -p async-opcua-server -- node_manager` to verify inline mask stripping works; then run `cargo test --locked --all-features` to verify no regressions from the revert
- [x] T008 [US2] Run `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` and `cargo fmt --all -- --check` to verify no warnings introduced
- [x] T009 [US2] Rebuild release binary and run benchmark (read mode, 3 runs, report median) to measure throughput improvement from VIEW-03 revert alone; record result in `docs/perf-analysis.md`

**Checkpoint**: VIEW-03 reverted. Result mask filtering behavior preserved. Throughput measured.

---

## Phase 4: User Story 3 — Add #[inline] on Hot-Path Functions (Priority: P1)

**Goal**: Add `#[inline]` annotations to the request dispatch hot path in `controller.rs` and `instance.rs` to counteract LLVM de-inlining from code-size heuristics.

**Independent Test**: `cargo build --release --bin async-opcua-localhost-bench` succeeds with no new warnings; `cargo test --locked --all-features` passes.

**Spec grounding**: FR-007, FR-008, FR-009

### Implementation for User Story 3

- [x] T010 [US3] Add `#[inline]` to `process_request()` (line 382) and `validate_request()` (line 995) in `async-opcua-server/src/session/controller.rs`
- [x] T011 [P] [US3] Add `#[inline]` to `validate_activated()` (line 251) and `validate_timed_out()` (line 223) in `async-opcua-server/src/session/instance.rs`
- [x] T012 [US3] Run `cargo build --release --bin async-opcua-localhost-bench` to confirm compilation with `#[inline]` annotations; then run `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` and `cargo test --locked --all-features`
- [x] T013 [US3] Rebuild release binary and run benchmark (read mode, 3 runs, report median) to measure throughput improvement from `#[inline]` annotations alone; record result in `docs/perf-analysis.md`

**Checkpoint**: Hot-path annotated. No behavior change. Throughput measured.

---

## Phase 5: User Story 4 — Tune Release Profile (Priority: P2)

**Goal**: Add `codegen-units = 1` and `lto = true` to the workspace `[profile.release]` in `Cargo.toml` to give LLVM full visibility for inlining decisions.

**Independent Test**: `cargo build --release --bin async-opcua-localhost-bench` succeeds; `cargo check --no-default-features -p async-opcua -p async-opcua-types -p async-opcua-nodes -p async-opcua-server` passes.

**Spec grounding**: FR-010, FR-011, FR-012

### Implementation for User Story 4

- [x] T014 [US4] Add `[profile.release]` section to workspace `Cargo.toml` with `codegen-units = 1` and `lto = true`, placed after the existing `[profile.dev.package.*]` blocks (around line 98) and before `[profile.embedded]` (around line 135)
- [x] T015 [US4] Run `cargo build --release --bin async-opcua-localhost-bench` to confirm release build with new profile; then run `cargo test --locked --all-features`
- [x] T016 [P] [US4] Run `RUSTFLAGS="-D warnings" cargo check --no-default-features -p async-opcua -p async-opcua-types -p async-opcua-nodes -p async-opcua-server` to verify no-default-features builds are not broken by profile change
- [x] T017 [US4] Rebuild release binary and run final benchmark (read mode, 3 runs, report median) to measure cumulative throughput after all fixes; record final result in `docs/perf-analysis.md`

**Checkpoint**: Release profile tuned. All builds pass. Final benchmark recorded.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Goal**: Run the full CI playbook and update the session handoff.

- [x] T018 Run the full CI playbook via `tools/ci-playbook.sh --ci` to confirm all gates pass with all fixes applied
- [x] T019 Re-run `perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses` on the final binary (after all fixes applied) and confirm hardware counter regressions identified in T005 have decreased toward baseline levels — document post-fix counters in `docs/perf-analysis.md`
- [x] T020 Compare final benchmark throughput against pre-059 baseline (from T002) and verify SC-001: throughput recovered to within 5% of baseline (~85,500 req/sec). Document acceptance outcome in `docs/perf-analysis.md`
- [x] T021 [P] Verify by code inspection that all 23 feature 059 compliance findings remain present and unaltered — focus inspection on `async-opcua-server/src/node_manager/view.rs` (VIEW-03 area most likely disturbed by revert) and cross-reference the finding list in `docs/spec-compliance-audit-2026-07-05.md`
- [x] T022 [P] Update `specs/SESSION-HANDOFF.md` with feature 060 summary: fixes applied, profiling results, final throughput, and any residual gap

---

## Dependencies

```
Phase 1: T001 ← T002 (parallel)
    │
Phase 2: T003, T004 (parallel) → T005 (comparison depends on both)
    │
    ├── Phase 3 (US2): T006 → T007 → T008 → T009
    │
    ├── Phase 4 (US3): T010, T011 (parallel) → T012 → T013
    │
    └── Phase 5 (US4): T014 → T015, T016 (parallel) → T017
                              │
                          Phase 6: T018 → T019 → T020; T021, T022 (parallel after T017)
```

- **T001, T002**: Independent, can run in parallel (different git checkouts)
- **T011**: Independent from T010 — different files, can run in parallel with T010
- **T016**: Independent from T015 — different build target, can run in parallel with T015
- **US2 and US3**: Independent phases — can run in parallel after Phase 2 (different files)
- **US4**: Depends on US2 and US3 completing (cumulative fix, need both preceding fixes applied)
- **T021, T022**: Independent from each other — can run in parallel after T017

## Parallel Execution Opportunities

### Phase 1 (Setup)
```
Agent A: T001 (baseline at HEAD)
Agent B: T002 (baseline at 983b222a7) — parallel, different checkout
```

### Phase 2 (Profiling)
```
(sequential: T003+T004 can run parallel, T005 must wait for both)
Agent A: T003 (perf stat on HEAD)
Agent B: T004 (perf stat on baseline) — parallel, different checkout
```

### Phase 3 (US2) and Phase 4 (US3) — after Phase 2
```
Agent A: T006 (VIEW-03 revert)
Agent B: T010 (add #[inline] to controller.rs) + T011 (add #[inline] to instance.rs) — parallel with US2
```

## Implementation Strategy

### MVP (Just User Story 1)
Profile first to confirm the mechanism before any code changes. This is gating: if profiling shows no measurable hardware-counter difference, the hypothesis is wrong and the fix strategy must be re-evaluated.

### Recommended Delivery Order
1. **T001-T002**: Record baselines (parallel)
2. **T003-T005**: Profile and confirm mechanism
3. **T006-T009**: Apply VIEW-03 revert (targeted, reversible)
4. **T010-T013**: Apply `#[inline]` annotations (targeted, safe)
5. **T014-T017**: Apply release profile tuning (broad, most compile-time impact)
6. **T018-T022**: Full CI gate, post-fix profiling, SC-001 verification, compliance audit, handoff

### Verification After Each Phase
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings`
- `cargo test --locked --all-features`
- Benchmark run (read mode, 3 runs)
