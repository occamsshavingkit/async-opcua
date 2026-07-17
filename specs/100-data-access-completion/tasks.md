---

description: "Task list for feature 100: Data Access Conformance Completion"
---

# Tasks: Data Access Conformance Completion

**Input**: Design documents from `/specs/100-data-access-completion/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Tasks are grouped by user story (P1→P2), each
independently implementable/testable.

## Format: `[ID] [P?] [Story] Description`

- **[Story]**: US1 (discrete-state types), US2 (array-shaped types)
- Every task cites the OPC-10000-8 § it implements against.

## Path Conventions

New `async-opcua-server/src/data_access.rs` module; tests in new
`async-opcua/tests/integration/data_access.rs`; evidence ledger in
`tools/cu-coverage-report/src/lib.rs`.

---

## Phase 1: Setup

- [X] T001 Create `async-opcua-server/src/data_access.rs`, declare it in `lib.rs`, and create `async-opcua/tests/integration/data_access.rs`, registering it in `mod.rs`.

---

## Phase 2: Foundational

No blocking cross-story prerequisites beyond T001.

---

## Phase 3: User Story 1 - Discrete-state DataAccess Variables (Priority: P1) 🎯 MVP

**Goal**: TwoStateDiscreteType, MultiStateDiscreteType, MultiStateValueDiscreteType
instances with all spec-mandated Properties (OPC-10000-8 §5.3.3.2-5.3.3.4).
Closes CUs 2361, 2831, 2988, and 2426 (abstract-base byproduct).

### Implementation for User Story 1

- [X] T002 [US1] In `data_access.rs`, add `create_two_state_discrete_variable` (TrueState/FalseState mandatory Properties).
- [X] T003 [US1] In `data_access.rs`, add `create_multi_state_discrete_variable` (EnumStrings mandatory Property).
- [X] T004 [US1] In `data_access.rs`, add `create_multi_state_value_discrete_variable` + `MultiStateValueDiscreteHandle` + `update_multi_state_value_discrete` (EnumValues + ValueAsText mandatory Properties, non-contiguous numeric codes).

### Tests for User Story 1

- [X] T005 [P] [US1] Add integration test instantiating a TwoStateDiscreteType Variable; assert Value and both state Properties read back correctly (OPC-10000-8 §5.3.3.2).
- [X] T006 [P] [US1] Add integration test instantiating a MultiStateDiscreteType Variable; assert Value and EnumStrings read back correctly (§5.3.3.3).
- [X] T007 [P] [US1] Add integration test instantiating a MultiStateValueDiscreteType Variable with non-contiguous EnumValues; assert Value/EnumValues/ValueAsText read back correctly; write a matching and a non-matching value and assert ValueAsText updates accordingly (§5.3.3.4).
- [X] T008 [US1] Run T005-T007; confirm all pass.

**Checkpoint**: Closes CUs 2361, 2426, 2831, 2988.

---

## Phase 4: User Story 2 - Array-shaped DataAccess Variables (Priority: P2)

**Goal**: YArrayItemType, XYArrayItemType, ImageItemType, CubeItemType,
NDimensionArrayItemType instances with all spec-mandated Properties
(OPC-10000-8 §5.3.4.1-5.3.4.6). Closes CUs 3323-3327.

### Implementation for User Story 2

- [X] T009 [US2] In `data_access.rs`, add `ArrayItemBaseProperties` struct + private `attach_array_item_base_properties` helper (EURange/EngineeringUnits/Title/AxisScaleType shared base, §5.3.4.1).
- [X] T010 [US2] In `data_access.rs`, add `create_y_array_item_variable` (+ XAxisDefinition, §5.3.4.2).
- [X] T011 [US2] In `data_access.rs`, add `create_xy_array_item_variable` (XVType-typed, + XAxisDefinition, §5.3.4.3).
- [X] T012 [US2] In `data_access.rs`, add `create_image_item_variable` (2-D, + X/Y AxisDefinition, §5.3.4.4).
- [X] T013 [US2] In `data_access.rs`, add `create_cube_item_variable` (3-D, + X/Y/Z AxisDefinition, §5.3.4.5).
- [X] T014 [US2] In `data_access.rs`, add `create_nd_dimension_array_item_variable` (N-D, + one AxisDefinition per dimension, §5.3.4.6).

### Tests for User Story 2

- [X] T015 [P] [US2] Add integration test for YArrayItemType; assert Value + base Properties + XAxisDefinition read back correctly.
- [X] T016 [P] [US2] Add integration test for XYArrayItemType; assert Value (XVType array) + base Properties + XAxisDefinition read back correctly.
- [X] T017 [P] [US2] Add integration test for ImageItemType; assert Value + ArrayDimensions (columns, rows) + X/Y AxisDefinition read back correctly.
- [X] T018 [P] [US2] Add integration test for CubeItemType; assert Value + ArrayDimensions + X/Y/Z AxisDefinition read back correctly.
- [X] T019 [P] [US2] Add integration test for NDimensionArrayItemType; assert Value + ArrayDimensions + one AxisDefinition per dimension read back correctly.
- [X] T020 [US2] Run T015-T019; confirm all pass.

**Checkpoint**: Closes CUs 3323, 3324, 3325, 3326, 3327.

---

## Phase 5: Polish & Cross-Cutting Concerns

- [X] T021 Run the full `async-opcua/tests/integration/data_access.rs` suite plus the full integration suite and `async-opcua-server` lib suite; confirm zero regressions.
- [X] T022 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CUs 2361, 2426, 2831, 2988, 3323, 3324, 3325, 3326, 3327 from `Gap` to `Implemented` with file:line/test evidence.
- [X] T023 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T024 Update `TODO.md`'s "DataAccess variable-type instances" backlog entry to reflect these 9 CUs closing, documenting CUs 2474/2776 as an explicit deferral with reasoning.
- [X] T025 Run `cargo clippy --all-targets --all-features`, `cargo fmt --all`, then the full CI gate (`tools/ci-playbook.sh --ci`, launched detached).

---

## Dependencies & Execution Order

US1 and US2 are independent (different VariableType families, no shared
runtime state), but both touch the same new `data_access.rs` file, so
implement serially. US1 first (simpler, more commonly used types).

## Implementation Strategy

1. T001 (setup) → US1 → validate → commit.
2. US2 → validate → commit.
3. Polish → PR.
