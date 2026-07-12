# Tasks: Gauntlet Error-Handling Fixes

**Feature**: 073-gauntlet-error-handling
**Branch**: `072-gauntlet-error-handling`
**Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

> **IMPORTANT — Spec Reading Protocol**: Each task below that modifies OPC UA service behavior references the exact OPC UA specification section and table that defines the expected operation-level result codes. **Before writing any code for a task, run `opc-ua-reference_search_text` with the cited docNumber and section to read the full normative text, or fetch the reference URL directly.** Do not implement based on the task summary alone.

## Phase 1: Setup

- [ ] T001 Verify build baseline — `cargo build --workspace` and `cargo test -p async-opcua-server` pass on branch 072-gauntlet-error-handling

- [ ] T002 Read and annotate the current control flow in these files against the spec sections cited in later tasks:
  - `async-opcua-server/src/session/services/node_management.rs` (dispatches to NodeManager for all four NodeManagement operations)
  - `async-opcua-server/src/session/services/query.rs` (QueryFirst handler)
  - `async-opcua-server/src/session/services/attribute.rs` (HistoryUpdate handler, `history_update()` function ~L286)
  - `async-opcua-server/src/subscriptions/actor.rs` (SetTriggering command handler)

## Phase 2: Foundational — AddressSpace Access for Validation

- [ ] T003 [P] Add `node_exists(&NodeId) -> bool` helper in `async-opcua-server/src/session/services/node_management.rs` using `context.address_space()` read access

- [ ] T004 [P] Add `reference_type_exists(&NodeId) -> bool` helper in `async-opcua-server/src/session/services/node_management.rs`

- [ ] T005 [P] Add `reference_exists(&NodeId, &NodeId, &NodeId) -> bool` helper (source, target, reference type) for duplicate detection in `async-opcua-server/src/session/services/node_management.rs`

## Phase 3: User Story 1 — AddNodes Validation (Priority: P1)

**Goal**: AddNodes returns operation-level status codes from OPC UA Part 4 §5.8.2.4 Table 24 instead of `BadServiceUnsupported`.

**Independent Test**: Send AddNodes with a non-existent `parentNodeId` and verify per-operation result is `BadParentNodeIdInvalid`.

