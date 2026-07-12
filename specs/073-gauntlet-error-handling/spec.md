# Feature Specification: Gauntlet Error-Handling Fixes

**Feature Branch**: `072-gauntlet-error-handling`  
**Created**: 2026-07-12  
**Status**: Draft  
**Input**: User description: "Fix remaining OPC UA Gauntlet error-handling failures from issue #282 — NodeManagement input validation, SetTriggering, QueryFirst, and HistoryUpdate status-code corrections."

## Context

The OPC UA Gauntlet compliance test tool reported 38 failures against `demo-server` at full profile.
Three were addressed in PR #284 (null NodeId handling for Read/Write/Call). The remaining 20 failures
in four categories share a common root cause: the server either returns `BadServiceUnsupported` for
services that should validate their inputs before rejecting them, or returns incorrect status codes
for edge cases.

These failures are in the **error-handling surface** — input validation and status-code correctness —
not in implementing the full service semantics. The fix is to add operation-level validation that
returns the OPC UA Part 4 specified status codes for bad inputs, without necessarily implementing
the full service behavior.

### Service Grounding References

| Service | Operation-Level Status Table | Spec Section |
|---|---|---|
| AddNodes | Table 28 | OPC-10000-4 §5.8.2.4 |
| AddReferences | Table 31 | OPC-10000-4 §5.8.3.4 |
| DeleteNodes | Table 34 | OPC-10000-4 §5.8.4.4 |
| DeleteReferences | Table 37 | OPC-10000-4 §5.8.5.3 |
| SetTriggering | Table 65 | OPC-10000-4 §5.13.6 |
| QueryFirst | Annex B B.2.3 | OPC-10000-4 Annex B |
| HistoryUpdate | Table 127 | OPC-10000-11 §5.2.2 |

## User Scenarios & Testing *(mandatory)*

### User Story 1 — NodeManagement Input Validation (Priority: P1)

As a conformance test tool, when I send AddNodes/AddReferences/DeleteNodes/DeleteReferences requests
with deliberately invalid inputs, the server returns operation-level error codes matching OPC UA Part 4
Tables 28/31/34/37 instead of a blanket `BadServiceUnsupported`.

**Why this priority**: NodeManagement accounts for 16 of the 20 remaining failures. Fixing input
validation here has the largest conformance impact.

**Independent Test**: Run the Gauntlet NodeManagement test suite against the demo server and verify
that 16 previously-failing NodeManagement tests now pass with the correct status codes. Individual
operations can be tested independently: validate AddNodes errors without implementing AddReferences,
or vice versa.

**Acceptance Scenarios**:

1. **AddNodes — BadNodeIdExists**: **Given** a client sends AddNodes with a `requestedNewNodeId` that already
   exists in the address space, **When** the server processes the request, **Then** the per-operation result
   contains `BadNodeIdExists` (not `BadServiceUnsupported`).

2. **AddNodes — BadParentNodeIdInvalid**: **Given** a client sends AddNodes with a `parentNodeId` that
   is not a valid parent (e.g., points to a node that cannot have children), **When** the server
   processes the request, **Then** the per-operation result contains `BadParentNodeIdInvalid`.

3. **AddNodes — BadReferenceNotAllowed**: **Given** a client sends AddNodes specifying a
   `referenceTypeId` that is not allowed for that parent-child relationship, **When** the server
   validates the request, **Then** the per-operation result contains `BadReferenceNotAllowed`.

4. **AddNodes — BadNodeClassInvalid**: **Given** a client sends AddNodes specifying a
   `nodeClass` that is not valid or incompatible with the parent, **When** the server validates the
   request, **Then** the per-operation result contains `BadNodeClassInvalid`.

5. **AddNodes — BadTypeDefinitionInvalid**: **Given** a client sends AddNodes with a `typeDefinition`
   that does not exist or is invalid for the requested NodeClass, **When** the server validates the
   request, **Then** the per-operation result contains `BadTypeDefinitionInvalid`.

