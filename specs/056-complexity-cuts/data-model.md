# Data Model: Complexity Cuts

**Feature**: 056-complexity-cuts
**Date**: 2026-07-03

## Entity: TypeSubtypeCache (Cut 2a)

**Location**: `async-opcua-nodes/src/type_tree.rs` — field on `DefaultTypeTree`

| Field | Type | Description |
|-------|------|-------------|
| `subtype_cache` | `moka::sync::Cache<(NodeId, NodeId), bool>` | Maps (child, ancestor) to `true` if child is subtype of ancestor. Populated lazily on first `is_subtype_of()` call per pair. |

**Invariants**:
- Keys are never invalidated — the type tree is immutable after startup.
- `(A, A)` must return `true` (a type is its own subtype).
- The cache is per-`DefaultTypeTree` instance and does not share state across instances.

**Lifecycle**:
- Created with `DefaultTypeTree::new()`, initialized empty.
- Populated lazily during browse/validation operations.
- Dropped when the `DefaultTypeTree` is dropped.

---

## Entity: BrowseNameIndex (Cut 2b)

**Location**: Shared across `node_manager/memory/mod.rs` and `node_manager/view.rs`

| Field | Type | Description |
|-------|------|-------------|
| `browse_name_index` | `Option<HashMap<(NodeId, QualifiedName), Vec<NodeId>>>` | Maps (parent NodeId, child BrowseName) to child NodeIds. `None` when invalidated and awaiting rebuild. |

**Invariants**:
- When `Some`, every (parent, BrowseName) pair in the address space is represented.
- When `node-management` feature is disabled, always `Some` after initialization.
- Filtering by reference type/direction is performed post-lookup, not encoded in the index key.

**Lifecycle**:
- Built at address space load time or lazily on first TranslateBrowsePaths call.
- Set to `None` when AddNodes, AddReferences, DeleteNodes, or DeleteReferences mutates the address space (only with `node-management` feature).
- Rebuilt lazily on next access after invalidation.

---

## Entity: ChannelSessionCounter (Cut 6)

**Location**: `async-opcua-server/src/session/manager.rs` — field on `SessionManager`

| Field | Type | Description |
|-------|------|-------------|
| `unactivated_by_channel` | `HashMap<u32, AtomicUsize>` | Per-secure-channel count of unactivated sessions. |

**Invariants**:
- The sum of all per-channel counts ≤ `max_sessions`.
- Each channel's count ≤ `max_unactivated_sessions_per_channel`.
- Counter for a channel may reach 0 but the key is retained (cheaper than remove+reinsert on next create).
- Once decremented to 0, the key remains; cleanup on SessionManager drop or explicit `remove(0)`.

**State transitions**:

```text
CreateSession on channel C → increment counter[C]
ActivateSession on session bound to channel C → decrement counter[C]
CloseSession / session expiry (unactivated only) → decrement counter[channel_id_of_session]
```

**Edge cases**:
- Cross-channel activation: the session was created on channel A, activated on channel B. Counter[A] decrements. Counter[B] unchanged (session arrives activated).
- Session created but immediately expired: counter decremented on expiry.

---

## Entity: PriorityCache (Cut 7)

**Location**: `async-opcua-server/src/subscriptions/session_subscriptions.rs`

| Field | Type | Description |
|-------|------|-------------|
| `cached_priority_order` | `Vec<u32>` | Subscription IDs ordered by priority (highest first), then by ID for stable round-robin. |
| `priority_cache_dirty` | `bool` | Set to `true` on subscription create, delete, or priority change. |

**Invariants**:
- When `priority_cache_dirty == false`, `cached_priority_order` accurately reflects the current subscription set in priority order.
- Subscriptions with equal priority must maintain stable ordering per OPC 10000-4 §5.14.2.2 (round-robin).
- The cache length always equals `self.subscriptions.len() - self.transferring.len()` (active subscriptions only).

**State transitions**:

```text
On subscription created → priority_cache_dirty = true
On subscription deleted → priority_cache_dirty = true
On subscription priority modified → priority_cache_dirty = true
On publish tick → if dirty, rebuild; else reuse cached_priority_order
```

---

## Entity: CachedChunkInfo (Cut 8)

**Location**: `async-opcua-core/src/comms/message_chunk.rs` — field on `MessageChunk`

| Field | Type | Description |
|-------|------|-------------|
| `cached_chunk_info` | `Option<ChunkInfo>` | Parsed once on first access, reused for subsequent accesses within the same message decode. |

**Invariants**:
- Set to `Some` after first `chunk_info()` call within a message's processing lifetime.
- The underlying chunk data (`self.data`) is immutable — no stale-cache risk.
- Consumed/dropped with the `MessageChunk`.

**Lifecycle**:
- Created as `None` when a `MessageChunk` is assembled from received bytes.
- Set to `Some(ChunkInfo)` on first `chunk_info()` call.
- `chunk_info()` returns `Ok(&ChunkInfo)` or the cloned cached value on subsequent calls.
- Dropped when the `MessageChunk` is dropped (end of message decode).
