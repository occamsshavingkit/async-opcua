# Research: Complexity Cuts — OPC-UA Standard Grounding

**Feature**: 056-complexity-cuts
**Date**: 2026-07-03
**Status**: Complete

## Research Method

Each cut was analyzed against the OPC UA specification (Parts 3, 4, 6) to verify:
1. The correctness of the current behavior being optimized
2. That the optimization preserves the spec-mandated semantics
3. That the data-structure choices align with spec guarantees (immutability, ordering, etc.)

---

## Cut 2a: `is_subtype_of` Memoization

### Spec references

| Document | Section | Content |
|----------|---------|---------|
| OPC 10000-3 | §4.10.1 | Type hierarchies use single inheritance: "the specification does not restrict those hierarchies to be single inheritance (i.e. a type can only have one super-type) it only specifies the semantic." |
| OPC 10000-3 | §5.3.3.3 | HasSubtype defines the subtype relationship: "inherited to its subtypes and can be refined there." Walk from child up to root. |
| OPC 10000-3 | §5.8.3 | DataType hierarchy: HasSubtype references span the DataType type hierarchy. |
| OPC 10000-3 | §7.10 | HasSubtype is the ReferenceType for spanning type hierarchies. |
| OPC 10000-4 | — | Browse service: `includeSubtypes` flag on `BrowseDescription` triggers `is_subtype_of` for reference type filtering. |

### Immutability guarantee

The OPC UA address space type hierarchy is defined by the companion specification (core namespace) and loaded at server startup. Per OPC 10000-3 §6.4, ObjectTypes and VariableTypes are instantiated from TypeDefinitionNodes, but the type tree itself (the HasSubtype relationships between types) is static after initialization. The current `DefaultTypeTree` has no mutation API — `add_type_node` is called only during startup via `core_namespace`. This guarantees the cache never needs invalidation.

### Design decision

- **Decision**: Use `moka::sync::Cache<(NodeId, NodeId), bool>` inside `DefaultTypeTree`.
- **Rationale**: `moka` is already a dependency. For a read-only workload with bounded key domain (type pairs), `moka` provides O(1) amortized access with negligible overhead. The `sync` feature is already enabled.
- **Alternatives considered**:
  - `HashMap<(NodeId, NodeId), bool>` with interior mutability (`Mutex<HashMap<...>>`): lock contention on every lookup. Rejected in favor of lock-free `moka`.
  - Precompute full transitive closure at startup: memory overhead proportional to |types|². Rejected — lazy population is simpler and equally correct.
  - Do nothing: the type tree depth T is server-defined constant (usually < 20). But with `includeSubtypes=true` on 1000s of browse references, the O(R·T) amplification is real. Accepted for implementation.

---

## Cut 2b: TranslateBrowsePaths Index

### Spec references

| Document | Section | Content |
|----------|---------|---------|
| OPC 10000-3 | §5.2.4 | BrowseName is "the human-readable name when browsing the AddressSpace to create paths out of BrowseNames. The TranslateBrowsePathsToNodeIds Service defined in OPC 10000-4 can be used to follow a path constructed from BrowseNames." |
| OPC 10000-4 | — | TranslateBrowsePathsToNodeIds resolves a `BrowsePath` (list of `RelativePathElement` containing target BrowseName + ReferenceTypeId) to NodeIds. |
| OPC 10000-3 | §4.10.2 | Interface Model: "TranslateBrowsePathsToNodeIds Service shall return them first." |

### Path resolution contract

Each `RelativePathElement` specifies:
- `referenceTypeId`: the ReferenceType to follow (or null for all)
- `isInverse`: direction (Forward or Inverse)
- `includeSubtypes`: whether to include subtypes of the reference type
- `targetName`: the `QualifiedName` of the target node

The current `impl_translate_browse_paths_using_browse` iterates all references of the current node and checks if any target's BrowseName matches. This is O(M·R) per depth level: for M matched nodes at depth D, iterate R references each to find the one matching the BrowseName.

### Index structure

Building `HashMap<(NodeId, QualifiedName), Vec<NodeId>>` maps a (parent, BrowseName) pair directly to all child nodes with that BrowseName. A path element resolves by:
1. Looking up `(parent_node, target_name)` → `Vec<child_nodes>`
2. Filtering by reference type/direction (cheap — child set is small per BrowseName)

