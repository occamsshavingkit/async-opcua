# Data Model: Hot-Path and Lock Optimization

## Overview

This feature modifies internal data structures in the server's address space, session management, and request dispatch. No new domain entities are introduced; existing types are reorganized.

## Modified Types

### `AddressSpace` (address_space/mod.rs)

**Before**:
```rust
pub struct AddressSpace {
    node_map: DashMap<NodeId, NodeType>,
    references: Vec<Reference>,
    browse_name_index: HashMap<QualifiedName, Vec<NodeId>>,
    namespaces: Vec<Namespace>,
}
// Usage: Arc<RwLock<AddressSpace>>
```

**After (US1)**:
```rust
pub struct AddressSpaceCold {
    references: Vec<Reference>,
    browse_name_index: HashMap<QualifiedName, Vec<NodeId>>,
    namespaces: Vec<Namespace>,
}

pub struct AddressSpace {
    pub node_map: Arc<DashMap<NodeId, NodeType>>,           // HOT — direct concurrent access
    pub cold: parking_lot::RwLock<AddressSpaceCold>,  // COLD — rw-locked for writes
}
// Usage: Arc<AddressSpace>   (no outer RwLock)
```

**Relationships**:
- `node_map`: `NodeId → NodeType` (lock-free sharded concurrent map, `DashMap` internally)
- `cold.references`: `Vec<Reference>` — ordered list, mutated on node add/delete
- `cold.browse_name_index`: `QualifiedName → Vec<NodeId>` — invalidated on node add/delete
- `cold.namespaces`: `Vec<Namespace>` — read-only after server startup

**Invariants**:
- `node_map` entries MUST have corresponding entries in `references` and `browse_name_index` when a node is added
- Hot path (Read, Browse) MUST only access `node_map` without acquiring `cold` lock
- Cold path (AddNode, DeleteNode) MUST acquire `cold.write()` before mutating references/index

---

### Request Dispatch Context (controller.rs / request dispatch path)

**Before**:
```rust
// Session looked up per-operation:
let session = session_manager.find_by_token(token)?;
```

**After (US2)**:
```rust
// Cached in dispatch context:
struct RequestContext {
    // ... existing fields ...
    cached_session: Option<(NodeId, Arc<RwLock<Session>>)>,
}
// First lookup populates; subsequent use cached value
```

**Relationships**:
- `cached_session.0` (NodeId) matches `token` — used for cache-key validation
- `cached_session.1` is `Arc<RwLock<Session>>` — cloneable, keeps session alive while request holds reference

**Invariants**:
- Cache MUST be populated before first access to session-dependent operation
- Cache MUST NOT be used if the session token differs from cached token
- Dropping `RequestContext` drops the cached `Arc`, releasing session reference

---

### Deadline Queue (controller.rs)

**Before**:
```rust
// Per-request timer:
// tokio::time::sleep_until(deadline) in FuturesUnordered
```

**After (US3)**:
```rust
pub struct DeadlineQueue {
    /// Ordered map from deadline Instant to request IDs expiring at that time.
    entries: BTreeMap<Instant, Vec<RequestId>>,
    /// Set of completed request IDs (lazy cleanup).
    completed: HashSet<RequestId>,
}
```

**Operations**:
- `push(deadline: Instant, request_id: RequestId)` — insert into queue
- `pop_expired(now: Instant) -> Vec<RequestId>` — drain all entries with deadline ≤ now
- `mark_completed(request_id: RequestId)` — lazy removal on request completion
- `is_empty() -> bool` — check if any pending deadlines exist

**Relationships**:
- `RequestId` maps back to pending message in `FuturesUnordered` or inflight request tracker
- Each `RequestId` has exactly one deadline entry while inflight

**Invariants**:
- A `RequestId` MUST NOT appear in multiple deadline entries simultaneously
- Expired requests MUST be popped atomically (all requests at a given deadline processed together)
- Lazy cleanup MUST filter out completed requests during `pop_expired`

---

## Unchanged Types

### `Session` (session/)
- No structural changes. The `Arc<RwLock<Session>>` reference is cached, not the session internals.

### `SessionManager` (session/manager.rs)
- `find_by_token` method signature unchanged. Callers of the cached path may bypass it after first call.

### `NodeType` (nodes/)
- No changes. Referenced through `DashMap` as before.

### `SecureChannel` (core/comms/)
- No changes. Lock structure unchanged.
