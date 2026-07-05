# Research: Hot-Path and Lock Optimization

## US1 — Split AddressSpace Hot/Cold

### Current Architecture

`AddressSpace` in `async-opcua-server/src/address_space/mod.rs` is wrapped in `Arc<RwLock<AddressSpace>>` and contains:

```rust
pub struct AddressSpace {
    node_map: DashMap<NodeId, NodeType>,       // HOT — hit every Read
    references: Vec<Reference>,                // COLD — write on node add/delete
    browse_name_index: HashMap<QualifiedName, Vec<NodeId>>,  // COLD — write on node add/delete
    namespaces: Vec<Namespace>,                // COLD — read-only after startup
}
```

Every Read calls `address_space.read().node_map.get(node_id)` — acquiring `parking_lot::RwLock::read()` just to reach through to `DashMap::get()`, which is already lock-free internally via sharded stripes.

On a single core this is ~free (uncontended RwLock read is a single atomic increment). On multi-core, the RwLock state word bounces between L1 caches, causing measurable contention.

### Decision: Expose `Arc<DashMap<NodeId, NodeType>>` directly, move cold fields behind `RwLock<AddressSpaceCold>`

**Rationale**: `node_map` is a `DashMap` — already designed for concurrent lock-free access. The outer `RwLock` is purely redundant overhead. Cold fields (`references`, `browse_name_index`, `namespaces`) are infrequently written (only on node manager operations, not on reads) so a separate `RwLock` for them is appropriate.

**Design**:
1. Add `pub struct AddressSpaceCold { references, browse_name_index, namespaces }` 
2. Change `AddressSpace` to hold `Arc<DashMap<NodeId, NodeType>>` + `RwLock<AddressSpaceCold>`
3. The outer `Arc<RwLock<AddressSpace>>` becomes `Arc<AddressSpace>` where `AddressSpace` itself is `Send + Sync` without an outer lock
4. All Read sites (`memory/mod.rs`, node manager reads) use `address_space.node_map.get()` directly — no lock
5. All Write/modify sites use `address_space.cold.write().references.push(...)` — unchanged from current pattern with a different lock target

**Key challenge**: The `AddressSpace` is currently used as a single locked unit. Call-sites that access both `node_map` and cold fields in the same critical section need audit. From the lock audit in TODO.md: "`namespaces` is read-only after server startup; `references` and `browse_name_index` aren't touched by simple Read." This means the split is safe for the Read path.

**Alternatives considered**:
- **Keep `RwLock` but use `RwLock<Arc<DashMap>>` instead**: Does nothing — the outer lock still bounces.
- **Use `dashmap::DashMap`'s own `RwLock`-like API with explicit locking**: Already the case internally; the problem is the outer `parking_lot::RwLock`.
- **Merge cold fields into `DashMap`**: `references` is a `Vec` with ordering requirements — not suitable for a concurrent map. The cold fields are best kept behind a write-side lock.

---

## US2 — Cache Session Arc in Request Dispatch Context

### Current Architecture

`SessionManager::find_by_token` performs a hash-table lookup (`BiMap`) each time any request operation needs the session. The token→session mapping is stable for the duration of a request, but the lookup is repeated for each sub-operation (e.g., a service call may look up the session for attribute reads, browse, etc.).

### Decision: Cache `Arc<RwLock<Session>>` in the per-request dispatch context

**Rationale**: The session token is authenticated once at the start of request processing. The `Arc<RwLock<Session>>` is cloneable and the session's lifetime is independent of the request's lifetime. Caching it in the request context avoids repeated hash lookups.

**Design**:
1. Add `Option<(NodeId, Arc<RwLock<Session>>)>` to the request context struct (or to the dispatch params passed through the service layer)
2. On first `find_by_token` call, populate the cache
3. Subsequent accesses use the cached value
4. Drop is automatic — `Arc` ref-count handles lifecycle

