# Complexity-cuts backlog (Big-O triage)

_Last updated 2026-07-04._

All complexity cuts have been applied via features 056 and 057:

| Cut | Feature | Description |
|-----|---------|-------------|
| Tier 1a | 056 | O(n²)→O(n) retransmission/publish-request queue cleanup |
| Tier 2a | 056 | `is_subtype_of` memoization via `moka::sync::Cache`, O(R·T)→O(1) |
| Tier 2b | 056 | `(parent,BrowseName)` index for TranslateBrowsePaths, O(D·M·R)→O(D) |
| Tier 3 #5 | 056 | Client `next_publish_time` recompute cached |
| Tier 3 #6 | 056 | CreateSession per-channel `HashMap<u32,AtomicUsize>` counter, O(sessions)→O(1) |
| Tier 3 #7 | 056 | Subscription priority dirty-flag cache, O(S log S)→O(1) stable |
| Tier 3 #8 | 056 | Chunk header `Mutex<Option<ChunkInfo>>` single-parse, 2×→1× |

All items from the original triage are complete. Previously deferred items (Tier 1b retransmission key-index, Tier 3 per-client recompute) have been re-evaluated and applied through features 056 and 057 where appropriate, or confirmed as bounded by existing caps/limits.

No remaining Big-O items.
