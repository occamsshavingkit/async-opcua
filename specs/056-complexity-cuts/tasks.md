# Tasks: Complexity Cuts (2a, 2b, 6, 7, 8)

**Input**: Design documents from `/specs/056-complexity-cuts/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: No new tests required — all cuts are verified by the existing test suite (FR compliance: SC-006).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

**Story mapping**: US1 = Cut 2a (`is_subtype_of`), US2 = Cut 2b (TranslateBrowsePaths), US3 = Cut 6 (CreateSession), US4 = Cut 7 (subscription priority), US5 = Cut 8 (chunk header)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Dependency)

**Purpose**: Add `moka` dependency to `async-opcua-nodes` crate (already present in `async-opcua-server`)

- [x] T001 Add `moka` to `async-opcua-nodes/Cargo.toml` with `sync` feature, matching the `async-opcua-server` version (`0.12`, features: `["sync"]`)

---

## Phase 2: Foundational (Pre-Flight Baseline)

**Purpose**: Verify clean CI baseline before any code changes. Each command below is a single verification step — all must pass.

**CRITICAL**: All cuts must start from a green baseline.

- [x] T002 Run `cargo fmt --all -- --check` to verify formatting compliance
- [x] T003 Run `cargo clippy --workspace --all-targets --all-features` to verify no lint warnings
- [x] T004 Run `RUSTFLAGS="-D warnings" cargo check --workspace` to verify all crates compile warning-free
- [x] T005 Run `cargo test -p async-opcua-nodes --lib && cargo test -p async-opcua-server --lib && cargo test -p async-opcua-core --lib` to verify all existing tests pass

**Checkpoint**: Baseline green — user story implementation can now begin independently

---

## Phase 3: User Story 1 — `is_subtype_of` Memoization (Priority: P1) 🎯 MVP

**Goal**: Cache `(parent, child) → bool` in `moka::sync::Cache` within `DefaultTypeTree` so repeated `is_subtype_of` calls for the same type pair return in O(1) without re-walking the HasSubtype chain (OPC 10000-3 §4.10.1 single-inheritance, §5.3.3.3 HasSubtype walk, §7.10 HasSubtype ReferenceType). Type tree is immutable after startup — no cache invalidation required. (FR-2a-001 through FR-2a-005)

**Independent Test**: Run `cargo test -p async-opcua-nodes --lib` — all existing type-tree and browse tests pass without modification. A manual check: `is_subtype_of(A, B)` called twice uses cache on second call.

### Implementation for User Story 1

- [x] T006 [US1] Add `subtype_cache: moka::sync::Cache<(NodeId, NodeId), bool>` field to `DefaultTypeTree` struct in `async-opcua-nodes/src/type_tree.rs`
- [x] T007 [US1] Initialize `subtype_cache` with unbounded capacity in `DefaultTypeTree::new()` in `async-opcua-nodes/src/type_tree.rs`
- [x] T008 [US1] Rewrite `is_subtype_of()` body in `async-opcua-nodes/src/type_tree.rs` to be a single fused algorithm: check `self.subtype_cache.get(&(child.clone(), ancestor.clone()))` on entry — on hit return cached value; on miss walk the HasSubtype chain (per OPC 10000-3 §5.3.3.3), store result in cache, then return. Includes handling `(A, A) → true` identity case.
- [x] T009 [US1] Run `cargo test -p async-opcua-nodes --lib` to verify all existing tests pass with cached `is_subtype_of`

**Checkpoint**: Cut 2a complete — `is_subtype_of` memoized, all type-tree tests green.

---

## Phase 4: User Story 2 — TranslateBrowsePaths Index (Priority: P2)

**Goal**: Build `HashMap<(NodeId, QualifiedName), Vec<NodeId>>` index mapping `(parent, BrowseName)` → `[child]` for O(1) path element resolution per OPC 10000-3 §5.2.4 (BrowseName path resolution) and the TranslateBrowsePathsToNodeIds service (Part 4). When `node-management` feature is enabled, invalidate on AddNodes/AddReferences/DeleteNodes/DeleteReferences. When disabled, build once at init. Post-lookup reference-type filtering handles `isInverse` and `includeSubtypes`. (FR-2b-001 through FR-2b-005)

**Independent Test**: Run `cargo test -p async-opcua-server --lib` — all TranslateBrowsePaths tests pass.

### Implementation for User Story 2

- [x] T010 [US2] Add `browse_name_index: Option<HashMap<(NodeId, QualifiedName), Vec<NodeId>>>` field to the address space struct (where node references are stored) in `async-opcua-server/src/node_manager/memory/mod.rs`
- [x] T011 [US2] Implement a `build_browse_name_index(&self) -> HashMap<...>` method that iterates all nodes in the address space, collecting each node's forward references and inserting `(source_node_id, target_browse_name) → target_node_id` entries. In `async-opcua-server/src/node_manager/memory/mod.rs`
- [x] T012 [US2] In `impl_translate_browse_paths_using_browse` in `async-opcua-server/src/node_manager/view.rs`: before the path-resolution loop, rebuild the index from the address space if `browse_name_index` is `None`
- [x] T013 [US2] In the same function (`impl_translate_browse_paths_using_browse`): inside the path-resolution loop, replace the `browse → iterate-references → filter-by-BrowseName` pattern with a single `self.browse_name_index.get(&(parent_id, browse_name))` lookup, followed by post-lookup filtering for reference-type direction and subtype inclusion per OPC 10000-3 §5.2.4. In `async-opcua-server/src/node_manager/view.rs`
- [x] T014 [US2] Add index invalidation: in each address-space mutation method (add_node, add_reference, delete_node, delete_reference), gate on `#[cfg(feature = "node-management")]` and set `self.browse_name_index = None`. In `async-opcua-server/src/node_manager/memory/mod.rs`
- [x] T015 [US2] When `node-management` feature is disabled, build the index once in `finish_loading_address_space` (or equivalent init method) so it is always `Some` after startup. In `async-opcua-server/src/node_manager/memory/mod.rs`
- [x] T016 [US2] Run `cargo test -p async-opcua-server --lib` to verify all TranslateBrowsePaths and node-management tests pass

