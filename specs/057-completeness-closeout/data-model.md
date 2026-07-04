# Data Model: Completeness Closeout

**Feature**: 057-completeness-closeout
**Date**: 2026-07-04

## OCSP Fetch Policy

| Field | Type | Description |
|-------|------|-------------|
| `mode` | enum: Off, Soft, Strict | Fetch policy. Off = no live fetch (default). Soft = fetch but fall back to CRL if unreachable. Strict = hard-fail on unreachable. |
| `timeout` | Duration | HTTP request timeout (default 5s) |
| `max_response_size` | usize | Maximum OCSP response size in bytes (default 64KB) |

### OCSP Cache Entry

| Field | Type | Description |
|-------|------|-------------|
| `issuer_name_hash` | Vec<u8> | SHA-1 hash of issuer DN |
| `issuer_key_hash` | Vec<u8> | SHA-1 hash of issuer public key |
| `serial_number` | Vec<u8> | Certificate serial number |
| `response` | Vec<u8> | DER-encoded OCSP response |
| `this_update` | DateTime | Response validity start |
| `next_update` | DateTime | Response validity end |
| `fetched_at` | Instant | When the response was fetched |

**Lifecycle**: On revocation check, look up cache key `(issuer_name_hash, issuer_key_hash, serial_number)`. If found and `next_update > now`, use cached response. If not found or expired, fetch live. On `Off` mode, skip both cache and fetch.

## Per-Endpoint Certificate

### ServerEndpoint (extended)

| Field | Type | Description |
|-------|------|-------------|
| `certificate_path` | Option<PathBuf> | Per-endpoint cert. Falls back to server-level default. |
| `private_key_path` | Option<PathBuf> | Per-endpoint private key. Falls back to server-level default. |

**Validation rule**: If `security_policy` requires RSA (Basic256Sha256, etc.) and the cert is ECC, fail at startup. If policy requires ECC (EccNistP256/P384) and cert is RSA, fail at startup. Policy "None" requires no cert.

### ServerInfo (modified)

| Field | Type | Description |
|-------|------|-------------|
| `endpoint_certs` | HashMap<EndpointIdentifier, Option<X509>> | Per-endpoint cert map. Replaces single `server_certificate`. |

**Lookup**: At secure channel creation, use the endpoint identifier (path + security policy + mode) to find the matching cert.

## Subscription Command Variants

### Before (existing)

```
SubscriptionCommand {
    LegacyCall(Box<dyn FnOnce(&mut SessionSubscriptions) + Send>),
    EnqueuePublish { ... },
    Stop,
}
```

### After (target)

Each management operation becomes a dedicated variant:

```
SubscriptionCommand {
    // Management operations (replace LegacyCall)
    CreateSubscription { request, info, response: oneshot::Sender<Result<u32, StatusCode>> },
    ModifySubscription { request, info, response: oneshot::Sender<Result<(), StatusCode>> },
    DeleteSubscriptions { ids, response: oneshot::Sender<Result<(), StatusCode>> },
    SetPublishingMode { request, response: oneshot::Sender<Result<(), StatusCode>> },
    CreateMonitoredItems { sub_id, requests, response: oneshot::Sender<Result<Vec<MonitoredItemCreateResult>, StatusCode>> },
    ModifyMonitoredItems { sub_id, requests, response: oneshot::Sender<Result<Vec<MonitoredItemModifyResult>, StatusCode>> },
    DeleteMonitoredItems { sub_id, items, response: oneshot::Sender<Result<(), StatusCode>> },
    SetMonitoringMode { sub_id, mode, items, response: oneshot::Sender<Result<(), StatusCode>> },
    SetTriggering { sub_id, ... },
    Republish { request, response: oneshot::Sender<Result<(), StatusCode>> },
    TransferSubscriptions { ... },

    // Read-only queries
    SubscriptionIds { response: oneshot::Sender<Vec<u32>> },
    MonitoredItemRefs { response: oneshot::Sender<Vec<MonitoredItemRef>> },
    SubscriptionAndMonitoredItemData { response: oneshot::Sender<(Vec<u32>, Vec<MonitoredItemRef>)> },
    MonitoredItemCount { sub_id, response: oneshot::Sender<Option<usize>> },
    MonitoredItemNodeIds { sub_id, ids, response: oneshot::Sender<Vec<Option<NodeId>>> },
    AvailableSequenceNumbers { sub_id, response: oneshot::Sender<Option<Vec<u32>>> },
    SubscriptionDiagnostics { response: oneshot::Sender<SubscriptionDiagnosticsDataType> },

    // State mutation
    UpdateOwner { key, type_tree_for_user, response: oneshot::Sender<()> },
    ApplyRevalidatedValues { values, response: oneshot::Sender<Vec<StatusCode>> },
    LegacyDeleteMonitoredItemsViaSession { sub_id, items, response: oneshot::Sender<()> },

    // Publish / stop (unchanged)
    EnqueuePublish { now, now_instant, request, response: oneshot::Sender<()> },
    Stop,
}
```

**(Exact variant names and counts TBD during task decomposition — the above is representative.)**

## Chat Server Information Model

### ChatLogsType (ObjectType)
- **BaseType**: BaseObjectType
- **SupportsEvents**: true
- **HasNotifier**: Server (so Server Object sends ChatLogs events)

### Post (Method)
- **TypeDefinition**: PostMethodType
- **Input Arguments**: Name (String), Content (String)
- **Related Event**: ChatLogEventType (AlwaysGeneratesEvent)
- **Side effect**: Increments PostCount

### PostCount (Variable)
- **DataType**: UInt32
- **Initial value**: 0
- **Updated on**: Post call

### ChatLogEventType (ObjectType)
- **BaseType**: BaseEventType
- **Properties**: ChatLog (ChatLogType)

### ChatLogType (VariableType)
- **DataType**: ChatLog (Structure)
- **Description**: Represents a chat log entry

### ChatLog (Structure/DataType)
| Field | Type | Description |
|-------|------|-------------|
| `At` | DateTime | When the message was posted |
| `Name` | String | Sender name |
| `Content` | String | Message content |

### Instance Hierarchy
```
ObjectsFolder
  └── ChatLogs (ChatLogsType) [HasNotifier → Server]
        ├── Post (Method)
        └── PostCount (Variable, UInt32)
```
