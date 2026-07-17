---

description: "Task list for feature 097: Base Info Conformance Completion"
---

# Tasks: Base Info Conformance Completion

**Input**: Design documents from `/specs/097-base-info-completion/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Tasks are grouped by user story (P1→P7), each
independently implementable/testable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US7)
- Every task cites the OPC-10000 Part/§ it implements against.

## Path Conventions

New `async-opcua-server/src/base_info.rs` module; tests in new
`async-opcua/tests/integration/base_info.rs`; evidence ledger in
`tools/cu-coverage-report/src/lib.rs`.

---

## Phase 1: Setup

- [X] T001 Create `async-opcua-server/src/base_info.rs`, declare it in `lib.rs`, and create `async-opcua/tests/integration/base_info.rs`, registering it in the integration test module list (mirroring how other test files are registered).

---

## Phase 2: Foundational

No blocking cross-story prerequisites beyond T001.

---

## Phase 3: User Story 1 - OrderedListType / IOrderedObjectType (Priority: P1) 🎯 MVP

**Goal**: Instantiate an `OrderedListType` Object with ordered,
interface-conformant children (OPC-10000-5 §6.10/§6.11), closing CU 2512
and, as a byproduct, CU 3560 (Address Space Interfaces — `HasInterface`
usage).

**Independent Test**: See spec.md.

### Tests for User Story 1

- [X] T002 [US1] Add integration test in `base_info.rs` instantiating an `OrderedListType` list with 3 ordered child Objects; assert `HasOrderedComponent` browse order matches insertion order, each child's `NumberInList` is unique and matches its position, and each child has a `HasInterface` reference to `IOrderedObjectType` (OPC-10000-5 §6.10/§6.11).

### Implementation for User Story 1

- [X] T003 [US1] In `base_info.rs`, add `create_ordered_list_in_address_space(address_space, ns, name, source_node_id) -> NodeId` (the list Object, `ObjectTypeId::OrderedListType`) and `add_ordered_object(address_space, ns, list_id, name, number_in_list) -> NodeId` (a child Object with `HasOrderedComponent` from the list, `HasInterface` to `ObjectTypeId::IOrderedObjectType`, and a `NumberInList` property).
- [X] T004 [US1] Run T002; confirm it passes.

**Checkpoint**: Closes CUs 2512 and 3560.

---

## Phase 4: User Story 2 - SelectionListType (Priority: P2)

**Goal**: Instantiate a `SelectionListType` Variable (OPC-10000-5 §7.18),
closing CU 2711.

### Tests for User Story 2

- [X] T005 [P] [US2] Add integration test instantiating a `SelectionListType` Variable with 3 `Selections` entries, matching `SelectionDescriptions`, and `RestrictToList = true`; assert all three properties read back correctly (OPC-10000-5 §7.18).

### Implementation for User Story 2

- [X] T006 [US2] In `base_info.rs`, add `create_selection_list_variable(address_space, ns, name, parent_id, data_type, selections, descriptions, restrict_to_list) -> NodeId`.
- [X] T007 [US2] Run T005; confirm it passes.

**Checkpoint**: Closes CU 2711.

---

## Phase 5: User Story 3 - OptionSetType (Priority: P3)

**Goal**: Instantiate an `OptionSetType` Variable (OPC-10000-5 §7.17),
closing CU 3127.

### Tests for User Story 3

- [X] T008 [P] [US3] Add integration test instantiating an `OptionSetType` Variable with a bitmask value and `OptionSetValues`/`BitMask`; assert both arrays read back with the correct per-bit values (OPC-10000-5 §7.17).

### Implementation for User Story 3

- [X] T009 [US3] In `base_info.rs`, add `create_option_set_variable(address_space, ns, name, parent_id, value, option_set_values, bit_mask) -> NodeId`.
- [X] T010 [US3] Run T008; confirm it passes.

**Checkpoint**: Closes CU 3127.

---

## Phase 6: User Story 4 - ValueAsText (Priority: P4)

**Goal**: Attach a `ValueAsText` property to an enumerated DataVariable,
kept in sync with its Value, closing CU 2969.

### Tests for User Story 4

- [X] T011 [P] [US4] Add integration test instantiating an enumerated DataVariable with `ValueAsText` wired; write 2 different valid enum values and assert `ValueAsText` updates to the matching localized text each time (Part 3, DataVariable InstanceDeclarations).

### Implementation for User Story 4

- [X] T012 [US4] In `base_info.rs`, add `create_enum_variable_with_value_as_text(address_space, ns, name, parent_id, enum_values: &[(i64, LocalizedText)], initial_value) -> (NodeId value_id, NodeId value_as_text_id)` plus `update_enum_value(address_space, value_id, value_as_text_id, enum_values, new_value)` recomputing `ValueAsText` from the enum table.
- [X] T013 [US4] Run T011; confirm it passes.

**Checkpoint**: Closes CU 2969.

---

## Phase 7: User Story 5 - ReferenceDescriptionVariableType (Priority: P5)

**Goal**: Attach a `ReferenceDescriptionVariableType` instance via
`HasReferenceDescription` describing a real Reference (OPC-10000-23 §5),
closing CU 3996.

### Tests for User Story 5

- [X] T014 [P] [US5] Add integration test: create two nodes with a Reference between them, attach a `ReferenceDescriptionVariableType` instance via `HasReferenceDescription` from the source node, and assert its Value's `SourceNode`/`ReferenceType`/`IsForward`/`TargetNode` match the actual Reference (OPC-10000-23 §5).

### Implementation for User Story 5

- [X] T015 [US5] In `base_info.rs`, add `attach_reference_description(address_space, ns, name, described_source, reference_type, is_forward, described_target) -> NodeId`, creating a `ReferenceDescriptionVariableType` Variable with the `ReferenceDescriptionDataType` Value and a `HasReferenceDescription` reference from `described_source`.
- [X] T016 [US5] Run T014; confirm it passes.

**Checkpoint**: Closes CU 3996.

---

## Phase 8: User Story 6 - CurrencyUnit Property (Priority: P6)

**Goal**: Attach a `CurrencyUnit` property to a monetary DataVariable
(OPC-10000-5 §12.2.12.2), closing CU 5240.

### Tests for User Story 6

- [X] T017 [P] [US6] Add integration test instantiating a monetary DataVariable with a `CurrencyUnit` property set to a real ISO 4217 currency (e.g. USD: numeric_code=840, exponent=2, alphabetic_code="USD"); assert all four fields read back correctly (OPC-10000-5 §12.2.12.2).

### Implementation for User Story 6

- [X] T018 [US6] In `base_info.rs`, add `create_currency_variable(address_space, ns, name, parent_id, amount, currency: CurrencyUnitType) -> NodeId`.
- [X] T019 [US6] Run T017; confirm it passes.

**Checkpoint**: Closes CU 5240.

---

## Phase 9: User Story 7 - EstimatedReturnTime (Priority: P7)

**Goal**: Expose `Server.EstimatedReturnTime`, wired into the existing
`schedule_shutdown` mechanism (OPC-10000-5, ServerType definition),
closing CU 3198.

### Tests for User Story 7

- [X] T020 [US7] Add integration test: schedule a shutdown with a known estimated return time via the server's existing shutdown-scheduling API; assert a client reading `Server.EstimatedReturnTime` gets that value; assert it reads null before any shutdown is scheduled.

### Implementation for User Story 7

- [X] T021 [US7] In `server_status.rs`, add an `estimated_return_time: DateTime` field to `ShutdownTarget`, extend `schedule_shutdown` (or add a sibling method) to accept it, and expose it via `VariableId::Server_EstimatedReturnTime` in the attribute-read dispatch (returning null when no shutdown is scheduled).
- [X] T022 [US7] Run T020; confirm it passes.

**Checkpoint**: Closes CU 3198.

---

## Phase 10: Polish & Cross-Cutting Concerns

- [X] T023 Run the full `async-opcua/tests/integration/base_info.rs` suite plus any pre-existing suites touched (`read.rs` if `Server_EstimatedReturnTime` dispatch shares a match block); confirm zero regressions.
- [X] T024 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CUs 2512, 2711, 3127, 2969, 3996, 5240, 3198, and 3560 from `Gap`/`Partial` to `Implemented` with file:line/test evidence.
- [X] T025 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T026 Update `TODO.md`'s conformance backlog to reflect these 8 CUs closing.
- [X] T027 Run `cargo clippy --all-targets --all-features`, `cargo fmt --all`, then the full CI gate (`tools/ci-playbook.sh --ci`, launched detached).

---

## Dependencies & Execution Order

All 7 user stories are mutually independent (each touches a distinct set
of functions in the same new `base_info.rs` file, so implement serially
to avoid same-file conflicts even though logically parallel). US1 is
recommended first (MVP, closes 2 CUs).

## Implementation Strategy

1. T001 (setup) → US1 → validate → commit.
2. US2 → validate → commit.
3. US3 → validate → commit.
4. US4 → validate → commit.
5. US5 → validate → commit.
6. US6 → validate → commit.
7. US7 → validate → commit.
8. Polish → PR.

(Given the small size of each story, consider a single combined commit
per this repo's "one commit per user story" convention if the reviewer
prefers fewer, larger commits for a cluster this size — use judgment.)