**Checkpoint**: Cut 2b complete — TranslateBrowsePaths resolves via index, all server tests green.

---

## Phase 5: User Story 3 — CreateSession Per-Channel Counter (Priority: P3)

**Goal**: Replace O(sessions) linear scan in `commit_create_session_draft` with `HashMap<u32, AtomicUsize>` per-secure-channel counter of unactivated sessions per OPC 10000-4 §5.7.2 (session-channel binding). Increment on creation, decrement on activation (including cross-channel per OPC 10000-4 §5.7.3 ActivateSession), close, and expiry. Enforce `max_unactivated_sessions_per_channel` identically. (FR-6-001 through FR-6-005)

**Independent Test**: Run `cargo test -p async-opcua-server --lib` — all CreateSession, ActivateSession, CloseSession tests pass.

### Implementation for User Story 3

- [x] T017 [US3] Add `unactivated_by_channel: HashMap<u32, AtomicUsize>` field to `SessionManager` struct in `async-opcua-server/src/session/manager.rs`
- [x] T018 [US3] In `commit_create_session_draft`, replace the `sessions.values().filter().count()` linear scan with `self.unactivated_by_channel.entry(draft.secure_channel_id).or_default().load(Ordering::Acquire)` lookup. After successful session insertion, `fetch_add(1, Ordering::Release)`. In `async-opcua-server/src/session/manager.rs`
- [x] T019 [US3] In `activate_session`, after successful activation, decrement the old channel's counter via `fetch_sub(1, Ordering::Release)` if the session was previously unactivated. Handles cross-channel activation per OPC 10000-4 §5.7.3. In `async-opcua-server/src/session/manager.rs`
- [x] T020 [US3] In session close and expiry paths, if the session is not yet activated, decrement its creation channel's counter via `fetch_sub(1, Ordering::Release)`. In `async-opcua-server/src/session/manager.rs`
- [x] T021 [US3] Run `cargo test -p async-opcua-server --lib` to verify all session lifecycle tests pass

**Checkpoint**: Cut 6 complete — CreateSession counting O(1), all session tests green.

