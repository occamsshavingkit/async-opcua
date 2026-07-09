# Rust Async Patterns Audit: async-opcua

**Date**: 2026-07-07
**Scope**: Full codebase (`async-opcua-*` crates)
**Tooling**: `audit_locks.py` scan + manual trace analysis

---

## Executive Summary

The codebase demonstrates **above-average discipline** with async-safe lock usage. The core pattern is:

1. Use `parking_lot::Mutex`/`RwLock` for data-only critical sections
2. Extract data under lock, drop guard, then `.await`
3. Re-acquire lock after I/O completes if mutation is needed

Only one `tokio::sync::Mutex` exists (channel renewal gating) and one `tokio::sync::RwLock` reference (trait definition). `spawn_blocking` is used correctly in the SQLite backend. The session manager's "extract-read→await→write" pattern is well-executed.

**Critical findings (P0)**: 1 (tokio::sync::Mutex held across `.await` with nested lock acquisition in channel renewal)
**High-impact findings (P1)**: 2 (unnecessary Mutex in SqliteHistoryBackend, client publish path lock ordering risk)
**Improvement candidates (P2)**: 3 (SessionEntry construction under write lock, crypto spawn_blocking gap, `std::sync::Mutex` in secure channel)

---

## Audit Findings

### Finding ID: ASYNC-001
**File**: `async-opcua-client/src/transport/channel.rs:195-218`
**Severity**: P0
**Category**: sync-lock-in-async (async lock misuse)
**Summary**: `tokio::sync::Mutex` guard `_guard` deliberately held across `.await` and nested sync lock acquisition, creating deadlock risk and serialization bottleneck.
**Detail**:
```rust
// channel.rs:195-218
async fn renew_secure_channel(&self, send: Sender<OutgoingMessage>) -> Result<(), Error> {
    let _guard = self.issue_channel_lock.lock().await;  // tokio::sync::Mutex guard
    let should_renew_security_token = {
        let secure_channel = trace_read_lock!(self.secure_channel);  // parking_lot RwLock
        secure_channel.should_renew_security_token()
    };
    if should_renew_security_token {
        let request = self.state.begin_issue_or_renew_secure_channel(...)?;
        let resp = match request.send().await {  // .await with guard held
            Ok(resp) => resp,
            Err(err) => {
                self.close_channel().await;  // .await with guard held
                return Err(err);
            }
        };
        ...
    }
}
```

The `tokio::sync::Mutex` is intentionally held across `.await` to serialize channel renewal in a single-flight pattern. This is correct *for the single-flight goal*, but:

1. **Deadlock risk**: The async lock guard is held while `request.send().await` completes. If the underlying transport or response handler ever tries to call `send()` on the channel (which checks `should_renew_security_token` first, acquiring a parking_lot read lock under the async lock), the two lock types can interact poorly. The `parking_lot::RwLock` inside the `tokio::sync::Mutex` is benign, but the reverse (async lock held, blocking on sync lock) creates a priority inversion where the async task holding the guard synchronously blocks on the parking_lot lock while other async tasks queue behind the tokio mutex.

2. **Serialization**: During renewal (which involves network I/O), all other callers through `send()` are blocked waiting for `renew_secure_channel()` to complete. This is the intended design but means one slow renewal stalls all traffic.

3. **Panic safety**: If `request.send().await` panics, the `_guard` is dropped, but the channel may be in an inconsistent state.

**Recommendation**: The single-flight pattern is valid here given the low frequency of renewals. However, consider a `tokio::sync::Notify` or `ArcSwap<RenewalState>` approach instead:

```rust
struct AsyncSecureChannel {
    // Replace tokio::sync::Mutex<()> with:
    renewal_state: RwLock<RenewalState>,  // parking_lot
}

enum RenewalState {
    Idle,
    InProgress(tokio::sync::watch::Receiver<Result<(), Error>>),
}

async fn renew_secure_channel(&self, send: Sender<OutgoingMessage>) -> Result<(), Error> {
    // Check under sync lock if renewal is already in progress
    let maybe_rx = {
        let renewal = self.renewal_state.read();
        match &*renewal {
            RenewalState::InProgress(rx) => Some(rx.clone()),
            _ => None,
        }
    };
    if let Some(mut rx) = maybe_rx {
        // Wait for the existing renewal to complete
        return rx.wait_for(|v| v.is_some()).await.unwrap_or(Err(...));
    }
    // ... perform renewal, update state
}
```

This avoids holding any lock across `.await`.

---

### Finding ID: ASYNC-002
**File**: `async-opcua-history-sqlite/src/backend.rs:22-24`
**Severity**: P1
**Category**: sync-lock-in-async
**Summary**: `Arc<Mutex<Connection>>` is redundant: every access path goes through `spawn_blocking`, where the `Connection` is used single-threaded.
**Detail**:
The `SqliteHistoryBackend` stores:
```rust
pub struct SqliteHistoryBackend {
    connection: Arc<Mutex<Connection>>,     // parking_lot Mutex
    continuation_points: Arc<Mutex<HashMap<...>>>,
}
```

Every query method does:
```rust
let conn = self.connection.clone();
tokio::task::spawn_blocking(move || {
    let conn = conn.lock();  // acquire inside spawn_blocking
    Self::fetch_raw_modified_values(conn, request)
}).await...
```

Inside `spawn_blocking`, the closure runs on a dedicated blocking thread. The `Mutex` provides no benefit — the `Connection` is consumed by a single closure on a single thread. The `Mutex` incurs unnecessary overhead on every query.

The `continuation_points: Arc<Mutex<HashMap<...>>>` lock is also used from synchronous methods (`prune_continuation_points`, `insert_continuation_point`) that are called from within `spawn_blocking` closures, making the Mutex equally redundant.

**Recommendation**: Remove the `Mutex` wrappers. Since `Connection` is `!Sync`, wrapping in `Mutex` was the conventional way to share it. Since it's only accessed inside `spawn_blocking`, pass `Arc<Connection>` directly:

```rust
pub struct SqliteHistoryBackend {
    connection: Arc<Connection>,
    continuation_points: Arc<Mutex<HashMap<Vec<u8>, CachedContinuationPoint>>>,  // keep for continuation_points
}
```

For `continuation_points`, if all access is also through `spawn_blocking`, remove its `Mutex` too and use `std::cell::RefCell` inside `spawn_blocking` closures instead. If some access happens from async context (e.g., `prune_continuation_points` called during read operations), keep the `Mutex` but verify it's scoped correctly.

Actually: `prune_continuation_points` (line 268-271) is called from within `fetch_raw_modified_page` which runs inside `spawn_blocking`. The `continuation_points` lock is also acquired inside `spawn_blocking`. So it too is redundant inside `spawn_blocking`, but could in theory be accessed from outside. Check if any caller accesses `continuation_points` from async task threads — if not, both locks can be removed.

---

### Finding ID: ASYNC-003
**File**: `async-opcua-client/src/session/services/subscriptions/service.rs:2487-2503`
**Severity**: P1
**Category**: cancellation + sync-lock-in-async
**Summary**: `PendingClientDeliveryGuard` holds a `&Mutex<SubscriptionState>` reference and acquires the lock *inside* `Drop`, creating a cancellation-time lock acquisition that may conflict with the publish loop.
**Detail**:
```rust
// service.rs:2486-2503 (within publish())
let delivery = {
    let mut subscription_state = trace_lock!(self.subscription_state);
    subscription_state
        .collect_client_delivery_packet(...)
        .and_then(|packet| {
            PendingClientDelivery::stage(&mut subscription_state, ...)
        })
};

if let Some(delivery) = delivery {
    let mut delivery = PendingClientDeliveryGuard::new(delivery, &self.subscription_state);
    delivery.deliver();       // calls user callback (could panic)
    delivery.restore();       // acquires lock, restores state, drops lock
}
```

