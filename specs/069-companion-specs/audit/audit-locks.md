# Lock Contention & Deadlock Audit

**Codebase**: async-opcua
**Date**: 2026-07-07
**Methodology**: Double-pass — first pass exhaustive grep for all lock acquisitions, second pass deep trace of priority targets.
**Scope**: ~1174 .rs files, ~593 lock acquisition sites, 1 `tokio::sync::Mutex`, ~310× `parking_lot::Mutex`/`parking_lot::RwLock` sites.

## Executive Summary

The codebase uses `parking_lot` sync locks (`Mutex`, `RwLock`) re-exported as `opcua_core::sync::{Mutex, RwLock}`. Only **one** async mutex `tokio::sync::Mutex<()>` exists (channel renewal serialization). All locks use the `trace_lock!` / `trace_read_lock!` / `trace_write_lock!` macros which optionally log acquisition for debugging. DashMap and ArcSwap are used correctly for hot-path lock-free access (auth token lookup, node store, 5 hot-path fields).

**Overall risk**: LOW-MEDIUM. No circular lock-ordering deadlocks found. Primary concerns are write-lock-hold-time contention and the single SQLite connection mutex bottleneck.

---

## Critical Lock Architecture Summary

| Lock | Type | Contention Profile |
|------|------|--------------------|
| `SubscriptionCache.inner` | `RwLock<SubscriptionCacheInner>` | Write-heavy under subscription create/delete; read-heavy on dispatch |
| `SessionManager` (outer) | `RwLock<SessionManager>` wrapped in `Arc` | Write on create/expire/close; read on every request dispatch |
| `Session` (per-session) | `RwLock<Session>` wrapped in `Arc` | Read on every request validation; write on activation |
| `AddressSpace.cold` | `RwLock<AddressSpaceCold>` | Read on browse; write on import/modify |
| `SqliteHistoryBackend.connection` | `Arc<Mutex<Connection>>` | Serializes ALL SQLite reads/writes |
| `SubscriptionState` (client) | `Mutex<SubscriptionState>` | Locked on every notification delivery and subscription mutation |
| `SecureChannel.encoding_context` | `Arc<RwLock<ContextOwned>>` | Read on every message encode/decode |
| `first_request_signature` | `std::sync::Mutex<Vec<u8>>` | ECC channel thumbprint (brief hold) |

---

## LOCK-001: `close_session` holds manager read lock while awaiting actor termination

**File**: `async-opcua-server/src/session/manager.rs:952-1034`
**Severity**: P1
**Category**: ordering / scope
**Summary**: `close_session` acquires `read(mgr_lck)` at line 959, drops it at line 986, awaits actor termination (an `.await` point), then acquires `write(mgr_lck)` at line 1001 and `read(session)` at line 1003. The manager state can change between these acquisitions.

**Detail**:
```rust
// Line 958-986: Read lock scope
let (session, id, token, actor_sender) = {
    let mgr = trace_read_lock!(mgr_lck);           // LOCK A: read(mgr)
    let Some(session) = mgr.find_by_token(...) else { return ... };
    let (id, token, authentication_token) = {
        let session = trace_read_lock!(session);    // LOCK B: read(session) nested in read(mgr)
        // ... validate session state ...
        (id, token, authentication_token)
    };
    let Some(actor_sender) = mgr.actor_sender(...) else { return ... };
    (session, id, token, actor_sender)
};                                                  // BOTH LOCKS DROPPED

// Line 988-999: AWAIT POINT — manager/session state can change here
let terminated = acknowledged.await...;

// Line 1000-1018: Re-acquisition
{
    let mut mgr = trace_write_lock!(mgr_lck);       // LOCK C: write(mgr)
    {
        let session = trace_read_lock!(&session);   // LOCK D: read(session) nested in write(mgr)
        // ... decrement unactivated counter ...
    }
    mgr.sessions.remove(&terminated.session_id);
}
```

**Lock ordering**: read(mgr) → read(session) → **DROP ALL** → await → write(mgr) → read(session)
**Contention impact**: The manager write lock (LOCK C) blocks all request dispatches during this window. Write lock duration is short (~map remove), but the await between releases means the session could be concurrently modified.
**Mitigation**: Structurally sound — locks are properly dropped before `.await`. The TOCTOU risk is inherent to async close semantics. Consider collapsing the post-await write scope to a single manager mutation without re-reading the session.

