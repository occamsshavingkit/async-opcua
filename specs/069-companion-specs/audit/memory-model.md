# Memory Model / Atomic Ordering Audit

**Audit date**: 2026-07-07
**Scope**: Entire workspace (`async-opcua`, `async-opcua-core`, `async-opcua-client`, `async-opcua-server`, `async-opcua-fx`, `async-opcua-nodes`, `async-opcua-types`)
**Methodology**: First pass — exhaustive scan of all `Ordering::`, `ArcSwap`, `AtomicBool/U64/Usize/I64/I32/U32/U16/U8/Ptr`, `compare_exchange`, and `fence` sites. Second pass — deep happens-before trace through all 5 ArcSwap sites, all Atomib* usage in production session/transport/subscription paths, and the compare_exchange loop.

---

## Executive Summary

**No P0 (unsound/data corruption) findings.** No memory ordering bugs that would cause crashes, miscompilation, or silent data corruption.

**Key observation**: The codebase correctly uses external synchronization (tokio channels, mutexes, watch channels) as the primary happens-before mechanism. Atomic operations are overwhelmingly used for statistical counters and simple flags, where Relaxed ordering is correct. The 5 `ArcSwap` sites default to `SeqCst`, providing safe but potentially over-strong ordering.

**Main risks**: One fragile pattern where `Relaxed` relies on channel-internal synchronization for correctness (`should_reconnect`); one acknowledged non-atomic read-modify-write (`client_offset`); one temporal TOCTOU in session timeout validation.

---

## First Pass: Complete Atomic Inventory

### ArcSwap sites (5 total)

| # | Field | Type | Location | Writers | Readers |
|---|-------|------|----------|---------|---------|
| 1 | `session_id` | `Arc<ArcSwap<NodeId>>` | `client/src/session/mod.rs:193` | `create_session` response (services/session.rs:892), `reset()` (mod.rs:309) | `connect.rs:87,106`, `mod.rs:341` |
| 2 | `client_offset` | `ArcSwap<chrono::Duration>` | `client/src/transport/state.rs:32` | `set_client_offset()` (state.rs:173-178) | `make_request_header()` (state.rs:219,239) |
| 3 | `authentication_token` | `Arc<ArcSwap<NodeId>>` | `client/src/transport/state.rs:38` | `set_auth_token()` (state.rs:252) | `make_request_header()` (state.rs:238) |
| 4 | `request_send` | `ArcSwapOption<RequestSend>` | `client/src/transport/channel.rs:48` | `connect()` (channel.rs:261), `connect_no_retry()` (channel.rs:323) | `send()` (channel.rs:230), `close_channel()` (channel.rs:397) |
| 5 | `last_service_request` | `ArcSwap<Instant>` | `server/src/session/instance.rs:113` | `validate_timed_out()` (instance.rs:227) | `validate_timed_out()` (instance.rs:225), `deadline()` (instance.rs:243) |

All 5 use ArcSwap's **default ordering** (`load()` = `SeqCst`, `store()` = `SeqCst`). No explicit ordering parameters passed.

### `Ordering::` usage by variant

| Ordering | Count | Used in |
|----------|-------|---------|
| `Relaxed` | ~130+ | Vast majority: metrics, counters, test helpers, flags, `should_reconnect`, `AtomicHandle`, `TRACE_LOCKS_STATE`, `ServerDiagnostics::enabled`, `port`, `service_level`, `next_session_id` |
| `SeqCst` | ~20 | Test coordination (hostile_server, renewal_singleflight, subscription_delivery_locks), subscription service tests, ArcSwap (implicit via crate defaults) |
| `Release` | 6 | `unactivated_by_channel` fetch_sub (×4), `AuthenticationGate::pause_once` store, `AuthenticationGate::called` store |
| `Acquire` | 3 | `unactivated_by_channel` load, `AuthenticationGate::pause_once` load, `AuthenticationGate::called` load |
| `AcqRel` | 1 | `AuthenticationGate::pause_once` swap |

### `Atomic*` type inventory (production code only)

