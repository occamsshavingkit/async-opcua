# Feature Specification: Complexity Cuts (2a, 2b, 6, 7, 8)

**Feature Branch**: `056-complexity-cuts`  
**Created**: 2026-07-03  
**Status**: Draft  
**Input**: Five targeted complexity reductions in the OPC-UA server hot paths: type-subtype memoization, TranslateBrowsePaths indexing, per-channel CreateSession counting, subscription priority caching, and chunk header re-parse elimination.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Browse type filtering at scale (Priority: P1)

Operators monitoring large address spaces with hundreds of sessions use Browse with type-filtering (nodeClassMask + referenceTypeId with includeSubtypes=true). Each browse reference evaluation walks the type tree to check `is_subtype_of`, producing O(R·T) work per request. Under heavy browse load (hundreds of concurrent clients), this repeated linear walk causes latency amplification.

**Why this priority**: Browse is the most frequent OPC-UA service request. Type-filtering is a standard client pattern (filter by ObjectType, VariableType, or custom subtypes). The type tree is immutable after startup, making this the safest and highest-impact caching opportunity.

**Independent Test**: Run Browse with `includeSubtypes=true` on a type-filtered address space under load. Measure that repeated subtype checks for the same (parent, child) pair do not re-walk the type tree. Server tests pass green before and after.

**Acceptance Scenarios**:

1. **Given** a server with a type tree loaded at startup, **When** `is_subtype_of(BaseAnalogType, BaseDataVariableType)` is called twice for the same type pair during separate Browse requests, **Then** the second invocation returns from cache without walking the type hierarchy.
2. **Given** a type tree loaded at startup, **When** a Browse request evaluates 10,000 references with type filtering, **Then** repeated type checks across those references hit the cache and complete without linear type-tree walks.
3. **Given** the cache is populated during normal operation, **When** a new type is queried that is not in the cache, **Then** the result is computed by walking the type tree and stored in the cache for subsequent queries.
4. **Given** the cache is active, **When** an existing server test suite runs, **Then** all existing tests pass without modification (behavioral no-op).

---

### User Story 2 - TranslateBrowsePaths latency reduction (Priority: P2)

Clients frequently call TranslateBrowsePathsToNodeIds to resolve human-readable browse paths to NodeIds (e.g., monitoring a specific sensor hierarchy). The current `impl_translate_browse_paths_using_browse` performs a nested scan: for each path depth D, for each currently matched node M, it iterates all references R to find the one matching the next BrowseName. This O(D·M·R) pattern amplifies on large address spaces with deep hierarchies.

**Why this priority**: TranslateBrowsePaths is a foundational service used by every OPC-UA client to resolve browse paths. An index by (parent NodeId, BrowseName) makes each path step O(1) instead of O(M·R), directly reducing client path resolution latency. Invalidation is needed only when the address space mutates (AddNodes/AddReferences/DeleteNodes/DeleteReferences), which is gated behind the `node-management` feature.

**Independent Test**: Build a server with a 10,000-node address space and a 5-element hierarchical browse path. Call TranslateBrowsePathsToNodeIds. Verify that the path resolves in constant time per depth step rather than scanning the full reference set per depth. All existing TranslateBrowsePaths tests pass.

**Acceptance Scenarios**:

1. **Given** a server address space with 10,000 nodes and a hierarchical browse path of depth 5, **When** TranslateBrowsePathsToNodeIds is called, **Then** the path resolves with O(D) node-manager calls (D = path depth), not O(D·M·R).
2. **Given** the (parent, BrowseName) index is populated, **When** an AddNodes or AddReferences call succeeds, **Then** the index is invalidated and rebuilt on next access (or updated incrementally).
3. **Given** the `node-management` feature is disabled, **When** the server runs, **Then** the index is built once at load time and never invalidated.
4. **Given** the index is active, **When** the existing TranslateBrowsePaths test suite runs, **Then** all tests pass green.

---

### User Story 3 - CreateSession rate resilience (Priority: P3)

When many clients attempt to create sessions concurrently over the same secure channel (e.g., during a reconnection storm), the server scans _all_ sessions to count unactivated sessions per channel. With `max_sessions` set to a large value (e.g., 5000), this O(sessions) scan amplifies handshake latency even though the per-channel limit is small.

**Why this priority**: The scan is bounded by the session cap and handshake rate is bounded by connection limits, so this is latency hygiene, not a DoS fix. However, during reconnection storms the O(sessions) scan on every CreateSession call adds unnecessary handshake latency. A per-channel counter reduces it to O(1).

**Independent Test**: Create 1000 sessions across 10 secure channels with varying activation states. Call CreateSession on each channel. Verify that the unactivated-count check completes in O(1) with no linear scan across all sessions. Existing CreateSession tests pass.

**Acceptance Scenarios**:

1. **Given** 1000 sessions across 10 secure channels, **When** CreateSession is called on channel A, **Then** the unactivated-sessions-per-channel check does not iterate over sessions belonging to other channels.
2. **Given** a session is created on a channel, **When** that session is activated, **Then** the per-channel unactivated counter decrements.
3. **Given** a session is closed or expires, **When** the session is removed, **Then** the per-channel counter decrements if the session was unactivated.
4. **Given** the counter is active, **When** existing CreateSession, ActivateSession, and CloseSession tests run, **Then** all tests pass green.