6. **AddNodes — BadUserAccessDenied**: **Given** a client without write permissions sends an AddNodes
   request, **When** the server checks authorization, **Then** the per-operation result contains
   `BadUserAccessDenied`.

7. **AddReferences — BadSourceNodeIdInvalid**: **Given** a client sends AddReferences with a
   `sourceNodeId` that does not exist, **When** the server validates the request, **Then** the
   per-operation result contains `BadSourceNodeIdInvalid`.

8. **AddReferences — BadTargetNodeIdInvalid**: **Given** a client sends AddReferences with a
   `targetNodeId` that does not exist, **When** the server validates the request, **Then** the
   per-operation result contains `BadTargetNodeIdInvalid`.

9. **AddReferences — BadReferenceTypeIdInvalid**: **Given** a client sends AddReferences with a
   `referenceTypeId` that is not a valid ReferenceType NodeId, **When** the server validates the
   request, **Then** the per-operation result contains `BadReferenceTypeIdInvalid`.

10. **AddReferences — BadDuplicateReferenceNotAllowed**: **Given** a client sends AddReferences
    for a reference that already exists between the given nodes, **When** the server validates the
    request, **Then** the per-operation result contains `BadDuplicateReferenceNotAllowed`.

11. **AddReferences — BadInvalidSelfReference**: **Given** a client sends AddReferences with
    identical source and target NodeIds, **When** the server validates the request, **Then** the
    per-operation result contains `BadInvalidSelfReference`.

12. **DeleteNodes — BadNodeIdUnknown**: **Given** a client sends DeleteNodes for a NodeId that
    does not exist in the address space, **When** the server validates the request, **Then** the
    per-operation result contains `BadNodeIdUnknown`.

13. **DeleteNodes — BadNodeNotInView**: **Given** a client sends DeleteNodes with a
    `viewId` that constrains visibility, and the target node is not present in that view, **When**
    the server validates the request, **Then** the per-operation result contains
    `BadNodeNotInView` (only when a view is specified).

14. **DeleteNodes — BadUserAccessDenied**: **Given** a client without delete permissions sends a
    DeleteNodes request, **When** the server checks authorization, **Then** the per-operation result
    contains `BadUserAccessDenied`.

15. **DeleteReferences — BadSourceNodeIdInvalid**: **Given** a client sends DeleteReferences with a
    `sourceNodeId` that does not exist, **When** the server validates the request, **Then** the
    per-operation result contains `BadSourceNodeIdInvalid`.

16. **DeleteReferences — BadTargetNodeIdInvalid**: **Given** a client sends DeleteReferences with a
    `targetNodeId` that does not exist, **When** the server validates the request, **Then** the
    per-operation result contains `BadTargetNodeIdInvalid`.

---

### User Story 2 — SetTriggering Status Code (Priority: P2)

As a conformance test tool, when I send a SetTriggering request with a non-existent monitored item
ID, the server returns `BadMonitoredItemIdInvalid` at the service result level (not silently
accepting the request or returning a wrong code).

**Why this priority**: A single, well-defined status-code fix with a clear spec reference.
Established pattern from the subscription service already in place.

**Independent Test**: Send a SetTriggering request referencing a monitored item ID that was never
created on the server; verify the service result contains `BadMonitoredItemIdInvalid`.

**Acceptance Scenarios**:

1. **SetTriggering — BadMonitoredItemIdInvalid**: **Given** a client has an active subscription but
   references a monitored item ID that does not exist, **When** the client calls SetTriggering,
   **Then** the service result is `BadMonitoredItemIdInvalid` (OPC-10000-4 §5.13.6 Table 65).

---

### User Story 3 — QueryFirst Empty View (Priority: P2)

As a conformance test tool, when I send a QueryFirst request with a valid but empty query (no nodes
match the query criteria), the server returns a `Good` service result with empty query data sets
rather than `BadNothingToDo`.

**Why this priority**: A single status-code correction. The spec explicitly lists `BadNothingToDo`
as an operation-level code used when a continuation point has expired, not when no results match.

