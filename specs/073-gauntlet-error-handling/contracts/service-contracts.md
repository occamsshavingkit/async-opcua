# Contracts: Gauntlet Error-Handling Fixes

**Feature**: 073-gauntlet-error-handling
**Date**: 2026-07-12

No new external interfaces. This feature modifies internal validation within existing service handlers.
The OPC UA wire protocol (serialization, message types, status codes) is unchanged.

## Service Behavior Contracts

### NodeManagement

**Before**: All four NodeManagement operations return `BadServiceUnsupported` for every item when
`clients_can_modify_address_space` is OFF, regardless of input validity.

**After**: Each operation validates its inputs against the address space and returns operation-level
status codes from the OPC UA Part 4 tables. Only when all inputs are structurally valid does
the service fall through to the node manager's permission/feature gate.

### SetTriggering

**Before**: Subscriptions silently accept `SetTriggering` with non-existent monitored item IDs.

**After**: The subscription actor validates monitored item IDs and returns `BadMonitoredItemIdInvalid`
in the add/remove result arrays.

### QueryFirst

**Before**: May return `BadNothingToDo` when query matches zero nodes.

**After**: Returns `Good` with an empty `QueryDataSet` list.

### HistoryUpdate

**Before**: May return service-level `BadNothingToDo` for unsupported operations.

**After**: Returns operation-level error codes per OPC-10000-11 §5.2.2 Table 127.
