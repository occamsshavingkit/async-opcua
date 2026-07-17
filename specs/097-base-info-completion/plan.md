# Implementation Plan: Base Info Conformance Completion

**Branch**: `097-base-info-completion` | **Date**: 2026-07-16 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/097-base-info-completion/spec.md`

## Summary

Close seven required CUs (2512, 2711, 3127, 2969, 3996, 5240, 3198) plus
CU 3560 (Address Space Interfaces) as a direct byproduct of CU 2512's own
requirements. Each of the first six is a self-contained address-space
instantiation helper (a new `async-opcua-server/src/base_info.rs` module),
demonstrating the SDK can correctly expose each standard VariableType/
Property with a working example and test — matching this project's
existing precedent for "supports VariableType X" CUs. The seventh
(`EstimatedReturnTime`) extends the server's existing
`ServerStatusWrapper::schedule_shutdown` mechanism in `server_status.rs`
rather than introducing a new scheduling concept.

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`)
**Primary Dependencies**: `async-opcua-server` (new `base_info.rs`, `server_status.rs`), `async-opcua-types` (generated `ReferenceDescriptionDataType`/`ReferenceListEntryDataType`/`CurrencyUnitType`/`ObjectTypeId`/`ReferenceTypeId::HasReferenceDescription`/`HasInterface`)
**Storage**: N/A (in-memory `AddressSpace`)
**Testing**: `cargo test`, integration tests in `async-opcua/tests/integration/` (new or existing files per story)
**Target Platform**: Cross-platform Rust library (server crate)
**Project Type**: Rust workspace library — single crate area (`async-opcua-server/src/base_info.rs`, `async-opcua-server/src/server_status.rs`)
**Performance Goals**: No new performance targets; all instantiation is one-time address-space setup, not a hot path
**Constraints**: MUST NOT panic on malformed input (Constitution Principle IV) — not directly applicable here since these are server-side, application-configured instantiation helpers, not parsers of untrusted wire input; still, any attribute-read dispatch added MUST handle missing/uninitialized state gracefully (return null, not panic)
**Scale/Scope**: 6 new address-space instantiation helpers (US1-US6) + 1 existing-mechanism extension (US7), within one new ~300-400 LOC module plus a small `server_status.rs` addition

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Each user story ships with its own
  test proving the instantiated structure matches the spec's field-level
  requirements (not just "a node exists"), e.g. US1 checks `NumberInList`
  uniqueness/ordering and the actual `HasInterface` reference, not just
  that child Objects exist. PASS.
- **II. Do It Right Once**: US7 extends the *existing*
  `schedule_shutdown` mechanism rather than adding a second, parallel
  shutdown-scheduling concept. US1's `HasInterface` wiring directly
  reuses the same mechanism CU 3560 independently needs, avoiding
  duplicate future work. PASS.
- **III. Individual Task Discipline**: Enforced at `/speckit-tasks` —
  each of the 7 (8 counting 3560) CUs is its own independently-testable
  task cluster. PASS (verified at task generation).
- **IV. Security Is Paramount**: These are opt-in, application-configured
  address-space instantiation helpers, not new network-facing parsing of
  untrusted input; `EstimatedReturnTime`'s read path must handle the
  "nothing scheduled" case without panicking. PASS.
- **V. Leave It Better Than You Found It**: `tools/cu-coverage-report`'s
  `AUDIT_TABLE` updated for all 8 CUs (7 target + 1 byproduct) with
  file:line/test evidence on completion. PASS (verified at completion).

## Project Structure

### Documentation (this feature)

```text
specs/097-base-info-completion/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks, not this command)
```

### Source Code (repository root)

Single Rust workspace, no new crates. Touches:

```text
async-opcua-server/src/
├── base_info.rs          # NEW — OrderedList/SelectionList/OptionSet/ValueAsText/
│                          # ReferenceDescription/CurrencyUnit instantiation helpers (US1-US6)
├── lib.rs                # module declaration + re-export for base_info
└── server_status.rs      # EstimatedReturnTime wiring into schedule_shutdown (US7)

async-opcua/tests/integration/
└── base_info.rs           # NEW — one test per user story

tools/cu-coverage-report/src/lib.rs        # AUDIT_TABLE updates for 8 CUs
specs/conformance-tester/CU-COVERAGE.md    # Regenerated
```

**Structure Decision**: A single new small module (`base_info.rs`)
groups six unrelated-but-similarly-scoped "expose this standard
VariableType" helpers, matching how this project already groups
similarly-scoped one-off conformance instantiations (e.g. `alarms/`
groups multiple alarm-kind modules). `EstimatedReturnTime` stays in
`server_status.rs` since it's genuinely part of that existing subsystem,
not a new standalone concept.

## Complexity Tracking

*No constitution violations — this section is not applicable.*