| Type | Count | Location | Usage |
|------|-------|----------|-------|
| `AtomicU32` | 3 | `client/src/session/mod.rs` (static + field), `server/src/info.rs` (field) | `NEXT_SESSION_ID`, `internal_session_id`, `next_session_id` |
| `AtomicBool` | 3 | `client/src/session/mod.rs` (field), `server/src/diagnostics/server.rs` (field), `server/src/session/manager.rs` (field) | `should_reconnect`, `enabled`, `unactivated_by_channel` counter type |
| `AtomicU8` | 2 | `core/src/lib.rs` (static), `server/src/server_handle.rs` / `server/src/info.rs` | `TRACE_LOCKS_STATE`, `service_level` |
| `AtomicU16` | 2 | `server/src/info.rs` (field + static) | `port` |
| `AtomicU64` | 13 | `server/src/metrics.rs` (×12), `server/src/subscriptions/actor.rs` | Server metrics counters, `dropped` notification counter |
| `AtomicUsize` | ~4 | `server/src/metrics.rs` (×1), `server/src/session/manager.rs` (field), `server/src/subscriptions/subscription.rs` (test) | `actor_queue_peak_depth`, `unactivated_by_channel` |
| `AtomicHandle` | 2 | `client/src/transport/state.rs`, `client/src/session/mod.rs` | Request handle generator |

### `compare_exchange` loops

Only one in production: `AtomicHandle::next()` at `core/src/handle.rs:77`. Uses `Relaxed` for both success and failure ordering. The loop is bounded (exits after successful CAS or state transition). Never spins indefinitely unless `first` is set unreasonably.

No `compare_exchange_weak` in production; one use in test code (`async-opcua-types/src/tests/encoding.rs`).

### Fence usage

**Zero.** No explicit `fence(Acquire)` or `fence(Release)` anywhere in the workspace.

---

## Second Pass: Hapens-Before Deep Trace

### Finding MEM-001

**File**: `async-opcua-client/src/transport/state.rs:173-178`
**Severity**: P1
**Category**: happened-before
**Summary**: Non-atomic read-modify-write on `client_offset` ArcSwap; comment acknowledges it is "not strictly speaking thread safe"

**Detail**:
```rust
// state.rs:173-178
pub(super) fn set_client_offset(&self, offset: chrono::Duration) {
    // This is not strictly speaking thread safe, but it doesn't really matter in this case,
    // the assumption is that this is only called from a single thread at once.
    self.client_offset
        .store(Arc::new(**self.client_offset.load() + offset));
    debug!("Client offset set to {}", self.client_offset);
}
```

**Load-store interleaving analysis**: Between the `load()` and `store()`, the value cannot change due to concurrent write because `set_client_offset()` is only called from `end_issue_or_renew_secure_channel()`, which is reached via:
- `connect_no_retry()` — single-shot per connection attempt
- `renew_secure_channel()` — guarded by `self.issue_channel_lock` (tokio Mutex, line 195 of channel.rs)

The `issue_channel_lock` serializes all calls. However, an unsuspecting future maintainer adding another call site would introduce a lost-update bug. The comment is a warning that was left in place rather than fixed with a proper atomic `fetch_update`.

**Happens-before**: Writers are serialized by `issue_channel_lock`. Readers (`make_request_header()`) get SeqCst from ArcSwap. The load-store gap is safe ONLY because of the external lock.

**Recommendation**: Replace with a `fetch_update` or document the lock dependency more explicitly.

---

### Finding MEM-002

**File**: `async-opcua-server/src/session/instance.rs:224-228`
**Severity**: P1
**Category**: happened-before
**Summary**: Session timeout validation has a read-then-write gap on `last_service_request`

**Detail**:
```rust
// instance.rs:224-228
pub(crate) fn validate_timed_out(&self) -> Result<(), StatusCode> {
    let elapsed = Instant::now() - **self.last_service_request.load();
    self.last_service_request.store(Arc::new(Instant::now()));

    if self.session_timeout < elapsed {
        // ...
        Err(StatusCode::BadSessionIdInvalid)
    } else {
        Ok(())
    }
}
```

**TOCTOU window**: Between line 225 (load) and line 227 (store), another concurrent call to `validate_timed_out()` reads the same old timestamp and computes nearly the same elapsed time. Both calls then overwrite the stored value with slightly different `Instant::now()` values.

**Impact**: Low/None. The worst case is two concurrent calls both see a slightly older timestamp, making timeout detection marginally more sensitive. The store order doesn't matter — whichever Instant was later is what matters for the NEXT timeout check, and both are "now" within a microsecond. The `deadline()` method reads `last_service_request` independently and returns `last + timeout`, which may be slightly stale but is a point-in-time estimate regardless.

**Happens-before**: Both load and store use ArcSwap default SeqCst. The inter-call ordering is total (SeqCst). The only issue is the atomicity of the two-operation sequence (load + compute + store), not the ordering of individual operations.