---

## LOCK-002: `commit_create_session_draft` — write lock held during O(n) eviction scan

**File**: `async-opcua-server/src/session/manager.rs:720-825`
**Severity**: P1
**Category**: contention / scope
**Summary**: Holds `&mut self` (which typically implies exclusive access already, but in production is `write(mgr)` from the controller) while iterating all sessions for eviction candidate search, then read-locks individual sessions.

**Detail**:
```rust
// Line 720-765: write(mgr) held during O(n) scan
pub(crate) fn commit_create_session_draft(&mut self, ...) -> ... {
    if self.sessions.len() >= self.info.config.limits.max_sessions {
        let eviction_candidate: Option<NodeId> = {
            let mut oldest: Option<(NodeId, Instant)> = None;
            for session_arc in self.sessions.values() {
                let arc = session_arc.clone();
                let session = trace_read_lock!(arc);   // read(session) nested in write(mgr)
                if !session.is_activated() {
                    let created = session.created_at();
                    if oldest.as_ref().is_none_or(|(_, t)| created < *t) {
                        oldest = Some((session.session_id().clone(), created));
                    }
                }
            }
            oldest.map(|(id, _)| id)
        };
        if let Some(evicted_id) = eviction_candidate {
            // ... write(evicted_session) — evict and close ...
        }
    }
    // ... insert new session, register token, spawn actor ...
}
```

**Lock ordering** (from controller at controller.rs:575):
```
write(mgr) → read(session₀..sessionₙ) → write(evicted_session)
```

**Contention impact**: When session count approaches `max_sessions`, every `CreateSession` holds the manager write lock for an O(n) scan. At typical limits (e.g., 100-1000 sessions), this is <1ms, but the write lock blocks ALL other request dispatches during this window.
**Mitigation**: The eviction scan could be done under a read lock first to find the candidate, then a write lock to perform the eviction + insertion. Or use a `BTreeMap` keyed by `created_at` for O(log n) eviction.

---

## LOCK-003: SQLite `Arc<Mutex<Connection>>` — single mutex serializes all history I/O

**File**: `async-opcua-history-sqlite/src/backend.rs:22-23, 372, 388, 736, 778, 828, 1005, 1164, 1282`
**Severity**: P1
**Category**: contention
**Summary**: ALL history operations (read_raw_modified, read_events, read_annotations, update_data, update_structure_data, update_event, delete_raw_modified) funnel through `spawn_blocking` closures that acquire `conn.lock()` on a single `Arc<Mutex<Connection>>`. Concurrent history clients serialize on this mutex.

**Detail**:
```rust
// backend.rs:22-23
pub struct SqliteHistoryBackend {
    connection: Arc<Mutex<Connection>>,             // SINGLE mutex for ALL SQLite I/O
    continuation_points: Arc<Mutex<HashMap<...>>>,   // secondary mutex
}

// backend.rs:293-317 — read_raw_modified path
async fn fetch_raw_modified_page(&self, ...) -> ... {
    let conn = self.connection.clone();
    let result = tokio::task::spawn_blocking(move || {
        Self::fetch_raw_modified_values(conn, request)  // → conn.lock() at line 372
    }).await...;
}

// backend.rs:563-646 — read_raw_modified (trait method)
async fn read_raw_modified(&self, ...) -> ... {
    self.prune_continuation_points();                   // LOCK A: continuation_points.lock()
    // ...
    let cp = self.continuation_points.lock().remove(&token)...;  // LOCK A again
    // ... then spawn_blocking with conn.lock()           // LOCK B
}
```

**Lock ordering**: `continuation_points.lock()` → `connection.lock()` (in `read_raw_modified`)
**Contention impact**: Two concurrent `read_raw_modified` calls for different nodes or two `update_data` calls will serialize on the SQLite connection mutex. SQLite itself supports WAL-mode concurrent reads, but the single mutex prevents parallelism. Each history call also goes through `spawn_blocking`, which can exhaust the blocking thread pool under heavy load.
**Mitigation**: Use a connection pool (e.g., `r2d2_sqlite`) with WAL-mode SQLite. This would allow concurrent reads while writes still serialize. Alternatively, use `tokio::task::spawn_blocking` with a semaphore to limit concurrent SQLite access rather than the mutex.