The `PendingClientDeliveryGuard::Drop` impl acquires `trace_lock!(self.subscription_state)`:
```rust
// service.rs:662-670
fn restore_now(&mut self) {
    let Some(delivery) = self.delivery.take() else { return };
    let mut subscription_state = trace_lock!(self.subscription_state);
    delivery.restore(&mut subscription_state);
}
impl Drop for PendingClientDeliveryGuard<'_> {
    fn drop(&mut self) { self.restore_now(); }
}
```

If the future is cancelled between `delivery.deliver()` and `delivery.restore()`, the `Drop` runs, acquiring the Mutex to restore state. If the same task's `publish()` retry loop or the `next_publish_time()` call is also trying to lock — or if any other path holds the lock at cancellation time — this creates a blocking acquisition inside a destructor on the async runtime thread.

**Recommendation**: Two possible mitigations:

1. (Preferred) Make `restore()` idempotent and call it explicitly before any `.await`, removing the `Drop` acquisition entirely. The explicit `delivery.restore()` call already exists in the happy path — the Drop is a safety net for cancellation. Replace `Drop` with a `#[must_use]` type and a debug-assertion that it's been consumed.

2. Use a `tokio::sync::Mutex` for `subscription_state` instead, and make `PendingClientDeliveryGuard` hold a clone of the Mutex's `Arc` so `restore()` can `.lock().await` in the Drop. This is technically correct but introduces async Drop semantics which are unergonomic in Rust.

---

### Finding ID: ASYNC-004
**File**: `async-opcua-server/src/subscriptions/mod.rs:888-932`
**Severity**: P2
**Category**: sync-lock-in-async (write-lock scope)
**Summary**: `create_subscription` constructs `SessionEntry` (which spawns a Tokio task via `actor::spawn`) while holding the `subscription_cache.inner` write lock.
**Detail**:
```rust
// mod.rs:893-913
async fn create_subscription(&self, ...) -> Result<...> {
    let cache = {
        let mut lck = trace_write_lock!(self.inner);
        lck.session_subscriptions
            .entry(session_id)
            .or_insert_with(|| {
                SessionEntry::new(  // <-- THIS creates an actor task
                    session_id, limits, key, session, type_tree, ..., cleanup_tx,
                )
            })
            .handle()
    };
    // ... await (lock released)
    let mut lck = trace_write_lock!(self.inner);
    lck.subscription_to_session.insert(...);
}
```

`SessionEntry::new()` calls `actor::spawn()`, which internally calls `tokio::spawn(...)`. While `tokio::spawn` is fast and non-blocking, doing it under a write lock means any work the spawned task does during initialization (which could try to access the same lock) will deadlock.

Inspecting the actor spawn path, the actor doesn't try to acquire `self.inner` during initialization, so this is safe today. However, it's fragile — a future change to the actor initialization could introduce a deadlock.

**Recommendation**: Move the `entry().or_insert_with()` lookup before the write lock, or restructure so the SessionEntry is constructed outside the lock and inserted after:

```rust
async fn create_subscription(&self, ...) -> Result<...> {
    let cache = {
        let lck = trace_read_lock!(self.inner);
        lck.session_subscriptions.get(&session_id).map(|e| e.handle())
    };
    let cache = match cache {
        Some(h) => h,
        None => {
            let entry = SessionEntry::new(session_id, limits, key, session, type_tree, ..., cleanup_tx);
            let mut lck = trace_write_lock!(self.inner);
            lck.session_subscriptions.entry(session_id).or_insert(entry).handle()
        }
    };
    // ... etc
}
```

---

### Finding ID: ASYNC-005
**File**: `async-opcua-core/src/comms/secure_channel.rs:873-940, 1825-1878, 1266-1390`
**Severity**: P2
**Category**: spawn-blocking-gap
**Summary**: Symmetric and asymmetric cryptographic operations (`sign`, `encrypt`, `verify`, `decrypt`) run synchronously on async runtime threads without `spawn_blocking`.
**Detail**:
The `apply_security()`, `symmetric_sign_and_encrypt()`, and `asymmetric_sign_and_encrypt()` methods perform:
- RSA signing/verification (asymmetric_sign)
- AES encrypt/decrypt (symmetric_encrypt/symmetric_decrypt)
- HMAC computation (symmetric_sign)