**Recommendation**: Accept as is. The semantics are correct for timeout management. The comment-free code is the only concern — document the non-atomicity of the check-and-reset.

---

### Finding MEM-003

**File**: `async-opcua-client/src/session/mod.rs:362-381` (writer) / `async-opcua-client/src/session/event_loop.rs:161` (reader)
**Severity**: P2
**Category**: ordering
**Summary**: `should_reconnect` flag uses Relaxed ordering on both sides; correctness depends on mpsc channel providing happens-before

**Detail**:
```rust
// Writer — mod.rs:363
pub fn disable_reconnects(&self) {
    self.should_reconnect.store(false, Ordering::Relaxed);
}

// Writer — mod.rs:381 (inside disconnect_inner)
if disable_reconnect {
    self.should_reconnect.store(false, Ordering::Relaxed);
}

// Reader — event_loop.rs:161 (inside Connected state's select! branch)
TransportPollResult::Closed(code) => {
    let should_reconnect = slf.inner.should_reconnect.load(Ordering::Relaxed);
    if !should_reconnect {
        return Ok(None); // stop event loop
    }
    // ... transition to Disconnected → Connecting
}
```

**Happens-before chain for `disconnect_inner` path**:
```
store(false, Relaxed)     (line 381)
  → program order
close_channel().await      (line 391) — sends CloseSecureChannel through mpsc
  → mpsc channel internal synchronization (tokio mpsc uses internal atomics with at least Acquire/Release)
transport processes + closes
  → program order (transport task)
TransportPollResult::Closed emitted
  → event loop program order
load(Relaxed)              (line 161) — sees false
```

The mpsc channel provides the happens-before edge. Both Relaxed operations are correct because an intervening synchronized send/recv pair establishes the ordering.

**Risk for `disable_reconnects()` standalone** (line 362-363): If called while the event loop is CONNECTED and the transport hasn't yet disconnected, the Relaxed store has no external synchronization to guarantee visibility. However, the reader only checks `should_reconnect` AFTER `TransportPollResult::Closed`, which itself is preceded by channel operations. So the flag will be visible when it matters.

**Recommendation**: This is currently correct but fragile. A future refactor that changes the disconnect notification path could break the happens-before chain. Consider documenting the implicit ordering dependency, or upgrade to `Release`/`Acquire` for defense-in-depth (cost: one memory barrier on ARM).

---

### Finding MEM-004

**File**: `async-opcua-core/src/handle.rs:72-94`
**Severity**: P2
**Category**: relaxed-misuse (theoretical)
**Summary**: `AtomicHandle::next()` overflow-path compare_exchange loop uses Relaxed for all operations

**Detail**:
```rust
pub fn next(&self) -> u32 {
    let mut val = self.next.fetch_add(1, Ordering::Relaxed);
    while val < self.first {
        match self.next.compare_exchange(
            val + 1,
            self.first + 1,
            Ordering::Relaxed,  // success
            Ordering::Relaxed,  // failure
        ) {
            Ok(_) => val = self.first,
            Err(v) => {
                if v >= self.first {
                    val = self.next.fetch_add(1, Ordering::Relaxed);
                } else {
                    val = v;
                }
            }
        }
    }
    val
}
```

**Overflow analysis**: `self.first` is typically small (0 or 1). The `while val < self.first` condition is only true when the counter wraps around from `u32::MAX` to 0. This takes ~4 billion `next()` calls between wraps — essentially never in practice.

**Liveness**: The loop uses `compare_exchange` with Relaxed on both rails. The CAS may fail due to spurious failures (not possible on x86, but possible on ARM/RISC-V with the weak variant — but `compare_exchange` is the strong variant, so no spurious failure). The CAS only fails when another thread concurrently modified `self.next`. In the error branch, the code either does another `fetch_add` and retries, or takes the new value. This is bounded — at most 1-2 iterations.

**Ordering**: Handles are used as opaque identifiers. Readers only care about uniqueness, not ordering relative to other memory operations. Relaxed is correct here.

**Recommendation**: No action needed. The overflow path is theoretically reachable but practically never exercised. The logic is correct with Relaxed ordering.

---

### Finding MEM-005

**File**: `async-opcua-client/src/session/mod.rs:308-313`
**Severity**: P2
**Category**: happened-before
**Summary**: `reset()` updates `session_id` (ArcSwap SeqCst) and `internal_session_id` (AtomicU32 Relaxed) in separate operations