---

## Phase 6: User Story 4 — Subscription Priority Cache (Priority: P4)

**Goal**: Cache the priority-sorted subscription ID list with a dirty flag per OPC 10000-4 §5.14.2.2 (highest-priority-first, round-robin for equals). Re-sort only on subscription create, delete, or priority change. Preserve stable ordering for equal priorities using subscription ID as tiebreaker. (FR-7-001 through FR-7-004)

**Independent Test**: Run `cargo test -p async-opcua-server --lib` — all subscription and publish tests pass.

### Implementation for User Story 4

- [ ] T022 [US4] Add `cached_priority_order: Vec<u32>` and `priority_cache_dirty: bool` fields to the session subscriptions struct in `async-opcua-server/src/subscriptions/session_subscriptions.rs`
- [ ] T023 [US4] Modify `subscription_ids_by_priority()` in `async-opcua-server/src/subscriptions/session_subscriptions.rs` to: if `priority_cache_dirty`, rebuild by collecting `(id, priority)` from active subscriptions only (exclude `transferring`), sort by `(priority descending, id ascending)` per OPC 10000-4 §5.14.2.2 round-robin requirement, store in `cached_priority_order`, clear dirty flag. On clean cache, return clone of `cached_priority_order`
- [ ] T024 [P] [US4] Set `priority_cache_dirty = true` in the subscription creation handler in `async-opcua-server/src/subscriptions/session_subscriptions.rs`
- [ ] T025 [P] [US4] Set `priority_cache_dirty = true` in the subscription deletion handler in `async-opcua-server/src/subscriptions/session_subscriptions.rs`
- [ ] T026 [P] [US4] Set `priority_cache_dirty = true` in the ModifySubscription handler (when the priority field is present in the request) in `async-opcua-server/src/subscriptions/session_subscriptions.rs`
- [ ] T027 [US4] Run `cargo test -p async-opcua-server --lib` to verify all subscription and publish tests pass

**Checkpoint**: Cut 7 complete — subscription priority cached, all subscription tests green.

---

## Phase 7: User Story 5 — Chunk Header Single-Parse (Priority: P5)

**Goal**: Cache `ChunkInfo` in `MessageChunk` so it is parsed once and reused across validate and decode passes per OPC 10000-6 §6.7.2.2 (MessageHeader 12 bytes) + SecurityHeader + SequenceHeader. Chunk data is immutable within a message lifetime — no invalidation. (FR-8-001 through FR-8-004)

**Independent Test**: Run `cargo test -p async-opcua-core --lib` — all chunker and message encode/decode tests pass.

### Implementation for User Story 5

- [ ] T028 [US5] Add `cached_chunk_info: Option<ChunkInfo>` field to `MessageChunk` struct in `async-opcua-core/src/comms/message_chunk.rs`
- [ ] T029 [US5] Modify `chunk_info()` in `async-opcua-core/src/comms/message_chunk.rs` to accept `&mut self` instead of `&self`. On call: if `cached_chunk_info` is `None`, compute and store; return `Ok(cached_chunk_info.as_ref().unwrap())`. Chunk data (self.data) is immutable per OPC 10000-6 §6.7.2.2 header format
- [ ] T030 [US5] Update callers in `async-opcua-core/src/comms/chunker.rs`: in `validate_chunk_sequence` (line ~353) and `decode<T>` (line ~507), change `chunk.chunk_info(secure_channel)?` calls to use `&mut chunk` access pattern (iterate with mutable reference or index)
- [ ] T031 [US5] Run `cargo test -p async-opcua-core --lib` to verify all chunker and message decode tests pass

**Checkpoint**: Cut 8 complete — ChunkInfo parsed once per message, all core tests green.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Full workspace verification — all five cuts integrated. No new heap allocations on hot path (SC-007) is verified implicitly by existing tests passing without allocation regressions.