---

## LOCK-004: `create_subscription` — write lock held during `SessionEntry` construction

**File**: `async-opcua-server/src/subscriptions/mod.rs:887-932`
**Severity**: P1
**Category**: contention / nesting
**Summary**: `create_subscription` acquires `write(self.inner)` at line 894, constructs a `SessionEntry` inside the write lock which reads session state and spawns an actor, then drops and re-acquires `write(self.inner)` at line 920 to update `subscription_to_session`.

**Detail**:
```rust
// mod.rs:887-932
pub(crate) async fn create_subscription(&self, session_id: u32, ...) -> ... {
    let cache = {
        let mut lck = trace_write_lock!(self.inner);        // LOCK: write(inner)
        lck.session_subscriptions
            .entry(session_id)
            .or_insert_with(|| {
                SessionEntry::new(                            // Inside write lock:
                    session_id, self.limits,
                    Self::get_key(&context.session),          // → read(session)
                    context.session.clone(),                  //   clones session Arc
                    ..., cleanup_tx,
                )
            })
            .handle()
    };                                                        // LOCK DROPPED
    // ... async work with cache handle ...
    let res = cache.create_subscription(request, info).await...;
    let mut lck = trace_write_lock!(self.inner);              // LOCK RE-ACQUIRED
    lck.subscription_to_session.insert(res.subscription_id, session_id);
    Ok(res)
}
```

**Lock ordering**: write(inner) → read(session) (inside `SessionEntry::new`)
**Contention impact**: The first write lock is held during `SessionEntry::new` which clones `NodeManagers`, reads session state, and spawns a `SubscriptionActor`. The write lock duration is short (RwLock write on the inner map), but it blocks all other subscription operations. The drop/re-acquire pattern is correct but means the intermediate state is observable.
**Mitigation**: Pre-compute the `SessionEntry` key (`PersistentSessionKey`) outside the write lock. This reduces write lock hold time to just HashMap insertion.

---

## LOCK-005: `first_request_signature` uses `std::sync::Mutex` instead of `parking_lot::Mutex`

**File**: `async-opcua-core/src/comms/secure_channel.rs:123`
**Severity**: P2
**Category**: alternative
**Summary**: The ECC `first_request_signature` field is a `std::sync::Mutex<Vec<u8>>`, not `parking_lot::Mutex`. This is inconsistent with the rest of the codebase but the hold time is trivial.

**Detail**:
```rust
// secure_channel.rs:123
#[cfg(feature = "ecc")]
first_request_signature: Mutex<Vec<u8>>,      // std::sync::Mutex, not parking_lot

// secure_channel.rs:1331-1336
let mut first_request_signature = self
    .first_request_signature
    .lock()                                     // std::sync::Mutex::lock — may poison
    .map_err(|_| StatusCode::BadSecurityChecksFailed)?;
first_request_signature.clear();
first_request_signature.extend_from_slice(signature);
```

**Lock sites**: Lines 1332, 1339, 1690, 1713 — all brief, all within `asymmetric_sign_and_encrypt` and `asymmetric_decrypt_and_verify`.
**Contention impact**: Negligible — ECC channel setup is infrequent and the lock hold is <100ns. The poison error handling is correct.
**Mitigation**: Replace with `parking_lot::Mutex` for consistency, or leave as-is (no practical concern).

---

## LOCK-006: `tokio::sync::Mutex` for channel renewal with nested parking_lot locks

**File**: `async-opcua-client/src/transport/channel.rs:44, 194-199`
**Severity**: P2
**Category**: nesting
**Summary**: The `issue_channel_lock: tokio::sync::Mutex<()>` is held across `.await` at line 195, then used to guard a section that acquires `read(self.secure_channel)` and `write(self.secure_channel)` parking_lot locks. Mixed async/sync lock nesting.