**Key challenge**: The session must remain valid even if terminated mid-request (the cached `Arc` keeps the `Session` struct alive; the `Session` state may be `Closed` but the struct itself isn't deallocated until all `Arc`s are dropped). Read-side checks for session state already exist and will correctly reject operations on a closed session.

**Alternatives considered**:
- **Use thread-local storage**: Not applicable in an async/tokio context.
- **Use a `HashMap` in the controller**: Would add controller-level complexity. Per-request caching is simpler and more locally scoped.

---

## US3 — Replace Per-Request Timers With Shared Deadline Queue

### Current Architecture

In `SessionController::run()`, each inflight request spawns a `tokio::time::sleep_until(deadline)`. The `FuturesUnordered` drives these. Each timer creation/destruction calls `TimerEntry::drop` (cancellation) and `TimerEntry::reset`, which collectively cost ~2.8% CPU.

### Decision: Replace with a `BTreeMap`-backed deadline queue checked once per tick

**Rationale**: A single `BTreeMap<Instant, Vec<RequestId>>` (or equivalent) can track all inflight deadlines. The event loop checks `peek()` on each tick, comparing to `Instant::now()`. If the earliest deadline has passed, pop and timeout those requests. No per-request timer allocation; no timer wheel pressure.

**Design**:
1. Add `deadline_queue: BTreeMap<Instant, Vec<RequestId>>` to `SessionController`
2. On request dispatch: `deadline_queue.entry(deadline).or_default().push(request_id)`
3. On each event loop tick in `run()`: `while let Some((&t, _)) = deadline_queue.first_key_value() { if t > Instant::now() { break; } /* pop and timeout */ }`
4. On request completion: remove from deadline queue (or use lazy cleanup — a `completed` hash set checked at deadline pop time)

**Key challenge**: The deadline granularity must match or exceed the current `sleep_until` behavior. Since this is checked once per event loop tick (~microseconds), the effective timeout accuracy is within one tick duration, which is acceptable for the OPC UA timeout contract (typically seconds).

**Alternatives considered**:
- **Use tokio's `Sleep` but share a single future**: Would require cancelling and recreating the future each tick. `Instant::now()` compare is cheaper.
- **Use `tokio::time::Interval`**: Only fires at fixed intervals. Deadlines are request-specific and vary per request.
- **Keep individual timers but don't spawn them as separate futures**: The `FuturesUnordered` already handles polling; the problem is the number of timer entries in the runtime's timer wheel.

---

## US4 — Investigate and Resolve ArcSwap Debt Overhead

### Current Architecture

`arc_swap` is used in several places in the codebase. The `Debt::pay_all` overhead at ~2.5% CPU suggests many concurrent readers across multiple `ArcSwap` instances, each accumulating "debt" that must be paid by checking for new writes.

### Decision: Profile-first investigation, then apply targeted fix

**Rationale**: This is an investigative task — the overhead is visible in perf but the root cause (which ArcSwap instances) needs confirmation. Common causes:
- Multiple `ArcSwap<ServerInfo>` instances read at high frequency
- `ArcSwap<DiagnosticsConfig>` or similar config structs read on every request
- Configuration that changes only during setup but is wrapped in `ArcSwap` unnecessarily

**Design**:
1. Profile with `perf record -g` and inspect `Debt::pay_all` callers to identify which `ArcSwap::load()` callsites are hot
2. For each identified instance, evaluate:
   - **If writes are startup-only**: Replace with `Arc` (no swap needed)
   - **If writes are rare (e.g., config reload)**: Replace with `Arc` + `AtomicU64` generation counter (reader checks generation, reloads on mismatch). This is cheaper than ArcSwap's slot-based mechanism for rare-write workloads.
   - **If writes are truly concurrent with reads**: Keep ArcSwap but batch reads where possible

**Key challenge**: Changing from `ArcSwap` to `Arc` loses the wait-free read guarantee. For startup-only data, this is fine (no concurrent writes). For config reload, the generation-counter pattern provides comparable semantics.

**Alternatives considered**:
- **Reduce ArcSwap slot count**: ArcSwap internally uses 2 slots by default. Reducing to 1 slot removes the need for debt tracking but makes reads blocking (not acceptable for the hot path).
- **Use `AtomicPtr` directly**: Lower-level than ArcSwap but requires manual memory management. ArcSwap wraps this correctly; the goal is to avoid the swap mechanism entirely, not reimplement it.
