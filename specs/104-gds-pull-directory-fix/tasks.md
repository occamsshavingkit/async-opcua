---

description: "Task list for feature 104: GDS Pull Directory Singleton Correction (Run 1 rework)"
---

# Tasks: GDS Pull Directory Singleton Correction (Run 1 rework)

**Input**: Design documents from `/specs/104-gds-pull-directory-fix/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Single user story (P1) — this is a targeted correction, not a
multi-story feature.

## Path Conventions

Rewritten `async-opcua-server/src/gds/directory_instance.rs`. No other source
file is expected to need changes (see T003's verification step) — a real
finding this feature confirms before assuming otherwise, per Individual Task
Discipline (verify, don't assume).

---

## Phase 1: Setup

- [X] T001 Confirmed `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` is present locally (already fetched by feature 103).

---

## Phase 2: Foundational

- [X] T002 Re-verified (fresh `grep`/`awk`) every NodeId cited in research.md against the local NodeSet2.xml — all confirmed exactly as recorded: Directory object `ns=1;i=141` (`HasTypeDefinition -> i=63` CertificateDirectoryType, `Organizes` inverse from core `i=85` ObjectsFolder); six Mandatory methods `i=157/154/163/508/204/225` with `MethodDeclarationId`s `i=79/76/85/369/197/222` matching feature 103's type-level findings exactly; `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree `i=614/615/616` with `HasTypeDefinition`s `i=13813/12555/12522` (bare `i=`, confirming core namespace 0). Zero discrepancies found.

**Checkpoint**: All NodeIds this feature will hardcode are independently re-verified.

---

## Phase 3: User Story 1 - Server dispatches against the real Directory object (Priority: P1) 🎯 MVP

**Goal**: Replace the hand-built "Directory" object in `directory_instance.rs`
with a resolution of the real, already-imported singleton (OPC-10000-12
§7.9.2 `CertificateDirectoryType`).

**Independent Test**: See spec.md's Acceptance Scenarios; quickstart.md steps
1-3 give the concrete commands.

### Implementation for User Story 1

- [X] T003 [US1] Confirmed: `pull_methods/mod.rs`, `pull_methods/tests.rs`, and `gds_pull_companion_integration.rs` all consume `DirectoryInstanceNodeIds`' fields generically via `self.directory.<field>` / accessor methods, with zero hardcoded assumptions about identifier shape. Confirmed empirically too — after T004-T006's rewrite, all three files needed zero edits and the crate compiled/tested clean immediately.
- [X] T004 [US1] Rewrote `directory_instance.rs`: kept `resolve_gds_namespace` and the `CERTIFICATE_DIRECTORY_TYPE_ID`/type-existence check unchanged. Replaced all `ObjectBuilder`/`MethodBuilder` construction with direct `NodeId::new(gds_ns, <verified integer>)` for the Directory object (`141`) and six Mandatory methods (`157/154/163/508/204/225`, §7.9.3/§7.9.4/§7.9.5/§7.9.7/§7.9.9/§7.9.10) plus `DefaultApplicationGroup`/`TrustList` (`615/616`, §7.8), each verified present via `AddressSpace::find` in a loop (fail closed: `warn!` + `None`, never panic).
- [X] T005 [US1] Deleted `insert_method`, `argument`, `array_argument` — no longer called.
- [X] T006 [US1] Removed unused imports (`Argument`, `DataTypeId`, `LocalizedText`, `ObjectTypeId`, `MethodBuilder`, `ObjectBuilder`). `cargo build -p async-opcua-server --no-default-features --features companion-gds,gds` is zero-warning.

### Tests for User Story 1

- [X] T007 [P] [US1] Renamed and rewrote `instantiates_a_real_directory_object_when_companion_xml_is_present` -> `resolves_the_real_directory_object_when_companion_xml_is_present`, asserting each field equals its real integer NodeId (not just "exists"), plus the original existence checks.
- [X] T007a [P] [US1] Realized as `does_not_construct_a_duplicate_directory_object`: since no generic "count all nodes matching a browse name" API exists on `AddressSpace` (and building one would be scope creep), the concrete, direct regression guard is asserting `NodeId::new(gds_ns, "Directory")` (the exact old fabricated identifier) no longer resolves to anything, while `NodeId::new(gds_ns, 141)` (the real one) does. Closes SC-001 in spirit and precisely targets the exact bug class being fixed.
- [X] T008 [US1] `cargo test -p async-opcua-server --no-default-features --features companion-gds,gds --lib gds::` — 43 tests pass (42 original + T007a's new test; one renamed, not added, hence 43 not 44).
- [X] T009 [US1] `cargo test -p async-opcua-server --features companion-gds,method-call,generated-address-space --test gds_pull_companion_integration -- --nocapture` — passes, dispatching against the real `(ns=gds_ns;i=141, ns=gds_ns;i=154)` pair.

**Checkpoint**: Exactly one real "Directory" object exists after import (enforced by T007a, not just assumed); all Pull-model dispatch continues to work against it.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [X] T010 `async-opcua-server --all-features` full suite: 0 failures. `--no-default-features --features gds` (companion-gds disabled): zero warnings, unaffected.
- [X] T011 Updated `TODO.md`: corrected the CU 3582 Optional-methods deferral reasoning and added a one-line note about `RegisterApplication`/etc.'s now-known real NodeIds.
- [X] T012 Added a correction note to `specs/103-gds-pull-fix/research.md` pointing at feature 104 and explaining the grep-methodology root cause.
- [X] T013 [P] Updated `tools/cu-coverage-report/src/lib.rs`'s CU 3582 entry with the corrected design and reasoning.
- [X] T014 [P] Mirrored into `specs/conformance-tester/CU-COVERAGE.md`'s CU 3582 row (only one occurrence existed, unlike CU 2231's three copies).
- [X] T015 `cargo clippy --all-targets --all-features` and `cargo fmt --all` (workspace-wide) — clean.
- [X] T016 Ran the full local CI gate twice (first run was killed externally mid-way with no real failure; clean relaunch completed green) — the only FAIL was the expected/spurious `verify-codegen: check clean` (uncommitted working tree, zero actual generated-code drift).

---

## Dependencies & Execution Order

Phase 2 (NodeId re-verification) blocks Phase 3 — nothing should be hardcoded
before it's independently re-confirmed. Phase 3's T003 (verify no other file
needs changes) should run before T004 so the actual diff surface is known
up front rather than discovered piecemeal. T007a depends on T004 (needs the
corrected resolution logic to count against). Polish (T010-T016) depends on
Phase 3 being complete and green.

## Implementation Strategy

1. T001-T002 (setup + re-verification) → confirms exactly what to hardcode.
2. T003 (scope confirmation) → T004-T006 (the actual rewrite) → validate compiles.
3. T007-T009 (tests, incl. T007a's new duplicate-object regression guard) → validate green.
4. T010-T016 (polish, docs, CI gate) → PR.