**Detail**:
```rust
// channel.rs:190-200
async fn renew_secure_channel(&self, send: Sender<OutgoingMessage>) -> Result<(), Error> {
    let _guard = self.issue_channel_lock.lock().await;        // tokio Mutex (async)
    let should_renew_security_token = {
        let secure_channel = trace_read_lock!(self.secure_channel);  // parking_lot (sync)
        secure_channel.should_renew_security_token()
    };
    // ... async renewal work while holding tokio Mutex ...
    // Line 367: let mut secure_channel = trace_write_lock!(self.secure_channel);
}
```

**Lock ordering**: tokio::Mutex → parking_lot::RwLock (read) → tokio::Mutex → parking_lot::RwLock (write)
**Contention impact**: This is the ONLY async mutex in the codebase. It correctly serializes channel renewal to prevent double-renewal. The mixed lock types are safe because parking_lot locks are never held across `.await` and are dropped before the tokio mutex guard is dropped.
**Mitigation**: Acceptable pattern. Document the invariant that parking_lot locks must be dropped before `.await`.

---

## LOCK-007: Client `SubscriptionState` Mutex — 22 lock sites, some hold across computation

**File**: `async-opcua-client/src/session/services/subscriptions/service.rs:668, 1796, 1870, 1939, 1982, 2109, 2141, 2208, 2254, 2284, 2314, 2347, 2388, 2419, 2441, 2447, 2458, 2487, 2510, 2542, 2553, 2592`
**Severity**: P2
**Category**: contention
**Summary**: A single `Mutex<SubscriptionState>` guards the entire client subscription state. 22 lock acquisition sites in `service.rs` alone.

**Detail**:
```rust
// service.rs:668 — restore_now
fn restore_now(&mut self) {
    let mut subscription_state = trace_lock!(self.subscription_state);
    delivery.restore(&mut subscription_state);  // holds lock during Vec push/map insert
}

// service.rs:2542 — notification delivery  
let mut lck = trace_lock!(self.subscription_state);
// ... iterate through monitored items, check filters, build notification ...
```

**Lock ordering**: Flat — no nested locks. The `subscription_state` mutex is always the only lock held.
**Contention impact**: The mutex is held during notification delivery callbacks. If a callback blocks or is slow, all other subscription operations (create, modify, delete) will wait. The 22 sites are all in the same `SubscriptionService` struct, so they will contend with each other.
**Mitigation**: Consider splitting `SubscriptionState` into finer-grained locks: one for subscriptions map, one for monitored items, one for the notification queue. Or use a channel-based design where notification delivery is lock-free.

---

## LOCK-008: `ensure_browse_name_index` — TOCTOU read-then-write pattern

**File**: `async-opcua-server/src/address_space/mod.rs:391-394, 359-387`
**Severity**: P2
**Category**: contention / ordering
**Summary**: `ensure_browse_name_index` does `read(cold)` to check if the index exists, then calls `build_browse_name_index` which takes `write(cold)`. Under concurrent calls, both threads may try to build the index.

**Detail**:
```rust
// mod.rs:391-394
pub fn ensure_browse_name_index(&self, type_tree: &dyn TypeTree) {
    if self.cold.read().browse_name_index.is_none() {   // read check
        self.build_browse_name_index(type_tree);          // write build
    }
}

// mod.rs:359-387
pub fn build_browse_name_index(&self, type_tree: &dyn TypeTree) {
    let mut cold = self.cold.write();                     // acquires write lock
    // ... iterate all nodes, build HashMap index (potentially expensive) ...
    cold.browse_name_index = Some(index);
}
```

**Contention impact**: Two concurrent calls may both pass the `is_none()` check, then serialize on the write lock. The second thread rebuilds the index unnecessarily. `build_browse_name_index` iterates ALL nodes in the address space, which can be costly for large node sets.
**Mitigation**: Add a double-check inside `build_browse_name_index` after acquiring the write lock, or use an `OnceLock`/`OnceCell` for the index.

---

## LOCK-009: `refresh_client_response_body_limit_for_channel` — O(n) session iteration

**File**: `async-opcua-server/src/session/manager.rs:637-662`
**Severity**: P2
**Category**: contention / scope
**Summary**: Iterates ALL sessions, acquiring `read(session)` for each, to find the minimum `max_response_message_size` for a given channel. Called from `commit_create_session_draft` (under write(mgr)), `close_session` (under write(mgr)), and `activate_session` (under read(mgr)).

