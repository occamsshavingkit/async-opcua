# Contract: Subscription Command Variants

**Feature**: 057-completeness-closeout / US3
**Part of**: `async-opcua-server::subscriptions::actor`

## Before (Current)

```rust
pub(crate) enum SubscriptionCommand {
    LegacyCall(Box<dyn FnOnce(&mut SessionSubscriptions) + Send>),
    EnqueuePublish { now, now_instant, request, response: oneshot::Sender<()> },
    Stop,
}
```

## After (Target)

All `LegacyCall` call sites become dedicated enum variants. Each variant carries:
- Input parameters (captured from the closure)
- A `oneshot::Sender<ReturnType>` for the response

### Management Operations (Create/Modify/Delete)

| Variant | Inputs | Return Type |
|---------|--------|-------------|
| `CreateSubscription` | `request: CreateSubscriptionRequest, info: SubscriptionInfo` | `Result<u32, StatusCode>` |
| `ModifySubscription` | `request: ModifySubscriptionRequest, info: SubscriptionInfo` | `Result<(), StatusCode>` |
| `DeleteSubscriptions` | `ids: Vec<u32>` | `Result<(), StatusCode>` |
| `SetPublishingMode` | `request: SetPublishingModeRequest` | `Result<(), StatusCode>` |
| `Republish` | `request: RepublishRequest` | `Result<(), StatusCode>` |

### Monitored Item Operations

| Variant | Inputs | Return Type |
|---------|--------|-------------|
| `CreateMonitoredItems` | `sub_id: u32, requests: Vec<MonitoredItemCreateRequest>` | `Result<Vec<MonitoredItemCreateResult>, StatusCode>` |
| `ModifyMonitoredItems` | `sub_id: u32, requests: Vec<MonitoredItemModifyRequest>` | `Result<Vec<MonitoredItemModifyResult>, StatusCode>` |
| `DeleteMonitoredItems` | `sub_id: u32, items: Vec<u32>` | `Result<(), StatusCode>` |
| `SetMonitoringMode` | `sub_id: u32, mode: MonitoringMode, items: Vec<u32>` | `Result<(), StatusCode>` |
| `SetTriggering` | `sub_id: u32, triggering_item_id: u32, links_to_add: Vec<u32>, links_to_remove: Vec<u32>` | `Result<(), StatusCode>` |

### Read-Only Queries

| Variant | Inputs | Return Type |
|---------|--------|-------------|
| `SubscriptionIds` | — | `Vec<u32>` |
| `MonitoredItemRefs` | — | `Vec<MonitoredItemRef>` |
| `SubscriptionAndItemData` | — | `(Vec<u32>, Vec<MonitoredItemRef>)` |
| `MonitoredItemCount` | `sub_id: u32` | `Option<usize>` |
| `MonitoredItemNodeIds` | `sub_id: u32, ids: Vec<u32>` | `Vec<Option<NodeId>>` |
| `AvailableSequenceNumbers` | `sub_id: u32` | `Option<Vec<u32>>` |
| `SubscriptionDiagnostics` | — | `SubscriptionDiagnosticsDataType` |

### State Mutation

| Variant | Inputs | Return Type |
|---------|--------|-------------|
| `UpdateOwner` | `key: SecurityToken, type_tree_for_user: Arc<...>` | `()` |
| `ApplyRevalidatedValues` | `values: HashMap<u32, DataValue>` | `Vec<StatusCode>` |
| `MarkTransferring` | `sub_id: u32` | `Result<(), StatusCode>` |
| `CloneForTransfer` | `sub_id: u32` | `Option<(Subscription, Vec<Notification>)>` |
| `InsertForTransfer` | `sub: Subscription, notifs: Vec<Notification>` | `Result<(), StatusCode>` |
| `UserTokenMatches` | `key: SecurityToken` | `bool` |

## Invariants

- **Behavioral equivalence**: For every call site, the before (LegacyCall) and after (dedicated variant) produce the same result for all inputs. This is verified by the existing test suite passing without modification.
- **No LegacyCall after migration**: The `LegacyCall` variant and the `legacy()` helper method on `SubscriptionActorHandle` are deleted. `rgrep LegacyCall` returns zero results.
- **Exhaustive matching**: The `SubscriptionActor::run()` match arm for each variant explicitly handles the operation and sends the response. No wildcard/fallthrough arms for management operations — the compiler checks all variants.
- **Channel semantics unchanged**: All variants use `mpsc::unbounded_channel()` for command dispatch and `oneshot` for response — same as LegacyCall, no change to ordering or fairness guarantees.

## Implementation Notes

- The `legacy()` helper method is in `SubscriptionActorHandle` (actor.rs:57-69). It boxes the closure and sends `SubscriptionCommand::LegacyCall`. After migration, this method and all its call sites are deleted.
- The `LegacyCall` match arm in `SubscriptionActor::run()` (actor.rs:120-122) is replaced with N dedicated match arms, one per new variant.
- The `EnqueuePublish` variant and the notification ring / tick loop are unchanged — only management operations migrate.
