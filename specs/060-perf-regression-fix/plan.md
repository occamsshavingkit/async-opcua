# Implementation Plan: Performance Regression Fix — Localhost Benchmark

**Branch**: `060-perf-regression-fix` | **Date**: 2026-07-05 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/060-perf-regression-fix/spec.md`

## Summary

Fix the 27% throughput regression (90k → 66k req/sec) in the localhost Read/Write benchmark. The regression is caused by indirect compilation effects from the ~1,100 lines added in feature 059 (spec-compliance-audit-fixes). The hot-path code (`process_request` in `controller.rs`) is byte-for-byte identical between base and HEAD — the fix is about LLVM optimization behavior, not algorithm changes.

## Technical Context

**Language/Version**: Rust 1.75+ (workspace, edition 2021)
**Primary Dependencies**: tokio, async-opcua-server (the affected crate)
**Testing**: `tools/opcua-localhost-bench`, `cargo test --locked --all-features`
**Target Platform**: Linux (perf required for profiling)
**Performance Goals**: Recover throughput to within 5% of pre-059 baseline (~85.5k req/sec when baseline is 90k)
**Constraints**: No OPC UA compliance fix reverted; no behavioral changes to the server

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Notes |
|-----------|-----------|-------|
| I. Correctness Over Completion | PASS | Profiling first confirms mechanism before applying fixes; each fix is independently measurable |
| II. Do It Right Once | PASS | `#[inline]` annotations target specific hot-path functions; release profile changes apply to whole workspace uniformly |
| III. Individual Task Discipline | PASS | Four independent USs, each measurable. Profiling (US1) gates the targeted fixes (US2, US3) |
| IV. Security Is Paramount | PASS | No cryptographic or security-policy code changes; profiling collects only hardware counter data (no sensitive content) |
| V. Leave It Better Than You Found It | PASS | Release profile tuning benefits all workspace crates; inline annotations prevent future regressions |

**Gate: ALL PASS** — no violations.

## Project Structure

### Documentation (this feature)

```text
specs/060-perf-regression-fix/
├── spec.md            # Feature specification
├── plan.md            # This file
├── research.md        # Phase 0 output
├── data-model.md      # Phase 1 output (minimal — no new entities)
├── quickstart.md      # Phase 1 output
├── contracts/         # Phase 1 output
│   └── profiling.md   # Profiling methodology contract
└── tasks.md           # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
# US1 — Profiling (no code changes)
# Output: docs/perf-analysis-{baseline,head}.md

# US2 — VIEW-03 Revert
async-opcua-server/src/node_manager/view.rs          # Inline strip_result_mask_fields back into add() and add_unchecked()

# US3 — #[inline] Annotations
async-opcua-server/src/session/controller.rs         # Add #[inline] to process_request and hot-path dispatch

# US4 — Release Profile Tuning
Cargo.toml                                           # Add [profile.release] with codegen-units = 1, lto = true

# All: Verification
tools/opcua-localhost-bench/                         # Run benchmark after each fix
```

**Structure Decision**: All changes are isolated to 3 files + 1 Cargo.toml section. No new modules or crates. The benchmark is re-run after each fix to measure incremental improvement.

## Complexity Tracking

*No violations — this section intentionally empty.*