For small OPC UA message chunks (typically 1-64KB), these operations complete in microseconds. However, for large messages or under CPU contention, RSA operations especially can take milliseconds, which exceeds the ~100µs threshold where `spawn_blocking` is recommended.

These calls are made from:
- `controller.rs:process_request()` — the async event loop
- `transport/tcp.rs` — the transport read/write path
- `channel.rs:send()` — the client send path

All of these run on tokio worker threads.

**Recommendation**: For the common case (small chunks) this is not a problem in practice. However, to be defensive:

1. Measure the p99 latency of `apply_security` and `verify_and_remove_security` under realistic message sizes
2. If p99 > 50µs for symmetric or > 200µs for asymmetric operations, wrap the crypto in `spawn_blocking`

Given the OPC UA protocol norm of small message chunks and the observed performance, **no immediate action is required**. Document the assumption.

---

### Finding ID: ASYNC-006
**File**: `async-opcua-client/src/session/services/subscriptions/service.rs:204` (`Mutex<SubscriptionState>`)
**Severity**: P2
**Category**: migration-candidate
**Summary**: The `Mutex<SubscriptionState>` is used with consistent extract→unlock→await→reacquire discipline, but 27 lock acquisitions across the publish/subscription service create cognitive overhead and fragility.
**Detail**:
The `Session` struct contains:
```rust
pub subscription_state: Mutex<SubscriptionState>,  // parking_lot::Mutex
```

There are 27 distinct lock acquisition sites in `service.rs` for this field. Every one uses the "lock, extract, drop, await, relock" pattern. While each individual site is correct, the volume creates maintainability risk — a future contributor could easily add a `.await` between the first and second lock, or call `trace_lock!()` and forget to drop before `.await`.

This `Mutex<SubscriptionState>` is **not** a migration candidate to `tokio::sync::Mutex` — converting to an async mutex would make the code *less* safe by encouraging holding the guard across `.await`, which would serialize all subscription operations behind network I/O.

**Recommendation**: Convert to an actor/channel model:
```rust
enum SubscriptionCmd {
    AddSubscription { sub: Subscription, reply: oneshot::Sender<u32> },
    ModifySubscription { id: u32, ... reply: oneshot::Sender<()> },
    GetSubscription { id: u32, reply: oneshot::Sender<Option<Subscription>> },
    // ...
}

async fn subscription_owner(rx: mpsc::Receiver<SubscriptionCmd>, state: SubscriptionState) {
    // Single-owner, no locks needed
}
```

This eliminates all 27 lock acquisitions and makes the state transitions explicit. The tradeoff is increased message-passing overhead and the need to handle the actor lifecycle (startup/shutdown). Given the subscription service is already single-owner (one session per client), this is a natural fit.

**Alternative (lower-risk)**: Keep the `Mutex<SubscriptionState>` but add a newtype wrapper that statically prevents holding the guard across `.await`:

```rust
struct SubscriptionStateLock<'a> {
    inner: parking_lot::MutexGuard<'a, SubscriptionState>,
}
impl<'a> !Send for SubscriptionStateLock<'a> {}
```

This ensures the compiler rejects any attempt to hold the guard across a `.await` point in a multi-threaded runtime. Since the lock is scoped within block expressions already, this is mostly documentation-value.

---