**Detail**:
```rust
pub(crate) fn reset(&self) {
    self.session_id.store(Arc::new(NodeId::null()));
    self.internal_session_id.store(
        NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
        Ordering::Relaxed,
    );
}
```

**Inconsistency window**: Between the two stores, a reader calling `session_id()` (returns `internal_session_id`) and `server_session_id()` (returns the ArcSwap value) will see NULL session_id with OLD internal_session_id. The inconsistency lasts for ~nanoseconds.

Since `reset()` is called from `SessionConnector::try_connect()` which runs in the session event loop (single task), the only concurrent readers are external API calls. The inconsistency is harmless — it represents a session in transition.

**Recommendation**: Accept as is. The window is minuscule and the values are used as identifiers, not for synchronization.

---

### Finding MEM-006

**File**: `async-opcua-server/src/server.rs:974,1006,1038` (writer) / `async-opcua-server/src/discovery/mdns.rs:195`, `async-opcua-server/src/info.rs:944` (reader)
**Severity**: P2
**Category**: ordering
**Summary**: `port: AtomicU16` written and read with Relaxed; no happens-before required but written once/propagated through discovery

**Detail**:
```rust
// Writer — server.rs:974
self.info.port.store(addr.port(), Ordering::Relaxed);

// Readers — discovery/mdns.rs:195, info.rs:944
let port = info.port.load(Ordering::Relaxed);
self.port.load(Ordering::Relaxed)
```

The port is set once before the server begins accepting connections, and readers access it later via discovery APIs. The tokio task spawn that starts the server provides the happens-before edge (task spawn is synchronized). Relaxed is correct.

**Same applies to**: `service_level: Arc<AtomicU8>` — written by `ServerHandle::set_service_level()` (server_handle.rs:94, Relaxed), read by node_manager (memory/core.rs:987, Relaxed). These are user-triggered reads/writes with no ordering requirement between them.

**Recommendation**: No action. Relaxed is correct for these non-coordinating values.

---

### Finding MEM-007

**File**: `async-opcua-core/src/lib.rs:94-112`
**Severity**: P2
**Category**: relaxed-misuse (benign)
**Summary**: `TRACE_LOCKS_STATE` double-check pattern with Relaxed may cause redundant environment variable reads

**Detail**:
```rust
static TRACE_LOCKS_STATE: AtomicU8 = AtomicU8::new(TRACE_LOCKS_UNKNOWN);

pub fn trace_locks() -> bool {
    match TRACE_LOCKS_STATE.load(Ordering::Relaxed) {
        TRACE_LOCKS_ENABLED => return true,
        TRACE_LOCKS_DISABLED => return false,
        _ => {}
    }
    let state = match std::env::var("OPCUA_TRACE_LOCKS") {
        Ok(s) if s != "0" => TRACE_LOCKS_ENABLED,
        _ => TRACE_LOCKS_DISABLED,
    };
    TRACE_LOCKS_STATE.store(state, Ordering::Relaxed);
    state == TRACE_LOCKS_ENABLED
}
```

This is a lazy-initialization cache. With Relaxed ordering on all paths, concurrent calls may all observe `TRACE_LOCKS_UNKNOWN` and redundantly call `std::env::var()`. This is benign — `std::env::var()` is idempotent and the store winner correctly sets the cache.

**Recommendation**: No action. The pattern is intentionally lock-free and the redundant reads are harmless. If anything, `Relaxed` is correct here because the worst case is a few extra env var lookups during startup.

---

### Finding MEM-008

**File**: `async-opcua-server/src/session/manager.rs:547,759-760,770-801,870-874,1005-1007,1309-1310`
**Severity**: P2 (positive finding)
**Category**: ordering
**Summary**: `unactivated_by_channel` counter uses correctly paired Release/Acquire ordering for operational limits enforcement

**Detail**:
```rust
// Increment on session creation — manager.rs:802
counter.fetch_add(1, Ordering::Release);

// Decrement on session expiry/close/activation — manager.rs:760,874,1007,1310
counter.fetch_sub(1, Ordering::Release);

// Read to check limit — manager.rs:771
counter.load(Ordering::Acquire);
```

**Happens-before chain**:
```
fetch_add(1, Release)        [create session]
  → synchronizes-with
load(Acquire)                  [check limit on next create]
  → synchronizes-with
fetch_sub(1, Release)          [session expires]
  → synchronizes-with
load(Acquire)                  [check limit — sees decremented count]
```

