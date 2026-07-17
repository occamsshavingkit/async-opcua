# Implementation Plan: Data Access Conformance Completion

**Branch**: `100-data-access-completion` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/100-data-access-completion/spec.md`

## Summary

Close 8 required CUs (2361, 2426, 2831, 2988, 3323, 3324, 3325, 3326, 3327)
by adding `async-opcua-server/src/data_access.rs`, an instantiation-helper
module for the OPC-10000-8 (Data Access) DiscreteItemType family
(TwoStateDiscreteType, MultiStateDiscreteType, MultiStateValueDiscreteType)
and ArrayItemType family (YArrayItemType, XYArrayItemType, ImageItemType,
CubeItemType, NDimensionArrayItemType), matching the established pattern
from feature 097 (Base Info completion). CU 2426 (the abstract
`DiscreteItemType` base) closes as a byproduct of any concrete subtype.

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`, stable toolchain)
**Primary Dependencies**: `async-opcua-server` (address space builders), `async-opcua-types` (Range, EUInformation, AxisInformation, AxisScaleEnumeration, EnumValueType, XVType — all already generated from the standard nodeset schema)
**Storage**: N/A (in-memory address space; no persistent storage)
**Testing**: `cargo test -p async-opcua --test integration_tests -- integration::data_access::`
**Target Platform**: Cross-platform Rust library/server (Linux CI primary)
**Project Type**: Library (OPC UA server SDK) — single Cargo workspace
**Performance Goals**: N/A (conformance/capability addition, not perf-sensitive)
**Constraints**: Each Property attached must exactly match its spec-mandated DataType and ModellingRule (Mandatory); multi-dimensional array Values must set the `Variant::Array`'s own `dimensions` field (not just the node's `ArrayDimensions` attribute) for `NDimensionArrayItemType` specifically, per a real validation-model finding during implementation (see research.md)
**Scale/Scope**: 8 conformance units; one new module with 8 instantiation functions plus 2 write-time-update helpers; one new integration test file with 8 tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Every VariableType's mandatory
  Properties grounded against the local OPC-10000-8 v1.05.07 PDF before
  implementing; the two CUs (2474/2776) that could NOT be grounded against
  either the local PDF or the public reference site are explicitly
  deferred rather than guessed at (see spec.md Assumptions). PASS.
- **II. Do It Right Once**: Shared `ArrayItemBaseProperties` struct + a
  private `attach_array_item_base_properties` helper avoid duplicating the
  four-Property base set across five near-identical functions. PASS.
- **III. Individual Task Discipline**: Two user stories (discrete-state
  types, array-shaped types), each independently testable. PASS.
- **IV. Security Is Paramount**: No new attack surface — these are
  address-space instantiation helpers used at server-build time, not
  request-handling code paths. PASS.
- **V. Leave It Better Than You Found It**: Updates `AUDIT_TABLE` evidence
  and `CU-COVERAGE.md`, matching established project convention. PASS.

No violations; Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/100-data-access-completion/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/data_access.rs                        # NEW: instantiation helpers for all 8 VariableTypes
async-opcua-server/src/lib.rs                                 # + pub mod data_access;
async-opcua/tests/integration/data_access.rs                   # NEW: 8 integration tests
async-opcua/tests/integration/mod.rs                            # + mod data_access;
tools/cu-coverage-report/src/lib.rs                              # AUDIT_TABLE evidence for all 8 CUs
specs/conformance-tester/CU-COVERAGE.md                          # regenerated
TODO.md                                                            # "DataAccess variable-type instances" backlog entry closed (except 2474/2776, documented deferred)
```

**Structure Decision**: New `data_access.rs` module mirroring the existing
`base_info.rs` module exactly (same builder-function style, same
"SDK-capability helper" framing), plus a new sibling integration test
file, matching the established pattern from feature 097.

## Complexity Tracking

*No violations — section not needed.*