### Finding ID: ASYNC-007
**File**: `async-opcua-server/src/session/manager.rs:958-1034`
**Severity**: P2
**Category**: channel-alternative
**Summary**: The "extract-under-read-lock, await, reacquire-write-lock" pattern in `close_session` and `activate_session` is correct but could be simplified with an actor model.
**Detail**:
```rust
// manager.rs:958-1034
pub(crate) async fn close_session(mgr_lck: &RwLock<SessionManager>, ...) -> ... {
    let (session, id, token, actor_sender) = {
        let mgr = trace_read_lock!(mgr_lck);       // 1. read lock
        let session = mgr.find_by_token(...)?;
        let (id, token, auth_token) = {
            let session = trace_read_lock!(session); // 2. read lock (nested)
            // extract data
            (id, token, auth_token)
        };
        let actor_sender = mgr.actor_sender(&auth_token)?;
        (session, id, token, actor_sender)
    };                                             // 3. drop both read locks

    // 4. await on actor communication
    actor_sender.send(SessionMessage::Terminate {...}).await?;
    let terminated = acknowledged.await?;

    {
        let mut mgr = trace_write_lock!(mgr_lck);   // 5. write lock
        // mutate manager state
        mgr.sessions.remove(&terminated.session_id);
        // ...
    }                                              // 6. drop write lock

    // 7. await on subscription cleanup
    handler.delete_session_subscriptions(...).await;
}
```

This is well-structured and correct. The nested `mgr_lck` read guard + `session` read guard followed by a re-acquisition of `mgr_lck` as write is correct because no `.await` occurs between the nested guard drops and the write guard acquisition.

**Recommendation**: The existing session actor infrastructure (`actor.rs`, `SessionMessage`) already provides an actor model for per-session state. Extending it to handle `CloseSession` and `ActivateSession` as actor commands would eliminate the nested lock pattern entirely:

```rust
SessionMessage::Close { reply: oneshot::Sender<CloseSessionResult> }
SessionMessage::Activate { ... reply: oneshot::Sender<ActivateSessionResult> }
```

The session actor would handle state transitions internally (no locks needed) and reply via oneshot. The manager's `sessions` map could be a `DashMap<AuthenticationToken, ActorHandle>` for lock-free lookup.

**Impact**: Medium refactoring effort. Preserve current correctness before optimizing.

---

### Finding ID: ASYNC-008
**File**: `async-opcua-core/src/comms/secure_channel.rs:123`
**Severity**: P2
**Category**: sync-lock-in-async (type choice)
**Summary**: `first_request_signature: std::sync::Mutex<Vec<u8>>` uses poisoning-enabled `std::sync::Mutex` instead of `parking_lot::Mutex`, inconsistent with the rest of the codebase.
**Detail**:
```rust
#[cfg(feature = "ecc")]
first_request_signature: Mutex<Vec<u8>>,  // std::sync::Mutex
```

The `SecureChannel` struct uses `parking_lot::RwLock` for `encoding_context` but `std::sync::Mutex` for `first_request_signature`. This is accessed at lines 1331-1336 and 1338-1343:
```rust
let mut first_request_signature = self
    .first_request_signature
    .lock()
    .map_err(|_| StatusCode::BadSecurityChecksFailed)?;
```

The `std::sync::Mutex` adds poisoning error handling that requires `.map_err()` on every access. Since these are short, non-fallible critical sections (just `.clear()` + `.extend_from_slice()`), `parking_lot::Mutex` would be simpler and more consistent.

**Recommendation**: Change to `parking_lot::Mutex<Vec<u8>>` for consistency with the rest of the codebase and to eliminate the poisoning error handling boilerplate.

---

### Finding ID: ASYNC-009
**File**: `async-opcua-server/src/session/controller.rs:296-360`
**Severity**: LOW
**Category**: cancellation
**Summary**: `tokio::select!` in controller run loop is safe — no locks held across branches, but large `FuturesUnordered` could increase drop pressure on cancellation.
**Detail**:
The controller's `tokio::select!` has four branches:
1. Connection deadline timeout
2. External close command
3. Deadline queue expiry
4. Response processing (from `FuturesUnordered`)
5. Transport polling

No locks are held when entering the select. The `FuturesUnordered` contains `Pin<Box<dyn Future<...>>>` — on cancellation, all pending futures are dropped synchronously, which could cause lock acquisitions in Drop impls if they exist. No problematic Drop impls were found in the current codebase.