---

### User Story 4 - Subscription publish tick efficiency (Priority: P4)

Every 100ms publish tick, the server re-sorts all subscriptions by priority to determine the order in which they should receive publish responses. With 1000 subscriptions, this is ~10,000 comparisons per tick — wasted work when priorities rarely change between ticks.

**Why this priority**: Sorting is bounded (capped subscription count) and per-tick (not per-message), making this low-risk latency hygiene. However, subscriptions change priority only on ModifySubscription, so caching the sorted order and recomputing only on change eliminates unnecessary work at scale.

**Independent Test**: Create 500 subscriptions with varying priorities. Trigger multiple publish ticks without modifying any subscription. Verify that the priority-ordered subscription list is reused across ticks without re-sorting. Subscription-related tests pass.

**Acceptance Scenarios**:

1. **Given** 500 subscriptions with stable priorities, **When** two consecutive publish ticks fire without any subscription modification, **Then** the subscription processing order is reused from the cached sorted list without re-sorting.
2. **Given** a subscription priority is modified via ModifySubscription, **When** the next publish tick fires, **Then** the cached sort order is recomputed to reflect the new priority.
3. **Given** a subscription is created or deleted, **When** the next publish tick fires, **Then** the cached sort order is updated.
4. **Given** the priority cache is active, **When** existing subscription and publish tests run, **Then** all tests pass green.

---

### User Story 5 - Chunk header single-parse path (Priority: P5)

Each received message chunk decodes its `ChunkInfo` (header, security header, sequence header) once during the validate pass and again during the decode pass. This double-parse is a constant-factor overhead that wastes CPU cycles on the hot receive path.

**Why this priority**: Chunk headers are cheap to parse (few dozen bytes), making this a marginal constant-factor improvement. But chunk data is immutable for the lifetime of a single message, making this the lowest-risk cut in the set with no invalidation concerns.

**Independent Test**: Send a multi-chunk message to the server. Verify that `ChunkInfo` is computed exactly once per chunk across both validation and decoding passes. Existing chunker and message tests pass.

**Acceptance Scenarios**:

1. **Given** a message composed of N chunks, **When** the message is received and decoded, **Then** each chunk's `ChunkInfo` is parsed exactly once and reused across the validation and decoding phases.
2. **Given** `ChunkInfo` is cached per chunk, **When** the decode pass runs, **Then** it accesses the cached `ChunkInfo` rather than re-parsing the raw chunk bytes.
3. **Given** the single-parse path is active, **When** existing message encode/decode tests run, **Then** all tests pass green.

---

### Edge Cases

- **Cut 2a**: A type pair `(A, A)` (same node) returns `true` from `is_subtype_of` — the cache must handle this trivial case correctly.
- **Cut 2a**: The type tree is populated during server startup and is never modified at runtime. No cache invalidation is needed, but the cache must be per-`TypeTree` instance to avoid cross-instance contamination.
- **Cut 2b**: When the `node-management` feature is enabled and the address space mutates, stale index entries could return incorrect results. The index must either update incrementally on each mutation or be invalidated and lazily rebuilt.
- **Cut 2b**: BrowsePaths with `is_inverse=true` or `includeSubtypes=false` must still resolve correctly against the index.
- **Cut 6**: The per-channel counter must account for sessions that are created on one channel but activated on another (cross-channel activation). The unactivated count should be tracked per channel and decremented on activation regardless of which channel the activation occurs on.
- **Cut 6**: Session closure and expiry must reliably decrement the counter even in error paths.
- **Cut 7**: Subscriptions with equal priority must maintain a stable ordering across ticks to avoid unnecessary notification reordering.
- **Cut 7**: The priority cache must handle rapid create/delete cycles without accumulating stale entries.
- **Cut 8**: The chunk re-parse optimization must not keep references to chunk data beyond the message's lifetime — no dangling pointers to freed buffers.
- **Cut 8**: Message types that are not in the final position (OPEN SecureChannel asymmetric messages) must still be handled correctly when `ChunkInfo` is reused.

## Requirements *(mandatory)*

### Functional Requirements

#### Cut 2a — `is_subtype_of` memoization

- **FR-2a-001**: The system MUST cache `is_subtype_of(parent, child)` results such that repeated queries for the same type pair return in O(1) without re-walking the type hierarchy.
- **FR-2a-002**: The cache MUST be populated lazily — results are computed on first query and stored for subsequent queries.
- **FR-2a-003**: The cache MUST be per-`TypeTree` instance and live for the duration of that instance's lifetime.
- **FR-2a-004**: The cache MUST NOT require invalidation. The type tree is immutable after startup.
- **FR-2a-005**: Existing `is_subtype_of` callers (Browse filtering, data type validation, reference type filtering) MUST produce identical boolean results before and after caching.

#### Cut 2b — TranslateBrowsePaths index

