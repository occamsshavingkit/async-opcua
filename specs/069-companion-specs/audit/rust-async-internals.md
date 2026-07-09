# Rust Async Internals Audit — async-opcua

**Date**: 2026-07-07
**Methodology**: Two-pass audit at the Future/poll/waker level.
**Status**: Read-only. No source modifications.

---

## Finding ID: INTERN-001

**File**: `async-opcua-core/src/comms/secure_channel.rs:1266-1395`
**Severity**: P0
**Category**: crypto-on-async
**Summary**: `asymmetric_sign_and_encrypt` performs RSA signing + encryption on the tokio worker thread with no `spawn_blocking` wrapper.
**Detail**:
This function is called from `apply_security` (line 873), which runs during the transport send path in the connection's tokio task (the `SessionController::run` loop). It performs:
- RSA-2048 PKCS#1 v1.5 signing (~2–5ms for sign, ~0.1–1ms for encrypt) or RSA-OAEP (~5–10ms)
- ECC P-256 signing (~1–2ms)

Since the transport send path runs inline in the tokio task (no separate I/O task), each encrypted OpenSecureChannel response message blocks the worker thread for the duration of the crypto operation. Under concurrent connection setup (many clients opening secure channels simultaneously), this can stall the tokio runtime's cooperative scheduler.

**Mitigation**: Wrap `asymmetric_sign_and_encrypt` and the symmetric encrypt path in `tokio::task::spawn_blocking` at the call site in `apply_security`, which may require refactoring the chunk encoding pipeline to be async-aware.

---

## Finding ID: INTERN-002

**File**: `async-opcua-core/src/comms/secure_channel.rs:1502-1660`
**Severity**: P0
**Category**: crypto-on-async
**Summary**: `asymmetric_decrypt_and_verify` performs RSA/ECC decryption + signature verification on the tokio worker thread.
**Detail**:
Called from `decrypt_open_secure_channel` (line 970), which is invoked by `verify_and_remove_security` (line 1174), which is called in the transport read path (`process_message` in `tcp.rs:541`). The transport read path runs in the tokio task's poll loop. RSA-2048 private-key decryption is the expensive part here:
- RSA-2048 OAEP decryption: ~5–20ms
- ECC P-256 verification: ~1–3ms

Every OpenSecureChannel request arriving at the server blocks the worker thread for this duration. Under connection storms (many simultaneous clients), the cumulative effect can cause request latency spikes across all connections on the same runtime worker.

**Mitigation**: Same as INTERN-001 — wrap the asymmetric crypto path in `spawn_blocking`. For the `verify_and_remove_security / verify_and_remove_security_server` call sites, the read path would need to become async-aware.

---

## Finding ID: INTERN-003

**File**: `async-opcua-server/src/session/manager.rs:1046-1130`
**Severity**: P0
**Category**: long-scope
**Summary**: `activate_session` holds a long read lock on `SessionManager` (84 lines) while performing signature verification and other work.
**Detail**:
The read lock at line 1046-1047 (`let mgr = trace_read_lock!(mgr_lck);`) is held for 84 lines until line 1130. Inside this scope:
1. A session read lock is taken (1053-1128) — this acquires a `parking_lot::RwLock` read guard on the session
2. Inside the session read lock, `verify_client_signature` (1096-1101) is called, which performs RSA signature verification over the client nonce. **This is a blocking crypto operation** (RSA-2048 verify: ~0.1–2ms).
3. The manager read lock is released at 1130, before the `.await` at 1164.

