---

description: "Task list for feature 098: Address Space Write Enforcement Completion"
---

# Tasks: Address Space Write Enforcement Completion

**Input**: Design documents from `/specs/098-write-enforcement/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Tasks are grouped by user story (P1→P3), each
independently implementable/testable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US3)
- Every task cites the OPC-10000 Part/§ it implements against.

## Path Conventions

`async-opcua-server/src/address_space/write_validation.rs` (enforcement
logic); tests in `async-opcua/tests/integration/write.rs` and
`async-opcua-nodes/src/variable.rs`'s `access_level_ex_tests` module;
evidence ledger in `tools/cu-coverage-report/src/lib.rs`.

---

## Phase 1: Setup

No setup tasks — this feature modifies existing files only, no new modules.

---

## Phase 2: Foundational

No blocking cross-story prerequisites.

---

## Phase 3: User Story 1 - WriteFullArrayOnly enforcement (Priority: P1) 🎯 MVP

**Goal**: Reject client IndexRange Writes to Variables that declare
`WriteFullArrayOnly` on `AccessLevelEx`, per OPC-10000-3 §8.58 Table 42 and
OPC-10000-4 §5.11.4 Table 53. Closes CU 2820.

**Independent Test**: See spec.md User Story 1.

### Tests for User Story 1

- [X] T001 [US1] Add integration test in `async-opcua/tests/integration/write.rs` creating a Variable with an array value and `WriteFullArrayOnly` set on `AccessLevelEx`; assert an IndexRange Write to it returns `BadWriteNotSupported` and the stored value is unchanged; assert a full-array (no IndexRange) Write to the same Variable still succeeds; assert an equivalent Variable WITHOUT the flag still accepts IndexRange Writes (regression guard for CU 3147) (OPC-10000-3 §8.58 Table 42; OPC-10000-4 §5.11.4 Table 53).

### Implementation for User Story 1

- [X] T002 [US1] In `async-opcua-server/src/address_space/write_validation.rs`'s `validate_node_write_inner`, in the `NodeType::Variable(var)` arm, before the existing enum/data-type/EURange validation calls: when `has_index_range` is true and `var.access_level_ex() & AccessLevelExType::WriteFullArrayOnly.bits() as u32 != 0`, return `Err(StatusCode::BadWriteNotSupported)`. Import `AccessLevelExType` from `opcua_types`.
- [X] T003 [US1] Run T001; confirm it passes.

**Checkpoint**: Closes CU 2820.

---

## Phase 4: User Story 2 - StatusCode & Timestamp write round-trip test (Priority: P2)

**Goal**: Prove a client Write carrying a non-Good StatusCode and explicit
Source/Server Timestamps is stored and returned correctly. Closes CU 2936.

### Tests for User Story 2

- [X] T004 [P] [US2] Add integration test in `write.rs`: Write a scalar Variable's Value with `status: Some(StatusCode::Uncertain)` and distinct, explicit `source_timestamp`/`server_timestamp`; Read it back with `TimestampsToReturn::Both`; assert value, status, and both timestamps all match what was written (OPC-10000-4 §5.11.4 Table 53, `value` row).

### Implementation for User Story 2

- [X] T005 [US2] Run T004; confirm it passes. **Correction**: the research.md prediction of "no code change needed" was wrong. T004 found TWO real bugs in `write_node_value` (`async-opcua-server/src/address_space/utils.rs`): (1) `server_timestamp` was hardcoded to `DateTime::now()` instead of using the client-supplied value; (2) discovered as a byproduct while auditing the same function, the ByteString→byte-array coercion that `Variable::set_value` has was missing from the Write-service path entirely (Part 4 §5.11.4 Table 53: "A Server shall accept a ByteString if an array of Byte is expected"). Both fixed in `write_node_value`.

**Checkpoint**: Closes CU 2936.

---

## Phase 5: User Story 3 - NonVolatile/Constant round-trip test (Priority: P3)

**Goal**: Prove the `NonVolatile` and `Constant` `AccessLevelEx` bits can be
set and read back correctly. Closes CU 4237.

### Tests for User Story 3

- [X] T006 [P] [US3] Add a test (unit test in `async-opcua-nodes/src/variable.rs`'s `access_level_ex_tests` module, OR integration test in `write.rs`/`read.rs` using `VariableBuilder`) that sets `AccessLevelExType::NonVolatile | AccessLevelExType::Constant` on a Variable and confirms a Read of `AttributeId::AccessLevelEx` returns both bits set (OPC-10000-3 §8.58 Table 42, bits 12/13).

### Implementation for User Story 3

- [X] T007 [US3] Run T006; confirm it passes. No production code change expected.

**Checkpoint**: Closes CU 4237.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T008 Run the full `write.rs` integration suite plus any adjacent suites touched (`read.rs` if the CU 4237 test lands there); confirm zero regressions.
- [X] T009 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CUs 2820, 2936, and 4237 from `Partial` to `Implemented` with file:line/test evidence.
- [X] T010 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T011 Update `TODO.md`: close out the "Attribute Write remaining gaps" backlog entry.
- [ ] T012 Run `cargo clippy --all-targets --all-features`, `cargo fmt --all`, then the full CI gate (`tools/ci-playbook.sh --ci`, launched detached).

---

## Dependencies & Execution Order

US1 is the MVP (the one real behavioral gap). US2 and US3 are independent
test-only closures with no code dependency on US1 or each other, but all
three touch the shared `write.rs` file, so implement serially to avoid
same-file conflicts even though logically parallel.

## Implementation Strategy

1. US1 → validate → commit.
2. US2 → validate → commit.
3. US3 → validate → commit.
4. Polish → PR.

(Given the small size of this cluster, a single combined commit for the
whole feature is also acceptable per this repo's "one commit per user
story" convention allowing judgment calls for small clusters — matches how
feature 097 was committed.)