**Detail**:
```rust
// manager.rs:637-662
fn refresh_client_response_body_limit_for_channel(&self, channel: &mut SecureChannel) {
    let effective_limit = self.sessions.values()
        .filter_map(|session| {
            let session = trace_read_lock!(session);          // read lock per session
            let is_closed = matches!(...);
            if session.secure_channel_id() == secure_channel_id && !is_closed {
                let limit = session.max_response_message_size();
                (limit > 0).then_some(limit)
            } else { None }
        })
        .min();
    channel.set_client_response_body_limit(effective_limit.unwrap_or(0));
}
```

**Lock ordering**: read(session₀..sessionₙ) — flat, no nesting.
**Contention impact**: Each call acquires N read locks. For 1000 sessions this is 1000 `parking_lot::RwLock::read()` calls. Read locks are cheap, but the cumulative cost + HashMap iteration is notable. Called on every CreateSession, ActivateSession, and CloseSession.
**Mitigation**: Track per-channel `max_response_message_size` as a separate `DashMap<u32, u32>` keyed by `secure_channel_id`, updated incrementally on session create/close. Eliminates the scan.

---

## LOCK-010: `activate_session` — read-then-write on session with manager read held

**File**: `async-opcua-server/src/session/manager.rs:1036-1321`
**Severity**: P2
**Category**: nesting / ordering
**Summary**: `activate_session` acquires `read(mgr_lck)` at line 1047, then `read(session)` at line 1054, drops both, does async authentication, then `write(session)` at line 1227. During the write lock, it reads `mgr` again at line 1308 (nested read inside write).

**Detail**:
```rust
// manager.rs:1046-1130
let (endpoint_url, session_nonce, session_lck, info) = {
    let mgr = trace_read_lock!(mgr_lck);               // read(mgr)
    let session = trace_read_lock!(session_lck);        // read(session) nested in read(mgr)
    // ... validate endpoint, security mode, signature ...
    (endpoint_url, session_nonce, session_lck, mgr.info.clone())
};                                                       // BOTH DROPPED

// ... async authentication (no locks held) ...

// manager.rs:1226-1320
let (server_nonce, session_id, user_changed, user_token) = {
    let mut session = trace_write_lock!(session_lck);   // write(session)
    // ... activate with all parameters ...
    if was_unactivated {
        let mgr = trace_read_lock!(mgr_lck);             // read(mgr) nested in write(session)!
        if let Some(counter) = mgr.unactivated_by_channel.get(...) {
            counter.fetch_sub(1, Ordering::Release);
        }
    }
    ...
};
```

**Lock ordering**: read(mgr) → read(session) → **DROP ALL** → **async** → write(session) → read(mgr)
**Contention impact**: The nested `read(mgr)` inside `write(session)` at line 1308 is a reverse ordering from the outer scope (which had read(mgr) → read(session)). This is safe only because the `mgr` read lock is Rc/RwLock style — non-exclusive. The `write(session)` lock scope is moderate (activation writes many fields).
**Mitigation**: Pre-compute the `was_unactivated` flag and the channel ID before the `write(session)` scope, so the nested `read(mgr)` can be eliminated.

---

## LOCK-011: AddressSpace `write(cold)` patterns — brief but serialized

**File**: `async-opcua-server/src/address_space/mod.rs:81, 244, 286, 296, 307, 319, 362, 414, 625, 679`
**Severity**: P2
**Category**: contention
**Summary**: The address space uses DashMap for nodes (lock-free) but `RwLock<AddressSpaceCold>` for references, namespaces, and browse-name index. Every reference insert/delete takes `write(cold)`. The `write(cold)` at line 362 (`build_browse_name_index`) is the most expensive — it iterates all nodes and builds a HashMap index.

**Detail**:
```rust
// mod.rs:44-48
pub struct AddressSpace {
    pub node_map: NodeMap,                          // DashMap — lock-free
    pub cold: RwLock<AddressSpaceCold>,              // RwLock — serializes writes
}
```