The Release stores on `fetch_add`/`fetch_sub` pair with the Acquire load, ensuring the limit check always sees a count consistent with the actual session creation/expiry ordering. This is the **only correctly paired Acquire/Release usage in the entire production codebase**.

**Recommendation**: No action. This is the gold standard for how atomics should be used for operational correctness. Consider this a model for any future atomic-based enforcement.

---

### Finding MEM-009

**File**: `async-opcua-client/src/transport/channel.rs:261,323,230,397`
**Severity**: P2
**Category**: publication
**Summary**: `request_send: ArcSwapOption<RequestSend>` publication ordering is correct via SeqCst (ArcSwap default)

**Detail**:
```rust
// Clear before reconnect attempt — channel.rs:261
self.request_send.store(None);

// Publish new sender after successful connect — channel.rs:323
self.request_send.store(Some(Arc::new(send)));

// Read in send path — channel.rs:230
let sender = self.request_send.load().as_deref().cloned();

// Read in close path — channel.rs:397
let sender = self.request_send.load().as_deref().cloned();
```

**Publication safety**: The `Arc::new(send)` is constructed before the `store(Some(...))`. With SeqCst ordering, any thread that observes the `Some` variant will see the fully-constructed `Arc<Sender>`. This is correct.

**Stale sender**: The `send()` path clones the `Sender` from the Arc. If the transport subsequently disconnects and stores `None`, the cloned sender still holds a reference to the mpsc channel. Messages sent through the stale sender will either succeed (if the receiver still exists) or fail with an error. Both outcomes are benign — the caller gets an error and can retry.

**Recommendation**: No action. Correct as-is.

---

### Finding MEM-010

**File**: `async-opcua-client/src/transport/state.rs:236-244`
**Severity**: P2
**Category**: ordering
**Summary**: `make_request_header()` reads `authentication_token` and `client_offset` from two independent ArcSwap sites; no atomic consistency between them

**Detail**:
```rust
pub(super) fn make_request_header(&self, timeout: Duration) -> RequestHeader {
    RequestHeader {
        authentication_token: self.authentication_token.load().as_ref().clone(),  // line 238
        timestamp: DateTime::now_with_offset(**self.client_offset.load()),         // line 239
        // ...
    }
}
```

**Analysis**: These are two independent values — the token identifies the session, the offset corrects for clock skew. There is no invariant requiring them to be read atomically together. Even if the token is updated between the two loads, the resulting header is valid: either old-token+new-offset or new-token+old-offset. A request header is a snapshot anyway.

**Recommendation**: No action. Independent values, no invariant violation.

---

### Finding MEM-011

**File**: `async-opcua-server/src/session/manager.rs:2392-2433` (test code)
**Severity**: P2 (positive finding)
**Category**: ordering
**Summary**: `AuthenticationGate` test utility uses correctly paired Release/Acquire/AcqRel for synchronization primitive

**Detail**:
```rust
fn pause_next_authentication(&self) {
    self.pause_once.store(true, Ordering::Release);            // Release
}

async fn maybe_pause(&self) {
    if self.pause_once.swap(false, Ordering::AcqRel) {        // AcqRel
        self.entered.notify_waiters();
        self.release.notified().await;
    }
}

async fn wait_until_entered(&self) {
    if self.pause_once.load(Ordering::Acquire) {               // Acquire
        self.entered.notified().await;
    }
}

fn was_called(&self) -> bool {
    self.called.load(Ordering::Acquire)                         // Acquire
}

// Writer:
self.called.store(true, Ordering::Release);                    // Release
```

**Happens-before**:
```
store(true, Release) on pause_once   [pause_next_authentication]
  → synchronizes-with
load(Acquire) / swap(AcqRel)          [wait_until_entered / maybe_pause]

store(true, Release) on called       [authenticate]
  → synchronizes-with
load(Acquire)                         [was_called]
```

All pairs are correctly matched. This is a well-implemented lock-free notification gate.

**Recommendation**: No action. Consider extracting this pattern as a reusable primitive if similar coordination is needed elsewhere.

---

### Finding MEM-012

**File**: `async-opcua-server/src/metrics.rs:87-166`
**Severity**: P2 (positive finding)
**Category**: relaxed-misuse (not misuse)
**Summary**: All `ServerMetrics` counters use Relaxed — correct for statistical/monitoring counters

