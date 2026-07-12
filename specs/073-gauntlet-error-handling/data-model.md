# Data Model: Gauntlet Error-Handling Fixes

**Feature**: 073-gauntlet-error-handling
**Date**: 2026-07-12

This feature adds no new entities. It modifies validation logic on existing OPC UA service types.

## Existing Types (no schema changes)

### NodeManagement Items

- **AddNodesItem** (OPC-10000-4 §5.8.2.2): `parentNodeId`, `referenceTypeId`, `requestedNewNodeId`,
  `browseName`, `nodeClass`, `nodeAttributes`, `typeDefinition`
- **AddNodesResult** (OPC-10000-4 §5.8.2.4): `statusCode`
- **AddReferencesItem** (OPC-10000-4 §5.8.3.2): `sourceNodeId`, `referenceTypeId`, `isForward`,
  `targetServerUri`, `targetNodeId`, `targetNodeClass`
- **AddReferencesResult** (OPC-10000-4 §5.8.3.4): `statusCode` (source-level)
- **DeleteNodesItem** (OPC-10000-4 §5.8.4.2): `nodeId`, `deleteTargetReferences`
- **DeleteNodesResult** (OPC-10000-4 §5.8.4.4): `statusCode`
- **DeleteReferencesItem** (OPC-10000-4 §5.8.5.2): `sourceNodeId`, `referenceTypeId`, `isForward`,
  `targetNodeId`, `deleteBidirectional`
- **DeleteReferencesResult** (OPC-10000-4 §5.8.5.3): `sourceStatusCode`, `targetStatusCode`

### SetTriggering

- **SetTriggeringItem** (binary-encoded): `triggeringItemId` (MonitoredItemId)
- **SetTriggeringResponse** (OPC-10000-4 §5.13.6): `addResults[]`, `removeResults[]`

### QueryFirst

- **QueryFirstResponse** (OPC-10000-4 §5.9.4): `queryDataSets[]`, `continuationPoint`,
  `parsingResults[]`, `diagnosticInfos[]`, `filterResult`

### HistoryUpdate

- **HistoryUpdateDetails** (OPC-10000-11 §5.2.2): Extension object containing
  `UpdateDataDetails`, `UpdateEventDetails`, etc.
- **HistoryUpdateResult** (OPC-10000-11 §5.2.2): `statusCode`, `operationResults[]`

## Validation Rules

### AddNodes (Table 28)
| Priority | Check | Error |
|----------|-------|-------|
| 1 | `requestedNewNodeId` already exists | `BadNodeIdExists` |
| 2 | `parentNodeId` does not exist or cannot have children | `BadParentNodeIdInvalid` |
| 3 | `referenceTypeId` not a valid hierarchical reference type for parent-child | `BadReferenceNotAllowed` |
| 4 | `nodeClass` incompatible with parent | `BadNodeClassInvalid` |
| 5 | `typeDefinition` does not exist or is wrong class | `BadTypeDefinitionInvalid` |
| 6 | User lacks write permission | `BadUserAccessDenied` |

### AddReferences (Table 31)
| Priority | Check | Error |
|----------|-------|-------|
| 1 | `sourceNodeId` does not exist | `BadSourceNodeIdInvalid` |
| 2 | `targetNodeId` does not exist | `BadTargetNodeIdInvalid` |
| 3 | `referenceTypeId` is not a valid ReferenceType NodeId | `BadReferenceTypeIdInvalid` |
| 4 | Reference already exists (duplicate) | `BadDuplicateReferenceNotAllowed` |
| 5 | `sourceNodeId == targetNodeId` (self-reference) | `BadInvalidSelfReference` |

### DeleteNodes (Table 34)
| Priority | Check | Error |
|----------|-------|-------|
| 1 | `nodeId` does not exist | `BadNodeIdUnknown` |
| 2 | `viewId` specified and node not in view | `BadNodeNotInView` |
| 3 | User lacks delete permission | `BadUserAccessDenied` |

### DeleteReferences (Table 37)
| Priority | Check | Error |
|----------|-------|-------|
| 1 | `sourceNodeId` does not exist | `BadSourceNodeIdInvalid` |
| 2 | `targetNodeId` does not exist | `BadTargetNodeIdInvalid` |