**Lock ordering**: Always flat — `write(cold)` or `read(cold)` is never nested with other locks from different modules.
**Contention impact**: Reference writes are fast (~1-10µs). `build_browse_name_index` is O(nodes) — for 10K+ nodes this could be 10-100ms holding a write lock. During this time, all reads on `cold` (browse operations) block.
**Mitigation**: The hot path (node existence check, reads) uses DashMap correctly. For `build_browse_name_index`, consider building the index in a separate HashMap, then atomically swapping it in with a brief write lock. Or use `ArcSwap` for the `browse_name_index`.

---

## LOCK-012: Two sequential `write(inner)` in `teardown_session`

**File**: `async-opcua-server/src/subscriptions/mod.rs:1606-1637`
**Severity**: P2
**Category**: scope
**Summary**: `teardown_session` acquires `write(inner)` at line 1612 to remove a `SessionEntry`, drops it, does async work, then acquires `write(inner)` again at line 1626 to clean up monitored items and subscription-to-session mappings.

**Detail**:
```rust
// mod.rs:1606-1637
pub(crate) async fn teardown_session(&self, session_id: u32, ...) {
    let entry = {
        let mut lck = trace_write_lock!(self.inner);       // FIRST write(inner)
        lck.session_subscriptions.remove(&session_id)
    };                                                       // DROPPED
    // ... async work — entry.handle.subscription_and_item_data().await ...
    let (subscription_ids, monitored_items) = entry.handle...await...;
    {
        let mut lck = trace_write_lock!(self.inner);       // SECOND write(inner)
        for id in subscription_ids {
            lck.subscription_to_session.remove(&id);
        }
        Self::cleanup_monitored_item_refs(&mut lck, &monitored_items);
    }
}
```

**Contention impact**: Two separate write lock acquisitions. The second one does O(subscriptions) HashMap removals and O(monitored_items) cleanup. Both are fast (<1ms) but hold the write lock for the duration.
**Mitigation**: These could be merged into a single write lock acquisition if `subscription_and_item_data()` results can be pre-fetched before the first write lock.

---

## LOCK-013: NodeManager `address_space.write()` during program execution

**File**: `async-opcua-server/src/programs/engine.rs:74, 101, 144, 151, 160, 188`
**Severity**: P2
**Category**: contention
**Summary**: The program engine (state machine execution) acquires `address_space.write()` and `state_machine.write()` inside a loop, with `progress.write()` counters being frequently updated.

**Detail**:
```rust
// engine.rs:74 — start
let space = self.address_space.write();                    // holds write across program init

// engine.rs:131 — run loop
while sm_clone.read().state() == ProgramState::Suspended {

// engine.rs:144 — progress update (inside loop)
*progress.write() = i;

// engine.rs:151 — address_space write inside loop
let space = address_space.write();                         // write lock per iteration!
```

**Lock ordering**: write(address_space) → read(state_machine) or write(state_machine) → write(progress)
**Contention impact**: The `address_space.write()` inside the program loop (line 151) is alarming — it acquires a write lock per iteration, blocking all browse operations on the address space during program execution. For long-running programs, this starves readers.
**Mitigation**: Use `write(address_space)` once at the start of the loop body and hold for the shortest possible scope, or use downgrade to `read` if possible. Consider batching address space mutations.

---

## LOCK-014: `info.type_tree.write()` held during node manager import

**File**: `async-opcua-server/src/node_manager/memory/mod.rs:1522`, `async-opcua-server/src/node_manager/memory/memory_mgr_impl.rs:146, 2282`
**Severity**: P2
**Category**: contention
**Summary**: `type_tree.write()` is held while adding type nodes one-by-one during initialization. Each `add_type_node` call adds references, etc.

**Detail**:
```rust
// memory_mgr_impl.rs:2282
let mut tt = context.type_tree.write();
// ... add multiple type nodes while holding write lock ...
// line 2122: context.type_tree.write().add_type_node(...)
// line 2128: context.type_tree.write().add_type_node(...)
// line 2188: context.type_tree.write().add_type_node(...)
```

**Contention impact**: During server startup, the type tree is populated under write locks. Since startup is single-threaded by nature, this is low risk. But if type tree is lazily loaded or updated at runtime, contention could occur.
**Mitigation**: Batch type tree operations or use bulk-insert methods that acquire the write lock once.

---

## Lock Ordering Map

The following lock acquisition orderings are observed. No AB/BA circular deadlock potential was found because:

1. `SessionManager` (mgr) is always the outermost lock.
2. `Session` locks are always acquired inside (or after) mgr.
3. `SubscriptionCache.inner` is independent — never acquired while holding mgr or session.
4. `AddressSpace.cold` is independent — never acquired while holding mgr or session.
5. SQLite `connection` mutex is always the innermost lock.

```
Hierarchy (outer → inner):

SessionManager.write  →  Session.read/write  →  (no further locks)
SessionManager.read   →  Session.read/write  →  (no further locks)
                         Session.write       →  SessionManager.read   [REVERSE at LOCK-010]

SubscriptionCache.write →  Session.read (in SessionEntry::new)
SubscriptionCache.read  →  (no nested locks, spawns actor messages)

AddressSpace.write/read →  (flat, no nesting)

tokio::Mutex (channel renewal) →  SecureChannel.write/read (parking_lot)

continuation_points.lock() →  connection.lock()  (SQLite, always same order)
```

**Verdict**: The reverse ordering at LOCK-010 (write(Session) → read(SessionManager)) is safe because:
- `SessionManager` is `RwLock` — multiple readers allowed
- The write(Session) only holds read(mgr) briefly to increment an atomic counter
- No code path holds write(mgr) then tries to write the same session (that would deadlock)

---

## Lock-Free Alternatives Assessment

| Lock | Alternative | Feasibility |
|------|-------------|-------------|
| `AddressSpace.cold: RwLock<...>` | `ArcSwap<AddressSpaceCold>` | Viable — swap entire cold state atomically. Would eliminate read contention on browse. |
| `SubscriptionCache.inner: RwLock<...>` | `DashMap<u32, SessionEntry>` | Viable for `session_subscriptions` and `subscription_to_session`. The `monitored_items` map is trickier due to `HashMap<MonitoredItemKey, HashMap<...>>`. |
| `SqliteHistoryBackend.connection: Mutex<Connection>` | Connection pool (r2d2) | Viable — SQLite WAL mode supports concurrent readers. |
| `SessionManager.sessions: HashMap<...>` | `DashMap` | Already partially migrated (`auth_tokens` uses DashMap). The `sessions` HashMap could be DashMap for O(1) concurrent access. |
| `client SubscriptionState: Mutex<...>` | Channel-based | The notification delivery path could use `tokio::sync::mpsc` to decouple locking from delivery. |

---

## Hot-Path Lock Acquisitions Per Request

For a typical OPC UA **Read** request on an activated session:

1. `read(mgr)` — find_by_token (DashMap lookup, lock-free)
2. `read(session)` — validate_request (session read lock)
3. `read(address_space.node_map)` — DashMap (lock-free)
4. `read(type_tree)` — type resolution (RwLock read)
5. `read(encoding_context)` — serialization (RwLock read)

**Total**: 3x RwLock reads, 0x writes. All cheap.

For a typical OPC UA **CreateSubscription** request:

1. `read(mgr)` — find_by_token
2. `read(session)` — validate
3. `write(subscription_cache.inner)` — create SessionEntry
4. `read(session)` — get persistent key (inside SessionEntry::new)
5. `write(subscription_cache.inner)` — update subscription_to_session (second acquisition)

**Total**: 2x RwLock reads, 2x RwLock writes. Acceptable.

---

## Recommendations (Priority Order)

1. **[P1] LOCK-003**: Implement SQLite connection pooling with WAL mode to eliminate serialization of concurrent history reads.
2. **[P1] LOCK-002**: Pre-scan eviction candidate under read lock, then upgrade to write lock for commit.
3. **[P1] LOCK-001**: Collapse post-await session state re-reading in `close_session`.
4. **[P2] LOCK-009**: Replace O(n) session iteration in `refresh_client_response_body_limit_for_channel` with per-channel DashMap tracking.
5. **[P2] LOCK-010**: Eliminate nested `read(mgr)` inside `write(session)` by pre-computing the `was_unactivated` flag.
6. **[P2] LOCK-008**: Add double-check under write lock in `build_browse_name_index`.
7. **[P2] LOCK-013**: Reduce address_space write lock scope in program engine loop — batch mutations.
8. **[P2] LOCK-011**: Use `ArcSwap` for `AddressSpaceCold` to eliminate browse-path contention.