- [ ] T032 Run `cargo fmt --all -- --check` to verify formatting across workspace
- [ ] T033 Run `cargo clippy --workspace --all-targets --all-features` to verify no new lint warnings
- [ ] T034 Run `RUSTFLAGS="-D warnings" cargo check --workspace` to verify all crates compile warning-free
- [ ] T035 Run `cargo test -p async-opcua-nodes --lib && cargo test -p async-opcua-server --lib && cargo test -p async-opcua-core --lib` to verify all existing tests pass for all five cuts (SC-006, SC-007)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 — no dependencies
- **Foundational (Phase 2)**: T002–T005 — depends on T001 (moka dep for check) — BLOCKS all user stories
- **US1 (Phase 3)**: T006–T009 — depends on Phase 2 baseline
- **US2 (Phase 4)**: T010–T016 — depends on Phase 2 baseline; independent of US1
- **US3 (Phase 5)**: T017–T021 — depends on Phase 2 baseline; independent of US1, US2
- **US4 (Phase 6)**: T022–T027 — depends on Phase 2 baseline; independent of US1–US3
- **US5 (Phase 7)**: T028–T031 — depends on Phase 2 baseline; independent of US1–US4
- **Polish (Phase 8)**: T032–T035 — depends on all user stories

### User Story Dependencies

All five user stories are **fully independent** — none depends on another. They touch different crates or different modules within the same crate:

- **US1 (Cut 2a)**: `async-opcua-nodes/src/type_tree.rs` only
- **US2 (Cut 2b)**: `async-opcua-server/src/node_manager/` only
- **US3 (Cut 6)**: `async-opcua-server/src/session/manager.rs` only
- **US4 (Cut 7)**: `async-opcua-server/src/subscriptions/session_subscriptions.rs` only
- **US5 (Cut 8)**: `async-opcua-core/src/comms/` only

### Within Each User Story

- Field addition before method modification
- Within US2: T012 (pre-loop rebuild) before T013 (in-loop lookup) — both modify same function
- Within US4: T024, T025, T026 are [P] (touch different code paths in same file — create/delete/modify handlers)
- Core implementation before verification
- Story complete (tests green) before moving to next

### Parallel Opportunities

- All five user stories (Phase 3–7) can be implemented in parallel by different developers
- Within US4: T024, T025, T026 (dirty flag in 3 handlers) are all [P] — can be done simultaneously
- US1, US2, US3, US4, US5 all touch different files — zero merge conflict risk

---

## Parallel Example: All Five User Stories

```bash
# Launch all five independently (different crates/modules):
Task: "T006-T009: US1 is_subtype_of memoization in async-opcua-nodes"
Task: "T010-T016: US2 TranslateBrowsePaths index in async-opcua-server/node_manager"
Task: "T017-T021: US3 per-channel counter in async-opcua-server/session"
Task: "T022-T027: US4 priority cache in async-opcua-server/subscriptions"
Task: "T028-T031: US5 chunk header single-parse in async-opcua-core/comms"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: T001 (add moka dep)
2. Complete Phase 2: T002–T005 (baseline verification)
3. Complete Phase 3: T006–T009 (is_subtype_of memoization)
4. **STOP and VALIDATE**: `cargo test -p async-opcua-nodes --lib` green
5. Commit Cut 2a as a single commit

### Incremental Delivery (Sequential)

1. T001 → T002–T005 → T006–T009 → **US1 done** (MVP!)
2. T010–T016 → **US2 done** (no dependency on US1)
3. T017–T021 → **US3 done**
4. T022–T027 → **US4 done**
5. T028–T031 → **US5 done**
6. T032–T035 → Full workspace verification → **All five cuts done**

### Parallel Team Strategy

With multiple agents:

1. One agent completes Phase 1 + 2 (baseline)
2. Once baseline green, five agents take US1–US5 in parallel
3. Final agent runs Phase 8 polish

---

## Notes

- [P] tasks = different files or independent code paths, no dependencies — these ARE parallelizable
- All five user stories are independent and touch different files
- No new tests required — behavioral preservation verified by existing tests (SC-006)
- SC-007 (no new heap allocations on hot path) is verified implicitly: existing tests pass with no allocation regression
- Each cut is one commit per the Constitution (Principle III: Individual Task Discipline)
- Pre-flight CI: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features && RUSTFLAGS="-D warnings" cargo check --workspace`
