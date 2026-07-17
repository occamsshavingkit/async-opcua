# Implementation Plan: Address Space Write Enforcement Completion

**Branch**: `098-write-enforcement` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/098-write-enforcement/spec.md`

## Summary

Close the final 3 required CUs in the Attribute Write / Address Space
conformance cluster (CU 2820 WriteFullArrayOnly, CU 2936 StatusCode &
Timestamp write, CU 4237 NonVolatile/Constant) for the Micro/Embedded/
Standard 2025 server profiles. One real enforcement gap (2820: the
`WriteFullArrayOnly` bit on `AccessLevelEx` is stored/read but never
consulted on the Write path) plus two test-only evidence closures (2936,
4237: the underlying storage already round-trips correctly, but no test
proves it for these specific combinations).

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`, stable toolchain)
**Primary Dependencies**: `async-opcua-server` (write validation, node manager), `async-opcua-nodes` (`Variable`/`AccessLevelExType`), `async-opcua-types` (status codes, `NumericRange`), `tokio` (async test runtime)
**Storage**: N/A (in-memory address space; no persistent storage involved)
**Testing**: `cargo test` — targeted unit test in `async-opcua-nodes` (or `async-opcua-server` write_validation module) for CU 2820's rejection path, plus `async-opcua/tests/integration/write.rs` (`cargo test -p async-opcua --test integration_tests`) for CU 2936/4237
**Target Platform**: Cross-platform Rust library/server (Linux CI primary)
**Project Type**: Library (OPC UA server SDK) — single Cargo workspace
**Performance Goals**: N/A (correctness/conformance fix, not a perf-sensitive path)
**Constraints**: Must not change behavior for Variables that do NOT set `WriteFullArrayOnly`; must not affect server-internal (non-Write-service) value updates
**Scale/Scope**: 3 conformance units; one small enforcement check in one existing validation function, two new integration/unit tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Each CU is grounded against the local
  OPC-10000 Part 3 (§8.58, Table 42) and Part 4 (§5.11.4, Table 53) spec
  PDFs before implementation (see spec.md Assumptions and research.md).
  PASS.
- **II. Do It Right Once**: The enforcement point (`validate_node_write_inner`
  in `write_validation.rs`) is the single call site used by both node
  manager implementations' Write dispatch — no duplicated logic needed.
  PASS.
- **III. Individual Task Discipline**: Each CU maps to its own user story /
  task group with an independent test. PASS.
- **IV. Security Is Paramount**: This closes a real data-integrity gap (a
  server declaring "no partial writes" today has that declaration silently
  ignored) — this is a hardening change, not a new attack surface. PASS.
- **V. Leave It Better Than You Found It**: Updates `AUDIT_TABLE` evidence
  and `CU-COVERAGE.md` alongside the code, per established project
  convention. PASS.

No violations; Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/098-write-enforcement/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/address_space/write_validation.rs   # CU 2820: enforcement check in validate_node_write_inner
async-opcua/tests/integration/write.rs                     # CU 2820 + CU 2936: integration tests
async-opcua-nodes/src/variable.rs                           # CU 4237: existing access_level_ex_tests module gains a targeted test (or covered via write.rs/read.rs integration test using VariableBuilder)
tools/cu-coverage-report/src/lib.rs                          # AUDIT_TABLE evidence updates for all 3 CUs
specs/conformance-tester/CU-COVERAGE.md                      # regenerated
TODO.md                                                       # "Attribute Write remaining gaps" entry closed out
```

**Structure Decision**: No new modules. This is a small, targeted change
within the existing single-crate-workspace layout: one enforcement check
added to the existing shared write-validation function, plus tests added
to the existing `write.rs` integration suite (and/or the existing
`access_level_ex_tests` unit module in `async-opcua-nodes`), matching how
this server's conformance backlog has been closed in prior features
(095-097).

## Complexity Tracking

*No violations — section not needed.*