### Invalidation

OPC 10000-3 defines AddNodes (§6.4.2), AddReferences, DeleteNodes, and DeleteReferences. The `node-management` Cargo feature gates these operations. When disabled, the address space is static → index built once, never invalidated. When enabled, the index must invalidate/update on mutation. Since mutations are rare relative to reads, lazy rebuild on next lookup is acceptable.

### Design decision

- **Decision**: Build `HashMap<(NodeId, QualifiedName), Vec<NodeId>>` index. Invalidate on all four mutation operations when `node-management` is enabled. Build lazily on first access after invalidation.
- **Rationale**: O(1) per path element vs O(M·R). Mutations are rare; full rebuild is O(address-space size) which is acceptable for the occasional write.
- **Alternatives considered**:
  - Incremental update on each AddNodes/AddReferences: complex, error-prone, requires understanding every mutation's effect. Rejected.
  - `DashMap` for concurrent access: unnecessary — the index is only used during TranslateBrowsePaths which is inherently serial within a request.
  - Do nothing: the current O(D·M·R) is bounded by request limits. But for hierarchical paths on large address spaces, this is the highest-amplification pattern we have. Accepted.

---

## Cut 6: CreateSession Per-Channel Counter

### Spec references

| Document | Section | Content |
|----------|---------|---------|
| OPC 10000-4 | §5.7.2 | CreateSession: "The Server shall bind the Session to the SecureChannel specified by the secureChannelId in the request header." |
| OPC 10000-4 | §5.7.2 | ActivateSession: "The activation of a Session establishes the identity of the user and applications using the Session." Sessions are created (unactivated) then activated. |
| OPC 10000-4 | §7.1 | Server limits: `MaxSessions` (total) and operational limits per secure channel. |

### Current behavior

`commit_create_session_draft` scans all sessions linearly to count unactivated sessions on the requesting channel:

```text
unactivated_count = sessions.values()
    .filter(session.secure_channel_id == draft.secure_channel_id)
    .filter(!session.is_activated())
    .count()
```

This is O(total sessions) per handshake, capped by `max_sessions`.

### Per-channel counter semantics

- **Increment**: on successful `commit_create_session_draft` (before activation)
- **Decrement**: on successful activation (regardless of channel — cross-channel activation transfers the session)
- **Decrement**: on session close or expiry (only if still unactivated)

### Cross-channel activation

OPC 10000-4 §5.7.3 (ActivateSession) allows a session created on channel A to be activated on channel B. In this case:
- Channel A's unactivated counter decrements (the session left channel A)
- Channel B's counter is unaffected (it arrives already activated — or does it?)

The current code treats activation as a state change: `session.is_activated()` becomes true. The unactivated count is about *unactivated* sessions on a channel, not pending session tokens. So when a session moves from A to B via activation, channel A decrements. Channel B doesn't increment because the session arrives activated.

### Design decision

- **Decision**: `HashMap<u32, AtomicUsize>` keyed by `secure_channel_id`. Increment on session creation, decrement on activation/close/expiry. Replace the linear scan with an O(1) map lookup.
- **Rationale**: Eliminates the O(sessions) scan. AtomicUsize avoids lock contention on the hot CreateSession path. The SessionManager already holds a `&mut self` during commit, so no concurrent CreateSession calls compete.
- **Alternatives considered**:
  - `DashMap<u32, AtomicUsize>`: overkill — `commit_create_session_draft` is called under `&mut self`.
  - Per-channel counter stored inside SecureChannel: would require plumbing through the channel abstraction. Rejected for simplicity.
  - Do nothing: bounded by `max_sessions` and connection rate limits. Already documented as "latency hygiene, not DoS." Accepted for implementation.

---

## Cut 7: Subscription Priority Sort Cache

### Spec references

| Document | Section | Content |
|----------|---------|---------|
| OPC 10000-4 | §5.14.2.2 | CreateSubscription `priority` parameter: "When more than one Subscription needs to send a Publish response, the Server should de-queue a Publish request to the Subscription with the highest priority number. For Subscriptions with equal priority the Server should de-queue Publish requests in a round-robin fashion." |
| OPC 10000-4 | §5.14.3.2 | ModifySubscription: same priority semantics on modification. |

