# Research: Gauntlet Error-Handling Fixes

**Feature**: 073-gauntlet-error-handling
**Date**: 2026-07-12

## Decision 1: Where to add NodeManagement validation

**Decision**: Add input validation in `async-opcua-server/src/session/services/node_management.rs`
BEFORE dispatching to node managers, using the read-only `AddressSpace` for existence/reference checks.

**Rationale**:
- Current flow: request arrives → `node_management.rs` service handler → dispatches each item to
  owning `NodeManager` → memory manager checks `clients_can_modify_address_space` → returns
  `BadServiceUnsupported` per item.
- The issue is that `BadServiceUnsupported` masks the input validation. The Gauntlet sends
  deliberately bad inputs (non-existent NodeIds, duplicate references, self-references) expecting
  specific error codes.
- Adding validation at the service handler level (before dispatch) means bad inputs get proper
  error codes regardless of whether the `node-management` feature is enabled or whether
  `clients_can_modify_address_space` is true.
- The `AddressSpace` is available via `context.address_space()` for read-only existence checks.

**Alternatives considered**:
- Add validation inside `memory_mgr_impl.rs` only: Rejected because validation is needed even
  when `clients_can_modify_address_space` is OFF, and other NodeManagers (custom, facade) would
  also benefit from centralized validation.
- Add validation at the trait default implementation level: Rejected because `NodeMutator` trait
  defaults return `Err(BadServiceUnsupported)` immediately without any context about the address space.

## Decision 2: QueryFirst status code

**Decision**: Change `query_first()` in `async-opcua-server/src/session/services/query.rs` to
return `Good` with empty `QueryDataSet` list when no nodes match the query, instead of any error code.

**Rationale**:
- OPC-10000-4 Annex B §B.2.3 lists `BadNothingToDo` only for continuation point expiry, not for
  empty result sets.
- An empty result set is a valid, successful outcome — the query executed but matched nothing.
- The existing code already iterates through node managers; the fix is ensuring the response is
  constructed with `Good` status when `query_data_sets` is empty.

**Alternatives considered**:
- Return `BadNoMatch`: Rejected — that code is for TranslateBrowsePaths, not Query.
- Leave as-is: Rejected — fails Gauntlet T107.

## Decision 3: SetTriggering validation

**Decision**: In `async-opcua-server/src/subscriptions/actor.rs`, before adding/removing triggering
links, validate that each monitored item ID in `linksToAdd` and `linksToRemove` exists in the
subscription's monitored item list.

**Rationale**:
- OPC-10000-4 §5.13.6 Table 65 lists `BadMonitoredItemIdInvalid` as a service-level result code.
- The subscription actor already has access to the monitored item map.
- We can check existence and populate the per-operation add/remove results with the error code.

**Alternatives considered**:
- Validate at the message handler level: Rejected — only the subscription actor has the
  monitored item state needed for validation.

## Decision 4: HistoryUpdate operation-level codes

**Decision**: In `async-opcua-server/src/session/services/attribute.rs`, the `history_update()`
function should ensure per-operation results carry the correct error code when a node manager
rejects an operation, rather than defaulting to `BadNothingToDo`.

**Rationale**:
- The service handler already iterates through node managers via `invoke_service_concurrently_mut`
  and collects per-operation results.
- The default `HistoryProvider::history_update()` trait method returns
  `Err(StatusCode::BadHistoryOperationUnsupported)`.
- The fix is to ensure this error code propagates to per-operation results correctly, not to a
  service-level error.
- `BadNothingToDo` is not listed in OPC-10000-11 §5.2.2 Table 127 for HistoryUpdate operation results.

**Alternatives considered**:
- Implement full HistoryUpdate: Rejected — out of scope; this is error-handling only.

## Decision 5: Validation ordering (NodeManagement)

**Decision**: Validation order follows the priority list in each OPC UA Part 4 operation-level
status table. Results must contain one entry per input item in the same order.

**Rationale**:
- Each spec table defines an ordered list of status codes; validation follows that order.
- Per OPC-10000-4 §5.8.2.4 Table 28 (AddNodes), the first applicable error for each item wins.
- Per-operation errors are independent — item N's failure does not prevent validating item N+1.

**Alternatives considered**:
- Fail-fast on first error: Rejected — violates the per-operation result contract.
- Validate only at service level: Rejected — each item needs its own result entry.