**Detail**: 13 separate `AtomicU64` fields and 1 `AtomicUsize` field, all operated with `fetch_add(Relaxed)`, `fetch_sub(Relaxed)`, and `load(Relaxed)`.

**Rationale**: These are all monotonic counters and gauges for observability. No algorithm depends on their ordering relative to other memory operations. Two concurrent increments may be reordered, but both will eventually be visible — which is the contract for statistical counters. The `snapshot()` method reads all counters independently (no atomic snapshot across counters), which is acceptable for metrics.

**Recommendation**: No action. Relaxed is the correct choice for metrics counters.

---

### Finding MEM-013

**File**: All 5 ArcSwap sites (see table above)
**Severity**: P2 (observation)
**Category**: ordering
**Summary**: All ArcSwap operations use default SeqCst ordering — correct but possibly stronger than needed

**Detail**: The `arc_swap` crate defaults to `SeqCst` for `load()` and `store()` when no explicit ordering is specified. At all 5 sites, the operations are:
- `session_id`: SeqCst load/store. The reader in `connect.rs:87` checks `is_null()` to decide whether to create a session. Acq/Rel would suffice since the write comes from the session event loop's internal task.
- `client_offset`: SeqCst load/store. Since writes are serialized by `issue_channel_lock`, Relaxed store + Acquire load would suffice for the reader (`make_request_header`).
- `authentication_token`: SeqCst load/store. The token is read independently of other state. Acq/Rel would suffice.
- `request_send`: SeqCst load/store. The transport state machine needs full ordering between `store(None)` and `store(Some(...))`. SeqCst is appropriate here to ensure any thread sees the full state transition.
- `last_service_request`: SeqCst load/store. The load in `deadline()` is independent; the store in `validate_timed_out()` immediately follows. Acq/Rel would suffice.

**Performance note**: On x86, SeqCst imposes `mfence` instructions which are ~20-100 cycles. On ARM, SeqCst is equivalent to AcqRel (no additional cost vs Acq/Rel pairs). Since async-opcua targets server environments, the overhead of SeqCst on x86 is negligible relative to network I/O latency.

**Recommendation**: No action. SeqCst is not wrong, just conservatively stronger than necessary. Future optimization could reduce `session_id`, `client_offset`, `authentication_token`, and `last_service_request` to `Acquire` loads + `Release` stores if profiling shows atomic overhead.

---

## Summary Table

| ID | File | Line(s) | Severity | Category | Action |
|----|------|---------|----------|----------|--------|
| MEM-001 | transport/state.rs | 173-178 | P1 | happened-before | Document lock dependency; consider `fetch_update` |
| MEM-002 | session/instance.rs | 224-228 | P1 | happened-before | Document non-atomic check-and-reset; accept |
| MEM-003 | session/mod.rs + event_loop.rs | 362-381, 161 | P2 | ordering | Document implicit channel synchronization |
| MEM-004 | core/handle.rs | 72-94 | P2 | relaxed-misuse | Accept; verified bounded |
| MEM-005 | session/mod.rs | 308-313 | P2 | happened-before | Accept; benign window |
| MEM-006 | server.rs + discovery/ | 974,195 | P2 | ordering | Accept; single-writer |
| MEM-007 | core/lib.rs | 94-112 | P2 | relaxed-misuse | Accept; benign redundant reads |
| MEM-008 | session/manager.rs | 547-1310 | P2\(\*\) | ordering | No action; exemplar of correct Acquire/Release |
| MEM-009 | transport/channel.rs | 261-323 | P2 | publication | Accept; correct |
| MEM-010 | transport/state.rs | 236-244 | P2 | ordering | Accept; independent values |
| MEM-011 | session/manager.rs | 2392-2433 | P2\(\*\) | ordering | No action; well-implemented test utility |
| MEM-012 | server/metrics.rs | 87-166 | P2\(\*\) | relaxed-misuse | No action; correct for stats |
| MEM-013 | (5 ArcSwap sites) | — | P2 | ordering | No action; SeqCst correct, conservatively safe |

\(\*\)Positive finding — correctly implemented.

---

## What Was NOT Found

- No `fence()` calls anywhere — none needed
- No unsound `Relaxed` usage that would cause data corruption on weakly-ordered architectures
- No `lease()` calls on ArcSwap — no guard lifetime issues
- No `compare_exchange` loops that could spin indefinitely
- No mismatched Release/Acquire pairs (lone Release with no Acquire reader, or vice versa)
- No unsynchronized publication of objects through atomics