**Independent Test**: Send a QueryFirst request with a node type filter that matches nothing in the
address space; verify the service result is `Good` with empty results.

**Acceptance Scenarios**:

1. **QueryFirst — empty result set**: **Given** a client sends QueryFirst with filter criteria that
   matches zero nodes, **When** the server processes the request, **Then** the service result is
   `Good` and the query data set list is empty (OPC-10000-4 Annex B §B.2.3).

---

### User Story 4 — HistoryUpdate Status Code (Priority: P3)

As a conformance test tool, when I send HistoryUpdate requests that cannot be processed (unsupported
operation types, or updates for nodes without history), the server returns operation-level error codes
rather than the generic `BadNothingToDo`.

**Why this priority**: Two failures, lower priority because HistoryUpdate is a partially-implemented
feature area. The fix is to replace the default `BadNothingToDo` with the appropriate operation-level
status codes.

**Independent Test**: Send HistoryUpdate requests with unsupported `performUpdateType` values; verify
operation-level results carry the correct error codes per OPC-10000-11 §5.2.2 Table 127.

**Acceptance Scenarios**:

1. **HistoryUpdate — unsupported update type**: **Given** a client sends HistoryUpdate with a
   `performUpdateType` value not supported by the server, **When** the server processes the
   request, **Then** the operation-level result contains a spec-appropriate error code rather than
   `BadNothingToDo`.

2. **HistoryUpdate — node without history**: **Given** a client sends HistoryUpdate targeting a
   node that does not support history, **When** the server validates the request, **Then** the
   operation-level result contains an appropriate error code rather than `BadNothingToDo`.

---

### Edge Cases

- **NodeManagement with non-existent parent**: Validate `parentNodeId` existence before attempting
  name uniqueness checks.
- **AddReferences with OPC UA namespace NodeIds**: Source/target NodeIds in namespace 0 should
  still be validated for existence against the nodeset.
- **SetTriggering with null/missing parameters**: Parameters that are null or absent should be
  rejected at the service-level decoding phase.
- **QueryFirst on a server with no Query support**: The server should still return `Good` with empty
  results rather than an error.
- **HistoryUpdate on historical nodes with wrong update type**: Operation-level results should
  reflect the specific failure at the operation level, not the service level.
- **Concurrent NodeManagement requests**: Each operation in a multi-operation request gets its own
  result entry; validation errors for one operation do not prevent validation of others.

## Requirements *(mandatory)*

### Functional Requirements

#### NodeManagement Validation

- **FR-001**: The server MUST accept AddNodes, AddReferences, DeleteNodes, and DeleteReferences
  requests and return operation-level results with spec-specified status codes for validation
  failures, instead of a service-level `BadServiceUnsupported`.
- **FR-002**: AddNodes MUST validate `parentNodeId` existence and return `BadParentNodeIdInvalid`
  when the parent node does not exist or cannot have children.
- **FR-003**: AddNodes MUST validate `requestedNewNodeId` against existing nodes and return
  `BadNodeIdExists` on collision.
- **FR-004**: AddNodes MUST validate `referenceTypeId` against allowed reference types for the
  parent-child relationship and return `BadReferenceNotAllowed` when disallowed.
- **FR-005**: AddNodes MUST validate `nodeClass` compatibility with the parent node and return
  `BadNodeClassInvalid` when incompatible.
- **FR-006**: AddNodes MUST validate `typeDefinition` existence and return
  `BadTypeDefinitionInvalid` when the type definition NodeId is invalid.
- **FR-007**: AddReferences MUST validate `sourceNodeId` existence and return
  `BadSourceNodeIdInvalid` when not found.
- **FR-008**: AddReferences MUST validate `targetNodeId` existence and return
  `BadTargetNodeIdInvalid` when not found.
- **FR-009**: AddReferences MUST validate `referenceTypeId` as a valid ReferenceType NodeId and
  return `BadReferenceTypeIdInvalid` when invalid.