- [ ] T006 [US1] Implement AddNodes parent/NodeId validation in `async-opcua-server/src/session/services/node_management.rs`. Before dispatching to node managers, for each `AddNodesItem`: check `parentNodeId` existence → `BadParentNodeIdInvalid`; check `requestedNewNodeId` collision → `BadNodeIdExists`. Validation order follows OPC-10000-4 §5.8.2.4 Table 24 priority. **Before implementing: read OPC-10000-4 §5.8.2.4** ([reference.opcfoundation.org/specs/OPC-10000-4/5.8.2.4](https://reference.opcfoundation.org/specs/OPC-10000-4/5.8.2.4.md)).

- [ ] T007 [P] [US1] Implement AddNodes reference/type validation in `async-opcua-server/src/session/services/node_management.rs`. For each `AddNodesItem`: check `referenceTypeId` is a valid hierarchical ReferenceType → `BadReferenceTypeIdInvalid`; check reference is allowed for parent-child relationship → `BadReferenceNotAllowed`; check `typeDefinition` NodeId exists and matches the requested `nodeClass` → `BadTypeDefinitionInvalid`; check `nodeClass` compatibility → `BadNodeClassInvalid`. **Before implementing: read OPC-10000-4 §5.8.2.4**.

- [ ] T008 [P] [US1] Add integration test `add_nodes_invalid_parent` in `async-opcua/tests/integration/node_management.rs` — sends AddNodes with non-existent `parentNodeId`, verifies `BadParentNodeIdInvalid` in per-operation result

- [ ] T009 [P] [US1] Add integration test `add_nodes_duplicate_id` in `async-opcua/tests/integration/node_management.rs` — sends AddNodes with `requestedNewNodeId` that already exists in address space, verifies `BadNodeIdExists`

## Phase 4: User Story 1 — AddReferences Validation (Priority: P1)

**Goal**: AddReferences returns operation-level status codes from OPC UA Part 4 §5.8.3.4 Table 27.

**Independent Test**: Send AddReferences with non-existent `sourceNodeId` and verify per-operation result is `BadSourceNodeIdInvalid`.

- [ ] T010 [US1] Implement AddReferences existence validation in `async-opcua-server/src/session/services/node_management.rs`. For each `AddReferencesItem`: check `sourceNodeId` exists → `BadSourceNodeIdInvalid`; check `targetNodeId` exists (unless `targetServerUri` is set for remote) → `BadTargetNodeIdInvalid`. **Before implementing: read OPC-10000-4 §5.8.3.4** ([reference.opcfoundation.org/specs/OPC-10000-4/5.8.3.4](https://reference.opcfoundation.org/specs/OPC-10000-4/5.8.3.4.md)).

- [ ] T011 [P] [US1] Implement AddReferences relationship validation in `async-opcua-server/src/session/services/node_management.rs`. For each `AddReferencesItem`: check `referenceTypeId` is valid → `BadReferenceTypeIdInvalid`; check no duplicate reference exists between source and target → `BadDuplicateReferenceNotAllowed`; check `sourceNodeId != targetNodeId` for self-reference → `BadInvalidSelfReference`. **Before implementing: read OPC-10000-4 §5.8.3.4**.

- [ ] T012 [P] [US1] Add integration test `add_references_bad_source` in `async-opcua/tests/integration/node_management.rs` — sends AddReferences with non-existent `sourceNodeId`, verifies `BadSourceNodeIdInvalid`

- [ ] T013 [P] [US1] Add integration test `add_references_self_ref` in `async-opcua/tests/integration/node_management.rs` — sends AddReferences with `sourceNodeId == targetNodeId`, verifies `BadInvalidSelfReference`

## Phase 5: User Story 1 — DeleteNodes Validation (Priority: P1)

**Goal**: DeleteNodes returns operation-level status codes from OPC UA Part 4 §5.8.4.4 Table 30.

**Independent Test**: Send DeleteNodes with non-existent NodeId and verify per-operation result is `BadNodeIdUnknown`.

- [ ] T014 [US1] Implement DeleteNodes input validation in `async-opcua-server/src/session/services/node_management.rs`. For each `DeleteNodesItem`: check `nodeId` is syntactically valid → `BadNodeIdInvalid`; check `nodeId` exists in the address space → `BadNodeIdUnknown`. **Before implementing: read OPC-10000-4 §5.8.4.4** ([reference.opcfoundation.org/specs/OPC-10000-4/5.8.4.4](https://reference.opcfoundation.org/specs/OPC-10000-4/5.8.4.4.md)).

- [ ] T015 [P] [US1] Add integration test `delete_nodes_unknown_id` in `async-opcua/tests/integration/node_management.rs` — sends DeleteNodes with non-existent NodeId, verifies `BadNodeIdUnknown`

## Phase 6: User Story 1 — DeleteReferences Validation (Priority: P1)

**Goal**: DeleteReferences returns operation-level status codes from OPC UA Part 4 §5.8.5.3 Table 33.

**Independent Test**: Send DeleteReferences with non-existent `sourceNodeId` and verify result is `BadSourceNodeIdInvalid`.

- [ ] T016 [US1] Implement DeleteReferences input validation in `async-opcua-server/src/session/services/node_management.rs`. For each `DeleteReferencesItem`: check `sourceNodeId` exists → `BadSourceNodeIdInvalid`; check `targetNodeId` exists → `BadTargetNodeIdInvalid`. **Before implementing: read OPC-10000-4 §5.8.5.3** ([reference.opcfoundation.org/specs/OPC-10000-4/5.8.5.3](https://reference.opcfoundation.org/specs/OPC-10000-4/5.8.5.3.md)).

- [ ] T017 [P] [US1] Add integration test `delete_references_bad_source` in `async-opcua/tests/integration/node_management.rs` — sends DeleteReferences with non-existent `sourceNodeId`, verifies `BadSourceNodeIdInvalid`

## Phase 7: User Story 2 — SetTriggering Status Code (Priority: P2)

**Goal**: SetTriggering returns `BadMonitoredItemIdInvalid` for non-existent monitored item IDs.

**Independent Test**: Create a subscription, then call SetTriggering with a fabricated monitored item ID; verify service result contains `BadMonitoredItemIdInvalid`.

- [ ] T018 [US2] Implement SetTriggering monitored item ID validation in `async-opcua-server/src/subscriptions/actor.rs`. Before processing `linksToAdd`/`linksToRemove`, check each monitored item ID exists in the subscription's item map; set `BadMonitoredItemIdInvalid` for missing IDs. **Before implementing: read OPC-10000-4 §5.13.5.3** ([reference.opcfoundation.org/specs/OPC-10000-4/5.13.5.3](https://reference.opcfoundation.org/specs/OPC-10000-4/5.13.5.3.md)). `BadMonitoredItemIdInvalid` is listed in Table 73 as a service-level result code.

- [ ] T019 [P] [US2] Add integration test `set_triggering_bad_monitored_item_id` in `async-opcua/tests/integration/subscriptions.rs` — creates subscription, calls SetTriggering with non-existent monitored item ID, verifies `BadMonitoredItemIdInvalid`

## Phase 8: User Story 3 — QueryFirst Empty View (Priority: P2)

**Goal**: QueryFirst returns `Good` with empty `QueryDataSet` list when no nodes match (OPC UA Part 4 Annex B §B.2.3).

**Independent Test**: Send QueryFirst with a filter matching zero nodes; verify response is `Good` with empty `queryDataSets`.

- [ ] T020 [US3] Fix QueryFirst status code in `async-opcua-server/src/session/services/query.rs`. When `queryDataSets` is empty after all node managers return no results, the service result must be `Good` — not any error code. `Bad_NothingToDo` (listed in OPC-10000-4 §B.2.3 Table B.5) is for expired continuation points, not empty result sets. **Before implementing: read OPC-10000-4 §B.2.3** ([reference.opcfoundation.org/specs/OPC-10000-4/b-2-3](https://reference.opcfoundation.org/specs/OPC-10000-4/b-2-3.md)).

- [ ] T021 [P] [US3] Add integration test `query_first_empty_result` in `async-opcua/tests/integration/read.rs` — sends QueryFirst with filter matching no nodes, verifies service result is `Good` with empty `queryDataSets`

## Phase 9: User Story 4 — HistoryUpdate Status Code (Priority: P3)

**Goal**: HistoryUpdate returns per-operation error codes instead of `BadNothingToDo`.

**Independent Test**: Send HistoryUpdate with an unsupported `performUpdateType`; verify operation-level result code is spec-correct.

- [ ] T022 [US4] Fix HistoryUpdate error propagation in `async-opcua-server/src/session/services/attribute.rs`. When a node manager returns an error from `history_update()`, ensure the per-operation result carries that error (e.g., `BadHistoryOperationUnsupported`) rather than defaulting to `BadNothingToDo`. **Before implementing: read OPC-10000-4 §5.11.5.4** ([reference.opcfoundation.org/specs/OPC-10000-4/5.11.5.4](https://reference.opcfoundation.org/specs/OPC-10000-4/5.11.5.4.md)). Table 58 lists the valid operation-level result codes: `Bad_NotWritable`, `Bad_HistoryOperationInvalid`, `Bad_HistoryOperationUnsupported`, `Bad_UserAccessDenied`.

- [ ] T023 [P] [US4] Add integration test `history_update_unsupported_type` in `async-opcua/tests/integration/history.rs` — sends HistoryUpdate targeting a non-historical node or with unsupported update type, verifies operation-level error code is one of the Table 58 codes (not `BadNothingToDo`)

## Phase 10: Polish & Verification

- [ ] T024 Run `cargo fmt --check` and `cargo clippy --workspace --all-features` — fix any issues
- [ ] T025 Run `cargo test -p async-opcua-server` — verify server tests pass
- [ ] T026 Run `cargo test -p async-opcua --test integration_tests` with `--features node-management` — verify all existing and new integration tests pass
- [ ] T027 Run `tools/ci-playbook.sh --ci` — verify full local CI gate passes

## Dependencies

```
T001 ──> T002 ──> T003,T004,T005 (foundational)
                      │
         ┌────────────┼──────────────────────────────┐
         ▼            ▼                              │
    T006 ──> T007    T010 ──> T011                   │
    (AddNodes)       (AddReferences)                 │
         │            │                              │
         ├────┐       ├────┐                         │
         ▼    ▼       ▼    ▼                         │
       T008  T009   T012  T013                       │
         │            │                              │
         │            │                              │
         └────────────┤                              │
                      │                              │
         ┌────────────┤                              │
         ▼            ▼                              │
        T014         T016                             │
      (DelNodes)   (DelRefs)                         │
         │            │                              │
         ▼            ▼                              │
        T015         T017                              │
         │            │                              │
         └────────────┘                              │
                      │                              │
         ┌────────────┼────────────┬─────────────────┘
         ▼            ▼            ▼
        T018         T020         T022
      (SetTrig)   (QueryFirst)  (HistUpd)
         │            │            │
         ▼            ▼            ▼
        T019         T021         T023
         │            │            │
         └────────────┴────────────┘
                      │
                      ▼
              T024..T027 (polish)
```

## Parallel Execution

- **Phase 3-6**: After foundational (T003-T005), T006-T007 (AddNodes) can run in parallel with T010-T011 (AddReferences). Tests T008-T009 can run in parallel with T012-T013. T014-T015 and T016-T017 are independent of each other.
- **Phase 7-9**: T018, T020, T022 are fully independent and can run in parallel; their tests T019, T021, T023 can also run in parallel.

## Suggested MVP Scope

Phase 1-2 + US1 (T001-T017) = 17 tasks, fixes 16 of 20 Gauntlet failures.
