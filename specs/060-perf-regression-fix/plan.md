# Implementation Plan: Performance Regression Fix — Localhost Benchmark

**Branch**: `060-perf-regression-fix` | **Date**: 2026-07-05 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/060-perf-regression-fix/spec.md`

## Summary

Fix a 27% throughput regression (90k → 66k req/sec) in the localhost read/write benchmark caused by indirect compilation effects from feature 059 spec-compliance-audit-fixes. The plan applies a layered approach: profile to confirm the regression mechanism, revert VIEW-03 refactoring that disrupted LLVM inlining heuristics, add `#[inline]` annotations to hot-path request dispatch and session validation functions, and optimize the release profile for maximum inlining visibility.

## Technical Context

**Language/Version**: Rust (edition 2021, no MSRV pinned)
**Primary Dependencies**: tokio (async runtime), opcua workspace crates (async-opcua-core, async-opcua-server, async-opcua-types, async-opcua-crypto)
**Storage**: N/A (benchmark is in-memory, no persistence)
**Testing**: cargo test --locked --all-features, cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings
**Target Platform**: Linux (localhost benchmark using perf stat)
**Project Type**: library (workspace of 15+ crates) + benchmark tool
**Performance Goals**: >= 85,500 req/sec in localhost benchmark (within 5% of 90k baseline)
**Constraints**: No OPC UA compliance regression; all 23 spec compliance findings from feature 059 remain addressed; benchmark reproducible within ~5% run-to-run variance
**Scale/Scope**: Single-process localhost benchmark; four targeted code changes across view.rs, controller.rs, instance.rs, and Cargo.toml

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Status |
|-----------|-----------|--------|
| I. Correctness Over Completion | All fixes are compiler optimization hints (inline, codegen-units, lto) or inlining reverts that preserve identical behavior. Tests must pass at each step. | PASS |
| II. Do It Right Once | Profiling data (US1) must confirm the regression mechanism before fixes are applied. Targeted inlining avoids speculative annotations. | PASS |
| III. Individual Task Discipline | Each user story maps to one independently verifiable task: profile (US1), revert VIEW-03 (US2), add #[inline] (US3), tune profile (US4). | PASS |
| IV. Security Is Paramount | No changes touch decode/parse paths, cryptography, authentication, or network input. All changes are compiler optimization hints or method inlining. | PASS |
| V. Leave It Better Than You Found It | Release profile tuning benefits all workspace crates, not just the benchmark. Inline annotations serve as self-documenting hot-path markers. | PASS |

**Gate Result**: All principles pass. No violations to justify.

## Project Structure

### Documentation (this feature)

```text
specs/060-perf-regression-fix/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output — N/A (no external interfaces)
└── tasks.md             # Phase 2 output (/speckit.tasks command — NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
Cargo.toml                        # Workspace root — [profile.release] tuning target
async-opcua-server/src/
├── session/
│   ├── controller.rs             # US3: #[inline] on process_request dispatch
│   └── instance.rs               # US3: #[inline] on validate_timed_out, validate_activated
└── node_manager/
    └── view.rs                   # US2: Inline strip_result_mask_fields back into add()/add_unchecked()

tools/opcua-localhost-bench/
├── Cargo.toml                    # Benchmark dependencies
└── src/main.rs                   # Localhost read/write benchmark
```

**Structure Decision**: Single workspace with library crates under `async-opcua/` and tools under `tools/`. No structural changes needed — modifications are localized to three source files and the workspace Cargo.toml.

## Complexity Tracking

No violations to justify. All constitution principles pass without exceptions.
