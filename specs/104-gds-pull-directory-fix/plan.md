# Implementation Plan: GDS Pull Directory Singleton Correction (Run 1 rework)

**Branch**: `104-gds-pull-directory-fix` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/104-gds-pull-directory-fix/spec.md`

## Summary

Feature 103 (merged, PR #308) concluded that `CertificateDirectoryType` ships no pre-instantiated
singleton in the GDS companion NodeSet, and built ~250 lines of custom object-instantiation logic
(`directory_instance.rs`) to hand-construct a parallel "Directory" object with fabricated string
NodeIds. That conclusion was empirically wrong: the real NodeSet2.xml ships a fully-instantiated
"Directory" object (verified this session against the local schema) with real integer NodeIds for
every Mandatory (and even Optional) method, and a real `CertificateGroups`/`DefaultApplicationGroup`/
`TrustList` subtree. This feature replaces the hand-construction logic with a simple resolve-the-
real-object lookup, mirroring the existing `push_methods.rs` pattern (fixed, verified NodeIds) —
removing code, not adding it, and correcting the address space to expose one real Directory object
instead of two.

## Technical Context

**Language/Version**: Rust 2021, workspace crate `async-opcua-server`
**Primary Dependencies**: `opcua-nodes` (`AddressSpace`, `NodeId`), existing `companion::import_gds`
**Storage**: N/A (in-memory address space)
**Testing**: `cargo test` (unit tests in `directory_instance.rs`/`pull_methods/mod.rs`, integration
test `gds_pull_companion_integration.rs`)
**Target Platform**: Linux server (matches existing project CI matrix)
**Project Type**: Library (OPC UA server SDK)
**Performance Goals**: N/A — this is a correctness fix, not a performance-sensitive path
**Constraints**: Zero change to externally observable Call-service behavior for the six Mandatory
methods; zero effect on builds without `companion-gds`; zero regression to features 101/102
**Scale/Scope**: Single file rewrite (`directory_instance.rs`), NodeId-value updates in
`pull_methods/mod.rs` and its tests, doc/evidence-register updates. No new public API surface.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: This feature exists *because* of a correctness gap (a wrong
  research conclusion led to a non-conformant duplicate object). Re-verifying every cited NodeId
  independently against the real XML before hardcoding it (not trusting the investigation that
  surfaced this feature) is the plan's Phase 0 research step. PASS.
- **II. Do It Right Once**: Replaces a workaround (build-your-own object) with the correct approach
  (resolve the real one) — this is the "fix the root cause, not the symptom" case the principle
  describes. PASS.
- **III. Individual Task Discipline**: Tasks below are scoped one file/concern at a time. PASS.
- **IV. Security Is Paramount**: No new attacker-facing surface; resolution remains fail-closed
  (unchanged pattern: `AddressSpace::find` returns `None` → warn and abort wiring, never panic).
  PASS.
- **V. Leave It Better Than You Found It**: Net code deletion (removes `ObjectBuilder`/
  `MethodBuilder` scaffolding, `insert_method`/`argument`/`array_argument` helpers no longer
  needed); corrects stale documentation (research.md, TODO.md, AUDIT_TABLE) rather than leaving it
  wrong. PASS.

No violations. No Complexity Tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/104-gds-pull-directory-fix/
├── plan.md              # This file
├── research.md          # Phase 0 output — re-verified NodeIds, root-cause of the original miss
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/
├── src/gds/
│   ├── directory_instance.rs   # Rewritten: resolve real object instead of constructing one
│   ├── mod.rs                  # Unchanged structurally; calls directory_instance as before
│   └── pull_methods/
│       ├── mod.rs              # NodeId values only change (via directory_instance's output)
│       └── tests.rs            # Assertions updated where they encode old fabricated NodeIds
├── tests/
│   └── gds_pull_companion_integration.rs  # Unchanged behavior, now exercises real NodeIds
tools/cu-coverage-report/src/lib.rs         # CU 3582 evidence text updated
specs/conformance-tester/CU-COVERAGE.md    # Mirrors AUDIT_TABLE update
TODO.md                                     # Corrected Optional-method deferral reasoning
specs/103-gds-pull-fix/research.md          # Correction note on the original wrong finding
```

**Structure Decision**: No new files, no new crates. This is a targeted rewrite within feature 103's
existing module layout (`gds/directory_instance.rs`, `gds/pull_methods/`), consistent with treating
this as a correction to that feature rather than a new subsystem.

## Complexity Tracking

*No violations — table omitted.*