- **FR-010**: AddReferences MUST detect duplicate references and return
  `BadDuplicateReferenceNotAllowed` when a matching reference already exists.
- **FR-011**: AddReferences MUST detect self-references (`sourceNodeId == targetNodeId`) and
  return `BadInvalidSelfReference` when such references are disallowed.
- **FR-012**: DeleteNodes MUST validate NodeId existence and return `BadNodeIdUnknown` when
  the target node does not exist.
- **FR-013**: DeleteNodes MUST honor the `viewId` parameter and return `BadNodeNotInView` when
  the target node is not visible in the specified view.
- **FR-014**: DeleteReferences MUST validate `sourceNodeId` and `targetNodeId` existence and
  return appropriate operation-level errors.
- **FR-015**: All NodeManagement operations MUST return per-operation results in the same order
  and count as the input operations array.
- **FR-016**: NodeManagement input validation MUST be performed even when the writable address
  space is disabled — validation errors take precedence over access-denied errors in the
  operation result ordering defined by each spec table.

#### SetTriggering

- **FR-017**: SetTriggering MUST validate that all `linksToAdd` and `linksToRemove` monitored
  item IDs reference existing monitored items in the active subscription and return
  `BadMonitoredItemIdInvalid` when they do not (OPC-10000-4 §5.13.6 Table 65).

#### QueryFirst

- **FR-018**: QueryFirst MUST return a service-level `Good` result with empty query data sets
  when no nodes match the query criteria, rather than `BadNothingToDo`.

#### HistoryUpdate

- **FR-019**: HistoryUpdate MUST return operation-level results with appropriate error codes for
  unsupported update types or non-historical nodes, rather than a service-level `BadNothingToDo`.

### Key Entities

- **AddNodesItem / AddNodesResult**: Request item and per-operation result for the AddNodes service
  (OPC-10000-4 §5.8.2.2, §5.8.2.4).
- **AddReferencesItem / AddReferencesResult**: Same for AddReferences (OPC-10000-4 §5.8.3.2, §5.8.3.4).
- **DeleteNodesItem / DeleteNodesResult**: Same for DeleteNodes (OPC-10000-4 §5.8.4.2, §5.8.4.4).
- **DeleteReferencesItem / DeleteReferencesResult**: Same for DeleteReferences (OPC-10000-4 §5.8.5.2, §5.8.5.3).
- **SetTriggeringRequest/Response**: Service request/response for linking monitoring item triggers
  (OPC-10000-4 §5.13.6).
- **QueryFirstRequest/Response**: Service request/response for the Query service set (OPC-10000-4 §5.9.4).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 16 Gauntlet NodeManagement tests (P04-S05.8.2.3-001 through P04-S05.8.5.3-002)
  pass with their expected operation-level status codes.
- **SC-002**: Gauntlet test T127 (SetTriggering) passes with `BadMonitoredItemIdInvalid`.
- **SC-003**: Gauntlet test T107 (QueryFirst) passes with `Good` result.
- **SC-004**: Gauntlet tests T116 and T234 (HistoryUpdate) pass with appropriate operation-level
  error codes.
- **SC-005**: No existing Gauntlet tests regress — the pass count must not drop below the current
  baseline of 181 (plus the 3 fixed in PR #284, for 184 total before these fixes).
- **SC-006**: 20 additional Gauntlet tests pass, raising the total from 184 to 204.

## Assumptions

- The demo server will continue to run with `clients_can_modify_address_space = false` (default).
  NodeManagement input validation produces the same error codes regardless of this flag.
- The server does not need to implement full NodeManagement, Query, or HistoryUpdate functionality —
  only input validation and correct status codes at the service boundaries.
- NodeManagement validation can use existing `address_space` read-only access to check NodeId
  existence, node class, and reference existence.
- No change to the wire protocol, types, or serialization is needed.
- Integration tests can be added in the existing `async-opcua/tests/integration/` test harness
  using the `gauntlet` or `compliance` module conventions.