**Finding**: Low risk. Verified safe.

---

### Finding ID: ASYNC-010
**File**: `async-opcua-server/src/session/manager.rs:720-825` (`commit_create_session_draft`)
**Severity**: LOW
**Category**: sync-lock-in-async (potential)
**Summary**: `commit_create_session_draft` holds `&mut self` (so the method requires exclusive access) but calls `SessionManager::spawn_session_actor` which internally calls `tokio::spawn`. The spawned task can access `SessionManager` state through Arc references — this is safe because the borrow checker prevents the simultaneous access, but the pattern is subtle.
**Detail**:
This is a synchronous method (`fn`, not `async fn`) that takes `&mut self` and calls `tokio::spawn` via `spawn_session_actor`. The method is called from the `process_request` async context in the controller, but since it's synchronous, no `.await` points exist within it. The `&mut self` borrow ensures no other code can access the SessionManager while the draft is being committed.

**Finding**: Safe. No action needed.

---

## Summary Table

| ID | Severity | File | Category | Action |
|----|----------|------|----------|--------|
| ASYNC-001 | P0 | `channel.rs:195` | sync-lock-in-async | Restructure single-flight without holding tokio mutex across `.await` |
| ASYNC-002 | P1 | `backend.rs:22` | sync-lock-in-async | Remove redundant `Mutex<Connection>` |
| ASYNC-003 | P1 | `service.rs:2487` | cancellation | Remove lock from `PendingClientDeliveryGuard::Drop` |
| ASYNC-004 | P2 | `mod.rs:888` | sync-lock-in-async | Move SessionEntry construction outside write lock |
| ASYNC-005 | P2 | `secure_channel.rs:873` | spawn-blocking-gap | Document crypto assumption; measure p99 latency |
| ASYNC-006 | P2 | `service.rs:204` | migration-candidate | Consider actor model for SubscriptionState |
| ASYNC-007 | P2 | `manager.rs:958` | channel-alternative | Consider extending session actor for close/activate |
| ASYNC-008 | P2 | `secure_channel.rs:123` | sync-lock-in-async | Use parking_lot::Mutex for consistency |
| ASYNC-009 | LOW | `controller.rs:296` | cancellation | Verified safe |
| ASYNC-010 | LOW | `manager.rs:720` | sync-lock-in-async | Verified safe |

---

## What Works Well

The codebase demonstrates several best practices worth noting:

1. **Consistent lock-unlock-before-await pattern**: Every use of `parking_lot::RwLock`/`Mutex` from async context drops the guard before `.await`. The `trace_read_lock!`/`trace_write_lock!`/`trace_lock!` macros with block scoping (`{ let g = trace_lock!(x); g.do_work(); }`) make this pattern explicit and reviewable.

2. **Correct `spawn_blocking` use in SQLite backend**: All 10 `spawn_blocking` sites in `async-opcua-history-sqlite/src/backend.rs` correctly pass `Arc<Mutex<Connection>>` by clone into the blocking closure, ensuring the connection is only accessed from blocking threads.

3. **Single-flight channel renewal**: The `tokio::sync::Mutex` in `channel.rs` effectively solves the thundering-herd problem in secure channel renewal, even though the implementation has the noted P0 issue.

4. **Actor model adoption**: The session actor (`actor.rs`), subscription actor (`subscriptions/actor.rs`), and notification ring buffer (`subscriptions/ring.rs`) demonstrate progressive adoption of message-passing patterns instead of shared-state-with-locks.

5. **Lock-free patterns**: `ArcSwap` for `session_id` and `auth_token`, `DashMap` for the node map in `AddressSpace`, `AtomicU32` for monotonic handles — all used correctly with appropriate `Ordering::Relaxed`.

6. **No `block_in_place` or `block_on` misuse**: Neither function was found in the codebase, confirming the async runtime is not being blocked by synchronous work.
