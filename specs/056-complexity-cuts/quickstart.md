# Quickstart: Complexity Cuts Implementation

**Feature**: 056-complexity-cuts
**Date**: 2026-07-03

## Overview

Five independent complexity reductions, each in a single commit. No cut depends on another. Implement in priority order (P1 → P5), one per commit.

## Pre-flight CI check

```bash
# Verify clean baseline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
RUSTFLAGS="-D warnings" cargo check --workspace
cargo test -p async-opcua-server --lib
```

All cuts must leave these commands green.

---

## Cut 2a: `is_subtype_of` Memoization (P1)

**File**: `async-opcua-nodes/src/type_tree.rs`

1. Add field to `DefaultTypeTree`:
   ```rust
   subtype_cache: moka::sync::Cache<(NodeId, NodeId), bool>,
   ```

2. Initialize in `DefaultTypeTree::new()`:
   ```rust
   subtype_cache: moka::sync::Cache::new(1024), // or unbounded; bounded by unique type pairs
   ```

3. Modify `is_subtype_of()`:
   - On entry, check `self.subtype_cache.get(&(child.clone(), ancestor.clone()))`.
   - On cache hit, return cached value.
   - On cache miss, execute the existing loop, store result in cache, return.

4. Verify: `cargo test -p async-opcua-nodes --lib`

**Why moka instead of HashMap**: `moka` is already a dependency. It provides lock-free concurrent reads. The `sync` feature is already enabled in `async-opcua-server/Cargo.toml`. Add `moka` to `async-opcua-nodes/Cargo.toml` as well.

---

## Cut 2b: TranslateBrowsePaths Index (P2)

**Files**: `async-opcua-server/src/node_manager/view.rs`, `async-opcua-server/src/node_manager/memory/mod.rs`

1. Add field to `AddressSpace` (memory/mod.rs):
   ```rust
   browse_name_index: Option<HashMap<(NodeId, QualifiedName), Vec<NodeId>>>,
   ```

2. Build method: iterate all nodes in address space, for each node's forward references, insert `(source, target_browsename) → target_id` into index.

3. In `impl_translate_browse_paths_using_browse`:
   - Before the `loop { ... }`, if index is `None`, rebuild it.
   - Inside the loop, instead of browse→filter for each node, use the index: `index.get(&(parent_id, browse_name))`.
   - Apply reference type filtering and direction post-lookup.

4. Invalidation hooks (when `feature = "node-management"`):
   - In `add_node`, `add_reference`, `delete_node`, `delete_reference`: set `self.browse_name_index = None`.

5. When `node-management` is disabled: build the index once in `finish_loading_address_space` or equivalent. Never invalidate.

6. Verify: `cargo test -p async-opcua-server --lib`

---

## Cut 6: CreateSession Per-Channel Counter (P3)

**File**: `async-opcua-server/src/session/manager.rs`

1. Add field to `SessionManager`:
   ```rust
   unactivated_by_channel: HashMap<u32, AtomicUsize>,
   ```

2. In `commit_create_session_draft`, replace the linear scan:
   ```rust
   let count = self.unactivated_by_channel
       .entry(draft.secure_channel_id)
       .or_insert_with(|| AtomicUsize::new(0))
       .load(Ordering::Acquire);
   if count >= self.info.config.limits.max_unactivated_sessions_per_channel { ... }
   // After success:
   self.unactivated_by_channel[&draft.secure_channel_id].fetch_add(1, Ordering::Release);
   ```

3. In `activate_session` (after successful activation):
   ```rust
   // Decrement the old channel's count (the session was created there)
   if let Some(counter) = self.unactivated_by_channel.get(&old_channel_id) {
       counter.fetch_sub(1, Ordering::Release);
   }
   ```

4. In `close_session` handler:
   ```rust
   if !session.is_activated() {
       if let Some(counter) = self.unactivated_by_channel.get(&session.secure_channel_id()) {
           counter.fetch_sub(1, Ordering::Release);
       }
   }
   ```

5. In session expiry cleanup (same as close).

6. Verify: `cargo test -p async-opcua-server --lib`

---

## Cut 7: Subscription Priority Cache (P4)

**File**: `async-opcua-server/src/subscriptions/session_subscriptions.rs`

1. Add fields to the session subscriptions struct:
   ```rust
   cached_priority_order: Vec<u32>,
   priority_cache_dirty: bool,
   ```

2. Modify `subscription_ids_by_priority()`:
   ```rust
   fn subscription_ids_by_priority(&mut self) -> Vec<u32> {
       if self.priority_cache_dirty {
           let mut order: Vec<(u32, u8)> = self.subscriptions.values()
               .map(|v| (v.id(), v.priority()))
               .collect();
           order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))); // stable by ID
           self.cached_priority_order = order.into_iter().map(|(id, _)| id).collect();
           self.priority_cache_dirty = false;
       }
       self.cached_priority_order.clone()
   }
   ```

3. Set `priority_cache_dirty = true` in:
   - Subscription creation handler
   - Subscription deletion handler
   - `ModifySubscription` handler (when priority field is present in the request)

4. Note: The cache is cloned on each call. Subsections are typically < 1000, so cloning a `Vec<u32>` is cheap (4KB). The alternative is returning a reference but the caller iterates and may mutate during iteration.

5. Verify: `cargo test -p async-opcua-server --lib`

---

## Cut 8: Chunk Header Single-Parse (P5)

**File**: `async-opcua-core/src/comms/message_chunk.rs`, `async-opcua-core/src/comms/chunker.rs`

1. Add field to `MessageChunk`:
   ```rust
   cached_chunk_info: std::cell::OnceCell<ChunkInfo>,
   ```

   Or simpler — use `Option<ChunkInfo>` since the chunk is processed sequentially (no concurrent access):

   ```rust
   cached_chunk_info: Option<ChunkInfo>,
   ```

2. Modify `chunk_info()`:
   ```rust
   pub fn chunk_info(&mut self, secure_channel: &SecureChannel) -> EncodingResult<&ChunkInfo> {
       if self.cached_chunk_info.is_none() {
           self.cached_chunk_info = Some(ChunkInfo::new(self, secure_channel)?);
       }
       Ok(self.cached_chunk_info.as_ref().unwrap())
   }
   ```

3. Update callers in `chunker.rs`:
   - `validate_chunk_sequence`: change `chunk.chunk_info(secure_channel)?` to use `&ChunkInfo` (already returns by value, may need `&mut chunk`).
   - `decode<T>`: same.

4. Note: This changes `chunk_info()` from taking `&self` to `&mut self`. Callers must provide mutable references. The `chunk` iteration in both validate and decode already has `&mut` access via the `chunks: &[MessageChunk]` requiring adaptation (may need `&mut chunks[i]`).

5. Verify: `cargo test -p async-opcua-core --lib`

---

## Post-implementation verification

```bash
# All cuts together
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
RUSTFLAGS="-D warnings" cargo check --workspace
cargo test -p async-opcua-server --lib
cargo test -p async-opcua-core --lib
cargo test -p async-opcua-nodes --lib
```