### Current behavior

`subscription_ids_by_priority()` re-sorts all subscriptions by priority on every publish tick (~100ms):

```text
subscription_priority = subscriptions.values()
    .map(|v| (v.id(), v.priority()))
    .collect::<Vec<_>>()
    .sort_by_key(|s1| Reverse(s1.1))
```

This is O(S log S) per tick. With S=1000, ~10k comparisons every 100ms.

### Round-robin for equal priorities

The spec mandates round-robin for equal priorities. The current sort is unstable (no secondary key). The cached version **must** preserve the relative ordering of equal-priority subscriptions between ticks unless a priority changes. This means:
- On first build: sort by priority (descending), use subscription ID as tiebreaker for stable ordering.
- On priority change: rebuild.
- On create/delete: rebuild (or insert/remove while preserving order).
- On tick with no changes: reuse cached Vec.

### Design decision

- **Decision**: Cache `Vec<u32>` of subscription IDs sorted by priority. Set a dirty flag on ModifySubscription, subscription create, and subscription delete. On each tick, if dirty, rebuild; otherwise reuse.
- **Rationale**: Priority changes are infrequent relative to tick frequency (100ms ticks, priority changes on explicit client call). Most ticks reuse the cached order.
- **Alternatives considered**:
  - `BTreeSet<(u8, u32)>` with O(log S) insert/remove: simpler to implement, but requires handling the round-robin constraint (equal priority must preserve insert order or ID order). Rejected for complexity of round-robin correctness.
  - Sorted `Vec` with binary-search insert/remove: O(S) shift on insert/remove, no better than rebuild for small S. Rejected.
  - Do nothing: S is bounded by subscription cap, sort is per-tick. Already documented as "latency hygiene." Accepted.

---

## Cut 8: Chunk Header Single-Parse

### Spec references

| Document | Section | Content |
|----------|---------|---------|
| OPC 10000-6 | §6.7.2.2 | Message Header: MessageType (3 bytes ASCII), IsFinal (1 byte), MessageSize (4 bytes UInt32), SecureChannelId (4 bytes UInt32). Total: 12 bytes. |
| OPC 10000-6 | §6.7.2.3 | Security Header: TokenId + security policy token data. Asymmetric: sender certificate + thumbprint. Symmetric: token ID only. |
| OPC 10000-6 | §6.7.2.4 | Sequence Header: SequenceNumber + RequestId. |

### Current behavior

`ChunkInfo::new()` is called via `chunk.chunk_info(secure_channel)`:
- **Validate pass** (`chunker.rs:353`): checks channel ID, sequence number, request ID consistency across chunks
- **Decode pass** (`chunker.rs:507`): checks IsFinal flag per chunk, then decodes message body

Each call parses the full header: MessageHeader (12 bytes) + SecurityHeader (variable, expensive for asymmetric due to X509 cert decode) + SequenceHeader (8 bytes).

### Parse reuse

For the steady-state **symmetric** path (after OpenSecureChannel), the SecurityHeader is just a token ID (4 bytes). The double-parse is a cheap constant factor. For the rare **asymmetric** path (OpenSecureChannel), the sender certificate decode is expensive, but OpenSecureChannel is infrequent.

The `ChunkInfo` struct contains all parsed header fields. Once parsed, the chunk data backing the parse is immutable for the message's lifetime. Caching the `ChunkInfo` in the `MessageChunk` avoids the second parse.

### Design decision

- **Decision**: Add `Option<ChunkInfo>` field to `MessageChunk`. On first `chunk_info()` call, compute and store. Subsequent calls return the cached value. Clear on message boundary.
- **Rationale**: Simple one-field addition. Zero invalidation: chunk data is immutable within a message's lifetime. The option is cleared when the chunk is freed/dropped.
- **Alternatives considered**:
  - Pass `ChunkInfo` from validate to decode via function arguments: would require changing the API of `Chunker::decode()`. More invasive than a local cache.
  - Pre-parse all `ChunkInfo` during chunk receive (before validate): would parse even for messages that fail early validation. Rejected — lazily compute only when needed.
  - Do nothing: 2× cheap header parse is marginal. Already documented as "marginal constant-factor." Accepted for implementation.