While the manager read lock itself is not contended (it's a read lock and other readers can proceed concurrently), the operation is substantially longer than the intended microsecond-scale lock scope. The signature verification (1101) is a crypto operation on the async thread. Additionally, if a write lock is queued waiting for this read lock to be released, all other read lock attempts will be queued behind the writer due to `parking_lot`'s fairness.

**Timing estimate**: Microseconds to low milliseconds per call (the signature verification dominates). Under concurrent ActivateSession calls, the write lock queuing effect could push latency into millisecond territory.

**Mitigation**: Perform `verify_client_signature` after dropping the manager read lock (extract the required fields first, then verify with cleanup lock acquisition). This would require restructuring the function to extract `security_policy`, `info`, `session`, and `client_signature` fields before the verification call.

---

## Finding ID: INTERN-004

**File**: `async-opcua-server/src/session/manager.rs:923-946`
**Severity**: P1
**Category**: long-scope
**Summary**: `check_session_expiry` iterates all live sessions while holding read locks on each session, called synchronously from the expiry timer.
**Detail**:
This function iterates over all entries in `self.sessions` (a `HashMap<NodeId, Arc<RwLock<Session>>>`), and for each session:
1. Acquires a read lock (`session.read()` at line 928)
2. Computes `deadline` (calls `session.deadline()`, checks `session.is_activated()`, `session.created_at()`)
3. Holds the read lock until the end of the loop body

For N active sessions, this is N sequential `parking_lot` read lock acquisitions. Each read lock acquisition is fast (atomic CAS), but under high session count (e.g., 10,000 sessions), this becomes O(N) sequential work on a single tokio worker. The function is called from `check_session_expiry` which is invoked periodically (likely on a timer). If this blocks the timer's worker thread, other timers could be delayed.

**Timing estimate**: For N=100 sessions: ~10–50µs. For N=10,000 sessions: ~1–5ms.

**Mitigation**: If session counts grow beyond ~1,000, consider maintaining a separate expiry heap (e.g., `BinaryHeap` of `(Instant, NodeId)`) for O(log N) expiry checking instead of O(N) scanning.

---

## Finding ID: INTERN-005

**File**: `async-opcua-history-sqlite/src/backend.rs:23-24, 299-301`
**Severity**: P1
**Category**: contention-in-blocking
**Summary**: Single `Mutex<Connection>` shared across all `spawn_blocking` history operations creates a serialization bottleneck under concurrent load.
**Detail**:
The `SqliteHistoryBackend` wraps a single `Arc<Mutex<Connection>>`. All 10 `spawn_blocking` call sites clone the `Arc` before moving into the closure, then call `conn.lock()` to acquire the Mutex. This is correct for isolation, but:

1. All concurrent history reads, writes, and updates serialize on the same SQLite connection.
2. If one `spawn_blocking` task holds the connection Mutex (e.g., a long `update_data` transaction with many values), all other history operations queued in the blocking thread pool will stall inside `conn.lock()`.
3. SQLite in WAL mode allows concurrent reads, but the `Mutex` prevents any concurrency — the second reader blocks on the Mutex even though SQLite could handle it.

**Timing estimate**: Mutex acquisition is ~50ns uncontended. Under contention, the blocked thread parks (OS-level sleep) inside `spawn_blocking`, consuming a blocking pool thread. If all blocking pool threads are consumed waiting on the Mutex, additional `spawn_blocking` calls will queue (tokio default: 512 threads, so this is unlikely to be hit but still wasteful).

**Mitigation**: Use a connection pool (e.g., `r2d2-sqlite` or `deadpool-sqlite`) or separate read/write connections. This is especially important for `read_raw_modified` which is the most common operation.

---

## Finding ID: INTERN-006

**File**: `async-opcua-server/src/subscriptions/mod.rs:894-913`
**Severity**: P1
**Category**: long-scope
**Summary**: `create_subscription` holds a write lock on `SubscriptionCacheInner` while constructing a `SessionEntry` (which spawns a tokio actor).
**Detail**:
The write lock at line 894 (`let mut lck = trace_write_lock!(self.inner);`) is held while:
1. `SessionEntry::new()` is called (898-911)
2. `SessionEntry::new()` calls `Self::get_key(&context.session)` which acquires a session read lock (line 1448)
3. `SessionEntry::new()` creates a `SessionSubscriptions` and calls `actor::spawn()` which does `tokio::spawn`

The session read lock inside `get_key()` is brief (just field reads). The `tokio::spawn` call is non-blocking (it just queues the task). The `SessionSubscriptions::new()` construction is also CPU-only (data structure initialization). Total hold time: microseconds.

However, the write lock on `SubscriptionCacheInner` blocks ALL concurrent subscription operations (create_monitored_items, delete_subscriptions, data_route_snapshot, etc.) for the duration. Under heavy subscription creation (e.g., many clients creating subscriptions simultaneously), the write lock becomes a hotspot.

**Timing estimate**: 10–50µs per `create_subscription`. Under serialized access (write lock), N concurrent creates = N × 50µs of sequential blocking.

**Mitigation**: Defer the write lock to only the final insertion step. Pre-construct the `SessionEntry` (with the actor) outside the lock, then acquire the write lock solely for the `session_subscriptions.entry(session_id).or_insert_with()` call. The `SessionEntry` construction reads immutable data from the `SubscriptionCache` (e.g., `self.cleanup_tx`) but does not mutate `SubscriptionCacheInner`.

---

## Finding ID: INTERN-007

**File**: `async-opcua-server/src/subscriptions/mod.rs:1063-1070`
**Severity**: P2
**Category**: long-scope
**Summary**: `data_route_snapshot` acquires a read lock on `SubscriptionCacheInner` during synchronous notification dispatch.
**Detail**:
The `data_route_snapshot` function is called from:
- `notify_data_change` (line 1133) — synchronous function
- `maybe_notify` (line 1187) — synchronous function
- `notify_for` (line 310) — synchronous function

Each call acquires a `trace_read_lock!(self.inner)` and iterates through `lck.monitored_items` to build a `NotificationRouteSnapshot`. The snapshot construction involves HashMap lookups (O(1) per key) and vector allocation. For a node with many monitored items (e.g., 10,000 subscriptions all monitoring the same ServerStatus node), the iteration could be O(items).

**Timing estimate**: For a heavily monitored node with 1,000 monitored items: ~50–200µs. For 10,000 items: ~0.5–2ms. This is worst-case — typical nodes have 1–10 monitored items (<1µs).

**Mitigation**: If server-wide nodes accumulate thousands of monitored items, consider sharding the `monitored_items` index or using a concurrent hash map to avoid holding a global read lock during snapshot construction.

---

## Finding ID: INTERN-008

**File**: `async-opcua-server/src/session/manager.rs:637-662`
**Severity**: P2
**Category**: long-scope
**Summary**: `refresh_client_response_body_limit_for_channel` iterates all sessions and holds a read lock on each.
**Detail**:
This function is called from multiple manager mutation sites (commit_create_session_draft, close_session, activate_session). It iterates all `self.sessions` values, acquiring a read lock on each, filtering by secure_channel_id, and computing the minimum `max_response_message_size`. For N sessions, this is O(N) sequential read lock acquisitions.

**Timing estimate**: For N=100 sessions: ~5–20µs. For N=1,000 sessions: ~50–200µs.

**Mitigation**: Maintain a per-channel minimum cached with each write that sets `max_response_message_size`. This would reduce the lookup from O(N) to O(1).

---

## Finding ID: INTERN-009

**File**: `async-opcua-server/src/session/manager.rs:1096-1101`
**Severity**: P0
**Category**: crypto-on-async
**Summary**: `verify_client_signature` is called inside the `activate_session` function on the async thread, performing RSA signature verification.
**Detail**:
`SessionManager::verify_client_signature` (line 827-861) calls `opcua_crypto::verify_signature_data` which performs RSA-2048 signature verification. This is called at line 1096-1101 inside `activate_session`, which is an async function running on a tokio worker thread. RSA-2048 signature verification takes ~0.1–2ms.

Combined with INTERN-003 (the long read lock scope), this means every ActivateSession request blocks the worker thread for up to several milliseconds with a read lock held on the session manager.

**Mitigation**: Move the signature verification to `spawn_blocking` if the crypto library is purely synchronous, or restructure `activate_session` to perform verification before acquiring the manager lock.

---

## Finding ID: INTERN-010

**File**: `async-opcua-history-sqlite/src/backend.rs:268-271, 281-291`
**Severity**: P2
**Category**: poll-block
**Summary**: Continuation point pruning and insertion use `parking_lot::Mutex` on the async thread.
**Detail**:
- `prune_continuation_points` (line 268-271): Called at the top of every history trait method (read_raw_modified, update_data, update_structure_data, update_event, delete_raw_modified, delete_at_time, delete_event). Acquires `self.continuation_points.lock()` which is a `parking_lot::Mutex`.
- `insert_continuation_point` (line 281-291): Same Mutex.

The prune operation iterates the HashMap and removes expired entries. The insert just inserts one entry. Both are fast (microseconds), but since `prune_continuation_points` is called before every operation and `insert_continuation_point` is called during reads, under high frequency history query churn, the Mutex could contend.

**Timing estimate**: Microseconds (HashMap retain with small maps, typically <100 entries).

**Mitigation**: Low priority. If history query churn becomes a bottleneck, replace `Mutex<HashMap>` with `DashMap` and a periodic pruning task.

---

## Finding ID: INTERN-011

**File**: `async-opcua-server/src/session/manager.rs:1101-1128`
**Severity**: P1
**Category**: long-scope
**Summary**: The inner session read lock in `activate_session` (1053-1128) is held for 76 lines including multiple validation checks and field extractions.
**Detail**:
Inside the manager read lock scope, a session read lock is acquired at line 1053 and held until line 1128. This scope includes:
- `validate_timed_out()` (1055) — checks timestamps
- `endpoint_exists()` lookup (1059-1061) — iterates endpoint registrations
- Cross-channel validation (1070-1093) — string comparisons, SecurityPolicy from_uri parsing
- `verify_client_signature` (1095-1101) — RSA signature verification (see INTERN-009)
- Identity token construction and validation (1104-1126) — several branch checks on enums

The session read lock itself is not heavily contended (only one writer at a time updates session state), but the RSA signature verification (see INTERN-009) is the dominant cost.

**Timing estimate**: 0.2–3ms per call (dominated by RSA verify at 1101).

**Mitigation**: Extract the `session_nonce`, `endpoint_url`, and other immutable fields before the session read lock, then perform the expensive validation (signature verify) outside both locks. Only re-acquire the session write lock for the final state mutation at line 1226-1227.

---

## Finding ID: INTERN-012

**File**: `async-opcua-server/src/session/manager.rs:730-825`
**Severity**: P2
**Category**: long-scope
**Summary**: `commit_create_session_draft` holds `&mut self` (exclusive access) while scanning all sessions for eviction and spawning the session actor.
**Detail**:
This function takes `&mut self` (the entire SessionManager), which means no other manager operations can proceed during:
1. Session limit check and eviction scan (731-766) — O(N) scan of all sessions
2. Unactivated count check (767-774)
3. HashMap insertion and token registration (798-803)
4. Actor spawn (804-811) — tokio::spawn is non-blocking
5. Client response body limit refresh (812) — O(N) session scan

Steps 1 and 5 each iterate all sessions. For 100 sessions this is ~10µs. But step 1 holds a write lock on each eviction candidate session to call `close()`, which could be more expensive.

**Timing estimate**: 10–200µs for typical session counts.

**Mitigation**: Already well-structured — the exclusive access is intentional for atomic create+register. Session count limits mitigate worst-case behavior.

---

## Finding ID: INTERN-013

**File**: `async-opcua-server/src/session/manager.rs:959-986`
**Severity**: P2
**Category**: long-scope
**Summary**: `close_session` read lock scope contains two nested locks (session read lock at 964 and manager read lock at 959).
**Detail**:
The read lock at line 959 (`let mgr = trace_read_lock!(mgr_lck);`) nests a session read lock at 964. Both are read locks, so neither blocks other readers. The scope includes:
- `find_by_token` (DashMap lookup, lock-free)
- Session read lock for field extraction (964-979): checks `is_activated()`, `secure_channel_id()`, `user_token()`, `authentication_token`
- `actor_sender` lookup (DashMap lookup, lock-free)

All operations inside are O(1) field reads or lock-free map lookups. The session read lock is brief (CPUs-only).

**Timing estimate**: 1–5µs.

**Mitigation**: No action needed for the read lock scope. The lock discipline is correct — both locks are dropped before the `.await` at lines 994 and 999.

---

## Finding ID: INTERN-014

**File**: `async-opcua-server/src/subscriptions/mod.rs:1337-1365`
**Severity**: P2
**Category**: long-scope
**Summary**: `create_monitored_items` acquires a write lock on `SubscriptionCacheInner` to update the reverse index.
**Detail**:
After `.await` on `cache.create_monitored_items()` (line 1332-1335), a write lock is acquired at line 1337 (`let mut lck = trace_write_lock!(self.inner);`) to populate the `monitored_items` reverse index. For a batch of N created items, this writes N entries into the HashMap. Each entry involves a HashMap insertion.

**Timing estimate**: 1–5µs per item batch (typical: 1–10 items). Up to ~50µs for a batch of 100 items.

**Mitigation**: Acceptable. Write lock scope is proportional to batch size and only held after the `.await`.

---

## Finding ID: INTERN-015

**File**: `async-opcua-server/src/subscriptions/notify.rs` (inferred from usage)
**Severity**: P1
**Category**: waker-miss
**Summary**: `push_pending_data_notifications` (line 370-377) pushes to actor ring buffers without explicit waker notification.
**Detail**:
When `SubscriptionDataNotifier::drop` calls `push_pending_data_notifications`, notification work items are pushed into each subscription actor's ring buffer via `push_notification`. The actor wakes either via:
1. Its own tick timer (publishing interval)
2. Incoming publish request wakeup

There is no explicit wake after pushing notifications. This means:
- If a `PublishRequest` is already queued and waiting, it won't be woken immediately when data arrives
- The notification will only be picked up on the next actor tick or the next incoming PublishRequest

The impact is delayed notification delivery — the client gets data on the next publishing interval boundary rather than immediately. This is spec-compliant (Publish requests explicitly wait for data), but in a low-latency scenario, the delay could be up to one publishing interval (typically 100–500ms).

**Mitigation**: After pushing notifications, check if there are queued PublishRequests on the actor and wake them. The actor already has this logic in its tick loop, but a direct wake from the push path would reduce latency.

---

## Finding ID: INTERN-016

**File**: `async-opcua-server/src/subscriptions/mod.rs:1133-1181`
**Severity**: P2
**Category**: poll-block
**Summary**: `notify_data_change` is a synchronous function that acquires read locks and pushes notifications — could block if called from an async context under write lock contention.
**Detail**:
`notify_data_change` is called from:
- Node manager write handlers (e.g., `core.rs:1147`)
- `server_handle.rs:96`
- `server_status.rs:181`
- `sync_sampler.rs:222`

All these call sites execute in the session actor's message handler (tokio task). The function acquires a read lock on `SubscriptionCacheInner` (via `data_route_snapshot`), builds notification batches, and pushes to ring buffers. If a write lock on `SubscriptionCacheInner` is currently held (e.g., by `create_subscription`), the read lock will block via `parking_lot`. For a heavy write workload, this could cause µs-to-ms delays on the session actor's tokio task.

**Timing estimate**: Microseconds typical, up to ~1ms under write lock contention.

**Mitigation**: If contention becomes measurable, consider using a concurrent data structure (e.g., `DashMap` with `ArcSwap` for route snapshots) to eliminate the read lock entirely from the hot notification path.

---

## Finding ID: INTERN-017

**File**: `async-opcua-server/src/session/manager.rs:627-636, 706-717`
**Severity**: P2
**Category**: poll-block
**Summary**: `spawn_session_actor` calls `tokio::spawn` which is async-runtime-safe but enqueues the actor task immediately.
**Detail**:
`spawn_session_actor` is called inside `commit_create_session_draft` (line 804), which holds `&mut self`. The `tokio::spawn` at line 706 is non-blocking. The spawned future acquires `.read()` on the session inside `SessionActor::new`, but that's after the spawner's exclusive access is released (the spawner returns `Ok(response)` at line 824).

No issue found.

---

## Finding ID: INTERN-018

**File**: `async-opcua-server/src/session/manager.rs:370-416`
**Severity**: P2
**Category**: crypto-on-async
**Summary**: `CreateSessionServerSignature::preflight` calls `opcua_crypto::create_signature_data` for the server signature, which does RSA/ECC signing on the async thread.
**Detail**:
At line 271-289, `opcua_crypto::create_signature_data` is called with the server's private key. For RSA-based security policies (Basic256Sha256, Aes128Sha256RsaOaep), this does RSA signing (~2–5ms for RSA-2048). This is inside `CreateSessionDraft::prepare_endpoint_preflight` which is called during CreateSession processing on the async thread.

Additionally, line 301-317 calls `opcua_crypto::ecc::issue_server_ephemeral_key` for ECDH key issuance (ECC key generation, ~1–3ms).

**Timing estimate**: 1–5ms for RSA signing, 1–3ms for ECC key generation.

**Mitigation**: If CreateSession latency is a concern, wrap the signature creation and ECDH key issuance in `spawn_blocking`.

---

## Finding ID: INTERN-019

**File**: `async-opcua-server/src/session/manager.rs:171-176`
**Severity**: P2
**Category**: poll-block
**Summary**: `X509::from_byte_string` in `resolved_identity_from_activation` parses DER certificate on the async thread.
**Detail**:
At line 172, `X509::from_byte_string(&token.certificate_data)` parses a DER-encoded X.509 certificate. DER parsing is CPU-bound but fast for typical certificates (~1KB DER). This is called from inside `activate_session` at line 1281, within the session write lock scope.

**Timing estimate**: 10–100µs for DER parsing.

**Mitigation**: Acceptable for typical certificate sizes. If certificates are unusually large (>10KB), consider moving this to the `preflight` structure.

---

## Finding ID: INTERN-020

**File**: `async-opcua-history-sqlite/src/backend.rs:813-989`
**Severity**: P1
**Category**: contention-in-blocking
**Summary**: `update_data` holds a SQLite transaction inside `spawn_blocking`, serializing all concurrent writes.
**Detail**:
The `spawn_blocking` closure (827-982) acquires the connection Mutex, begins a SQLite transaction, iterates over all values, does per-value SELECT then INSERT/UPDATE/DELETE, then commits. For a batch of 100 values, this is 100 SELECT + 100 write operations within a single transaction. Under concurrent update_data calls (multiple nodes being updated simultaneously), each `spawn_blocking` task queues for the Mutex, serializing all work.

SQLite in WAL mode allows concurrent reads during a write transaction, but the Mutex serializes everything — reads AND writes queue behind the Mutex.

**Timing estimate**: 1–10ms for a 100-value batch (dominated by SQLite I/O). All other history operations wait for the Mutex during this time.

**Mitigation**: See INTERN-005. A connection pool with separate read/write connections would allow reads to proceed during writes.

---

## Finding ID: INTERN-021

**File**: `async-opcua-core/src/comms/secure_channel.rs:1333-1341`
**Severity**: P3
**Category**: poll-block
**Summary**: `std::sync::Mutex` (not `parking_lot`) used for `first_request_signature` inside `asymmetric_sign_and_encrypt`.
**Detail**:
At lines 1332-1334 and 1339-1341, `self.first_request_signature.lock()` is called. This is a `std::sync::Mutex` (from `use std::sync::Mutex;` at line 8). Under contention, `std::sync::Mutex` will block the OS thread (syscall), which is worse than `parking_lot::Mutex` (which spin-waits first). This Mutex is only used during the ECC server-side OpenSecureChannel response (first response only), so contention is unlikely. But if two concurrent OpenSecureChannel completions race on this Mutex, one will syscall-block the tokio worker.

**Timing estimate**: Nanoseconds for the mutex operation itself, microseconds under contention.

**Mitigation**: Replace `std::sync::Mutex` with `parking_lot::Mutex` for consistency. Low priority.

---

## Summary Statistics

| Category | P0 | P1 | P2 | P3 |
|----------|----|----|----|-----|
| crypto-on-async | 3 | 0 | 1 | 0 |
| long-scope | 1 | 2 | 6 | 0 |
| contention-in-blocking | 0 | 2 | 1 | 0 |
| poll-block | 0 | 0 | 3 | 1 |
| waker-miss | 0 | 1 | 0 | 0 |

## Top Mitigation Priorities

1. **INTERN-001, INTERN-002, INTERN-009, INTERN-018** (crypto-on-async): Wrap RSA/ECC operations in `spawn_blocking`. These are the highest-impact findings because RSA operations can block tokio workers for 1–20ms each.

2. **INTERN-003** (long-scope): Restructure `activate_session` to minimize the read lock scope and move signature verification outside the lock.

3. **INTERN-005, INTERN-020** (contention-in-blocking): Add a SQLite connection pool to allow concurrent reads.

4. **INTERN-006** (long-scope): Pre-construct `SessionEntry` outside the write lock in `create_subscription`.
