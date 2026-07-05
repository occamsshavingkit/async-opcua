# Feature Specification: Performance Regression Fix — Localhost Benchmark

**Feature Branch**: `060-perf-regression-fix`  
**Created**: 2026-07-05  
**Status**: Draft  
**Input**: User description: "Fix the 27% throughput regression (90k → 66k req/sec) in the localhost read/write benchmark caused by indirect compilation effects from feature 059 spec-compliance-audit-fixes. Apply fixes: profile with perf stat to confirm cache miss increase, roll back view.rs VIEW-03 refactoring, #[inline(always)] on hot-path functions, and codegen-units=1 + lto=true in release profile."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Profile to Confirm Root Cause (Priority: P1)

The throughput regression (90k → 66k req/sec) was hypothesized to be caused by indirect compilation effects: instruction cache pressure, LLVM de-inlining hot functions, or `.text` section layout disruption. Before applying fixes, the exact mechanism must be confirmed with hardware performance counters.

**Why this priority**: Profiling first avoids wasting effort on fixes that don't match the actual mechanism. The fix candidates have different expected impacts depending on whether the bottleneck is i-cache, branch prediction, or function call overhead.

**Independent Test**: Run `perf stat -e instructions,cycles,cache-misses,branch-misses` on both the pre-059 baseline (commit `983b222a7`) and current HEAD (commit `765434c3b`) builds. Confirm a statistically significant increase in cache misses, branch misses, or instructions per request.

**Acceptance Scenarios**:

1. **Given** a release build at commit `983b222a7` (pre-059 baseline), **When** the benchmark runs with `perf stat`, **Then** hardware counter data is collected for the baseline.
2. **Given** a release build at commit `765434c3b` (HEAD, post-059), **When** the benchmark runs with `perf stat`, **Then** hardware counter data is collected for HEAD.
3. **Given** profiling data from both builds, **When** the counters are compared, **Then** the comparison identifies at least one statistically significant regression metric (e.g., cache-misses > 10% increase, instructions-per-request > 5% increase, or branch-misses > 10% increase).

---

### User Story 2 - Roll Back VIEW-03 Refactoring (Priority: P1)

The `strip_result_mask_fields()` method was extracted from being inline in `BrowseNode::add()` during feature 059 (VIEW-03 fix). This extraction changes how LLVM lays out `BrowseNode` methods in the compilation unit, potentially triggering cascading inlining threshold effects on hot-path methods in the same crate. Rolling this back to inline field-stripping restores the pre-059 compilation-unit layout.

**Why this priority**: VIEW-03 is the only refactoring in the 059 diff that changes struct method layout on a frequently instantiated type (`BrowseNode`). Reverting it is the most targeted fix — it directly undoes the layout change without adding new annotations.

**Independent Test**: After reverting VIEW-03, run `cargo test --locked --all-features` to confirm the result mask filtering still works correctly (the VIEW-03 fix added result mask stripping to `add_unchecked()` and `add()` — this behavior must be preserved even if the method is inlined again).

**Acceptance Scenarios**:

1. **Given** the VIEW-03 refactoring is reverted (inlining `strip_result_mask_fields` back into `add()` and `add_unchecked()`), **When** `cargo test --locked --all-features` runs, **Then** all tests pass, including view-related tests.
2. **Given** the revert is applied, **When** `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` runs, **Then** no new warnings appear.
3. **Given** the revert is applied, **When** the localhost benchmark runs, **Then** throughput is measured and compared against the pre-revert baseline.

---

### User Story 3 - Add #[inline] on Hot-Path Functions (Priority: P1)

LLVM uses code-size heuristics to decide whether to inline functions at call sites. The added ~1,100 lines from feature 059 increase the crate code size, which can push hot-path functions past LLVM's inlining cost threshold — turning previously-inlined code into function calls. Adding `#[inline]` to key hot-path functions in `controller.rs` and `message_handler.rs` instructs LLVM to prioritize inlining these functions regardless of code-size heuristics.

**Why this priority**: This is the lowest-risk, highest-expected-impact fix after profiling confirms that added function calls (increased instructions-per-request) or i-cache misses are the bottleneck. It does not change any behavior.

**Independent Test**: After adding `#[inline]`, run `cargo test --locked --all-features` to confirm no behavior regression, and run the benchmark against the same baseline to measure throughput improvement.

**Acceptance Scenarios**:

1. **Given** `#[inline]` annotations are added to the hot-path message dispatch and related functions in `controller.rs` and `message_handler.rs`, **When** the code compiles in release mode, **Then** compilation succeeds without errors or new warnings.
2. **Given** the annotations are applied, **When** `cargo test --locked --all-features` runs, **Then** all tests pass.
3. **Given** the annotations are applied, **When** the localhost benchmark runs, **Then** throughput is measured and compared against the pre-fix baseline.

---

### User Story 4 - Tune Release Profile (Priority: P2)

The workspace `Cargo.toml` has no `[profile.release]` override, relying on Cargo defaults: `codegen-units = 16` and `lto = false`. With 16 codegen units, LLVM has limited visibility across compilation units and cannot inline across crate boundaries. Setting `codegen-units = 1` gives LLVM full visibility of the entire crate for inlining decisions. Enabling `lto = true` adds link-time optimization across crate boundaries within the workspace.

**Why this priority**: Profile tuning is a complementary fix that should be applied after the targeted fixes (US2, US3). It has the broadest impact on compilation time but provides the most thorough inlining coverage. It is ordered P2 only because it is the most invasive change (affecting compile times for all crates and all developers).

**Independent Test**: After setting `codegen-units = 1` and `lto = true` in the workspace `Cargo.toml`, run `cargo test --locked --all-features` and the benchmark.

**Acceptance Scenarios**:

1. **Given** `codegen-units = 1` and `lto = true` are set in the workspace `Cargo.toml` `[profile.release]`, **When** the workspace builds in release mode, **Then** compilation succeeds without errors.
2. **Given** the release profile changes, **When** `cargo test --locked --all-features` runs, **Then** all tests pass.
3. **Given** the release profile changes, **When** the localhost benchmark runs, **Then** throughput is measured and compared against the pre-fix baseline.

---

### Edge Cases

- **Benchmark noise**: The benchmark measures real-time throughput on a local machine. Thermal throttling, scheduler variance, and other system load can cause run-to-run variance. Fix: Run each benchmark configuration 3 times and report median, not best-of-N.
- **CI compile time**: `codegen-units = 1` and `lto = true` increase release build time. Fix: CI already uses `--locked` and `--all-features`, but the CI test step uses `cargo test` (debug mode) which is unaffected by `[profile.release]`. CI build step impact should be measured.
- **Microarchitectural variance**: `perf stat` results may differ across CPU models. Fix: Report the specific CPU model and cache topology alongside profiling results. The relative change (baseline vs. HEAD) is more important than absolute numbers.
- **VIEW-03 behavioral correctness**: The VIEW-03 fix added result mask stripping to `add_unchecked()`. If the inline revert accidentally removes this behavior, view tests must catch it. Fix: The revert must preserve the result mask filtering logic even when inlined.

## Requirements *(mandatory)*

### Functional Requirements

#### US1 — Profile to Confirm Root Cause

- **FR-001**: System MUST produce a reproducible profile comparison between pre-059 (commit `983b222a7`) and post-059 (HEAD) release builds of the localhost benchmark.
- **FR-002**: Profiling MUST use `perf stat` with at minimum the events `instructions`, `cycles`, `cache-misses`, and `branch-misses`.
- **FR-003**: Profiling results MUST be documented in a format that enables comparison between baseline and HEAD (e.g., a markdown table with per-event counts per benchmark run).

#### US2 — Roll Back VIEW-03 Refactoring

- **FR-004**: The `strip_result_mask_fields()` method in `async-opcua-server/src/node_manager/view.rs` MUST be inlined back into its two call sites: `BrowseNode::add()` and `BrowseNode::add_unchecked()`.
- **FR-005**: The inlined code MUST preserve the same result mask filtering logic (nulling browse_name, display_name, node_class, reference_type_id, and type_definition based on result_mask bits).
- **FR-006**: The `add_unchecked()` function MUST apply result mask stripping (the VIEW-03 fix explicitly added this behavior — it must be preserved).

#### US3 — Add #[inline] on Hot-Path Functions

- **FR-007**: `#[inline]` annotations MUST be added to the message dispatch handler in `async-opcua-server/src/session/controller.rs` (the `process_request` method and its match arm for `RequestMessage` dispatch).
- **FR-008**: `#[inline]` annotations MUST be added to session validation functions (`validate_timed_out`, `validate_activated`) if they exist in the controller or message handler modules.
- **FR-009**: Annotations MUST NOT change any behavior or API — they are purely optimization hints to LLVM.

#### US4 — Tune Release Profile

- **FR-010**: The workspace `Cargo.toml` `[profile.release]` section MUST set `codegen-units = 1`.
- **FR-011**: The workspace `Cargo.toml` `[profile.release]` section MUST set `lto = true`.
- **FR-012**: Release profile changes MUST NOT break any `--no-default-features` build variants (e.g., `cargo check --no-default-features -p async-opcua`).

### Key Entities

- **Localhost benchmark**: The tool at `tools/opcua-localhost-bench` that measures Read/Write throughput on localhost using `SecurityPolicy::None` + `MessageSecurityMode::None` + anonymous tokens. The `run` subcommand starts a server and client in the same process.
- **VIEW-03 refactoring**: The extraction of `strip_result_mask_fields()` from inline code inside `BrowseNode::add()` into a separate method during feature 059. This changes `BrowseNode`'s method vtable and compilation-unit layout.
- **Release profile**: Cargo compilation settings for optimized builds. Defaults are `codegen-units = 16` and `lto = false`. Reducing codegen units and enabling LTO gives LLVM more optimization opportunities at the cost of compilation time.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The localhost benchmark throughput at HEAD, after all fixes applied, recovers to within 5% of the pre-059 baseline (at least ~85,500 req/sec when baseline was 90k).
- **SC-002**: Profiling confirms the regression mechanism: at least one of `cache-misses`, `branch-misses`, or `instructions` per benchmark run increases by >5% in HEAD vs. baseline before fixes, and the same metric decreases toward baseline levels after fixes.
- **SC-003**: All existing CI gates pass after each fix is applied: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings`, `cargo test --locked --all-features`.
- **SC-004**: No OPC UA compliance fix from feature 059 is reverted or degraded. All 23 spec compliance findings remain addressed.

## Assumptions

- The `perf` tool is available on the development machine. If not, the profiling step may need an alternative (e.g., `cachegrind` via `valgrind`).
- The localhost benchmark's run-to-run variance is small enough (~5%) that a 27% regression is easily distinguishable from noise.
- The VIEW-03 revert does not require changing method signatures or public API — it is a private implementation detail of `BrowseNode`.
- `#[inline]` annotations are safe on the identified hot-path functions because they do not introduce new unsafe code, deadlocks, or behavioral changes.
- The `codegen-units = 1` and `lto = true` changes are acceptable for CI and developer workflows despite increased compilation time. If CI build times become problematic, these can be scoped to the benchmark binary only via `[profile.release.package."async-opcua-localhost-bench"]`.
- The benchmark connects over `SecurityPolicy::None` + `MessageSecurityMode::None` with anonymous tokens, so cryptographic overhead is not a factor in the regression.
