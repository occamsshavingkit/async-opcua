# Research: Performance Regression Fix — Localhost Benchmark

## US1 — Profiling with `perf stat`

### Decision: Profile baseline (983b222a7) and HEAD (765434c3b) with `perf stat` on the benchmark binary

**Rationale**: The regression mechanism (i-cache pressure, LLVM de-inlining, or `.text` layout disruption) must be confirmed before applying fixes. `perf stat` provides hardware counter data (`instructions`, `cycles`, `cache-misses`, `branch-misses`) without requiring debug symbols or instrumented builds. The benchmark's `run --op read` mode exercises the server hot path for 5 seconds of measurement — sufficient to collect statistically meaningful counters.

**Methodology**:
1. Build release binary: `cargo build --release --bin async-opcua-localhost-bench`
2. Profile: `perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5`
3. Run 3x per commit, report median
4. Compare: instructions-per-request, cache-misses-per-request, branch-misses-per-request between baseline and HEAD

**Alternatives considered**:
- `cachegrind` via valgrind: Slower (10-50x overhead), but provides cache-line-level detail. Better for identifying specific hot functions but worse for end-to-end throughput comparison. Use as fallback if `perf stat` doesn't show clear differences.
- `samply` profiler: Shows flame graphs but no hardware counters. Complementary to `perf stat`, not a replacement.
- `cargo flamegraph`: Shows hot functions but no cache behavior. Not useful for confirming i-cache pressure.

### Decision: Document profiling results in `docs/perf-analysis.md`

**Rationale**: The profiling data is the evidence that validates the fix strategy. It should be preserved in the repository as documentation for future performance work.

**Alternatives considered**: Inline in the feature spec. Would clutter the spec with raw counter data that changes over time. A separate doc is cleaner.

---

## US2 — VIEW-03 Revert

### Decision: Inline `strip_result_mask_fields()` back into its two call sites

**Rationale**: The VIEW-03 fix extracted `strip_result_mask_fields()` as a separate method on `BrowseNode` to avoid duplicating the result-mask-field-clearing logic across `add()` and `add_unchecked()`. For a Browse-heavy workload, `BrowseNode` is frequently instantiated — the method extraction changes the struct's compilation-unit layout, which can trigger LLVM inlining-threshold cascade effects on other methods in the same crate.

**The revert**: Remove the `strip_result_mask_fields()` method and expand the inline code at both call sites. This is a pure refactor — no behavioral change. The compliance fix (result mask stripping in `add_unchecked()`) is preserved.

**Alternatives considered**:
- `#[inline]` on `strip_result_mask_fields()` instead: LLVM may already inline a private method that is only called from two sites. The harm is not the call overhead — it's the method's existence changing the struct's method count and layout in LLVM's analysis. Inlining via attribute doesn't fix layout disruption.
- Keep the method but add `#[cold]` to it: The method is on the Browse hot path, not a cold path. Adding `#[cold]` would make things worse by pessimizing the branch prediction for Browse operations.

---

## US3 — `#[inline]` on Hot-Path Functions

### Decision: Add `#[inline]` to `process_request()` and the `RequestMessage` dispatch match arms

**Rationale**: LLVM uses a cost model to decide whether to inline. The ~1,100 lines added by feature 059 increase the overall crate code size, which can push hot functions past the inlining threshold. `#[inline]` tells LLVM to inline regardless of the cost model heuristic.

**Target functions** (controller.rs):
- `process_request()` — the main per-request dispatch
- The `RequestMessage` match arm body (each arm dispatches to a specific handler like `read_service`, `write_service`, etc.)
- `SessionController::run()` inner loop — the message pump that calls `process_request()`

**Alternatives considered**:
- `#[inline(always)]`: More aggressive than needed. `#[inline]` is a suggestion that still allows LLVM to refuse if truly impossible (e.g., recursive functions). `#[inline(always)]` forces inlining unconditionally and can cause code bloat if misapplied. Use `#[inline]` as the default, escalating to `#[inline(always)]` only if profiling shows the hint is insufficient.
- Per-arm `#[inline]` for each `RequestMessage::Read(...)` etc.: Each arm is a thin wrapper that calls a service handler. The dispatch itself is the hot path, not individual arms. Adding `#[inline]` on the dispatch function covers all arms.

---

## US4 — Release Profile Tuning

### Decision: Add `codegen-units = 1` and `lto = true` to `[profile.release]`

**Rationale**: The workspace currently has no `[profile.release]` override, using Cargo defaults: `codegen-units = 16` and `lto = false`. With 16 codegen units, LLVM only sees one unit at a time and cannot inline across unit boundaries. `codegen-units = 1` gives LLVM visibility of the entire crate, enabling more accurate inlining decisions. `lto = true` extends this to cross-crate inlining within the workspace.

**Note**: The workspace already has an `[profile.embedded]` with `codegen-units = 1` and `lto = true`, proving these settings work for this codebase. The `release` profile just needs the same treatment.

**Compile time impact**: `codegen-units = 1` roughly doubles release build time (less parallelism). `lto = true` adds ~10-30% more. Combined, expect a ~2.5x increase in release build time. CI impact is minimal because CI tests use debug builds (`cargo test` without `--release`). Only `cargo build` in CI is affected.

**Alternatives considered**:
- Scope to just the benchmark binary: `[profile.release.package."async-opcua-localhost-bench"]` with `inherits = "release"` plus the LTO/codegen settings. This would improve the benchmark without affecting other crates' build times. However, the regression affects any binary that exercises the server hot path (not just the benchmark), so a workspace-wide fix is more appropriate.
- `lto = "fat"` instead of `lto = true`: "fat" does cross-crate LTO in a single pass but is slower. `lto = true` (equivalent to "thin") is faster and sufficient for inlining across crate boundaries.