- **FR-2b-001**: The system MUST maintain a `(parent NodeId, BrowseName) -> Set of child NodeId` index for O(1) path element resolution in TranslateBrowsePaths.
- **FR-2b-002**: When the `node-management` feature is enabled, the index MUST be invalidated or updated on AddNodes, AddReferences, DeleteNodes, and DeleteReferences operations.
- **FR-2b-003**: When the `node-management` feature is disabled, the index MUST be built once during address space initialization and never invalidated.
- **FR-2b-004**: BrowsePath resolution using the index MUST produce the same results as the current O(D·M·R) scan for all valid inputs.
- **FR-2b-005**: The index MUST support BrowsePaths with both Forward and Inverse browse directions, as well as subtype-inclusive and subtype-exclusive reference type filtering.

#### Cut 6 — CreateSession per-channel counter

- **FR-6-001**: The system MUST maintain a per-secure-channel counter of unactivated sessions, replacing the O(sessions) scan in `commit_create_session_draft`.
- **FR-6-002**: The counter MUST increment when a session is created (before activation).
- **FR-6-003**: The counter MUST decrement when a session is activated, regardless of whether activation occurs on the same channel or a different channel.
- **FR-6-004**: The counter MUST decrement when an unactivated session is closed or expires.
- **FR-6-005**: The `max_unactivated_sessions_per_channel` limit MUST be enforced identically to the current linear-scan implementation.

#### Cut 7 — subscription priority sort cache

- **FR-7-001**: The system MUST cache the priority-sorted subscription ID list and recalculate it only when a subscription's priority changes, or a subscription is created or deleted.
- **FR-7-002**: The cache MUST preserve stable ordering for subscriptions with equal priority.
- **FR-7-003**: Each publish tick MUST use the cached ordering when iterating subscriptions to produce publish responses.
- **FR-7-004**: Subscription processing order after caching MUST be identical to the current O(S log S) sort-based order for the same input.

#### Cut 8 — chunk header single-parse

- **FR-8-001**: Each `MessageChunk`'s `ChunkInfo` MUST be parsed at most once during the lifetime of a single message decode.
- **FR-8-002**: The validation pass MUST make the parsed `ChunkInfo` available to the subsequent decode pass without re-parsing.
- **FR-8-003**: The parsed `ChunkInfo` MUST NOT outlive the message whose chunks it was parsed from.
- **FR-8-004**: Existing message encoding and decoding behavior MUST be preserved — no semantic change to error handling or chunk validation.

### Key Entities

- **TypeTree cache**: Maps `(NodeId, NodeId)` → `bool` for `is_subtype_of` results. Read-only after population. Bound by the number of distinct type pairs queried during the server's lifetime.
- **BrowsePath index**: Maps `(NodeId, QualifiedName)` → `Vec<NodeId>` for TranslateBrowsePaths resolution. Keyed by parent node and BrowseName of the target. Invalidated on address-space mutations when `node-management` is enabled.
- **Per-channel session counter**: Maps `u32` (secure channel ID) → count of unactivated sessions on that channel. Updated atomically on create, activate, close, and expiry.
- **Priority cache**: An ordered collection of subscription IDs, sorted by priority (highest first). Rebuilt only when a subscription's priority changes, or a subscription is added/removed.
- **Chunk metadata**: `ChunkInfo` parsed once per `MessageChunk` and carried through the validate→decode pipeline within a single message's processing lifetime.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `is_subtype_of` repeated calls for the same type pair within the server's lifetime do not re-walk the type tree; each unique pair walks at most once.
- **SC-002**: TranslateBrowsePathsToNodeIds resolves each path element with O(1) index lookups rather than O(M·R) reference scans.
- **SC-003**: CreateSession unactivated-session counting completes in constant time per call, independent of total session count.
- **SC-004**: Subscription publish ticks do not re-sort the subscription list when no subscription priorities have changed.
- **SC-005**: Each received chunk's `ChunkInfo` is parsed exactly once across the validate and decode passes.
- **SC-006**: All existing server tests pass without modification, confirming behavioral preservation for all five cuts.
- **SC-007**: No new heap allocations are introduced on the steady-state hot path for any of the five cuts (pre-allocated or amortized O(1) structures are acceptable).

## Assumptions

- The type tree (`DefaultTypeTree`) is fully populated during server initialization and never modified at runtime. This is verified by the existing codebase which builds the type tree from the core namespace at startup and provides no mutation API.
- The TranslateBrowsePaths index is only needed when `node-management` is enabled. When disabled, the address space is static and the index can be built once.
- The per-channel session counter does not need to handle cross-channel session migration (sessions are created and activated, not moved between channels post-activation). Cross-channel activation already transfers the session binding.
- Subscription priorities change only through explicit ModifySubscription calls and are stable between such calls, making a dirty-flag cache appropriate.
- Chunk data is owned by the `MessageChunk` or its enclosing message structure and remains valid throughout the validate→decode pipeline lifetime.
- The `moka` crate is already a dependency in the workspace (verified in Cargo.toml) and is suitable for the `is_subtype_of` cache.
- All five cuts are independent — each can be implemented, tested, and merged separately without depending on any other cut.
