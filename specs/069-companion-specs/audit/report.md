# Async Lock Audit — Consolidated Report

**Codebase**: async-opcua
**Date**: 2026-07-07
**Source Reports**:
1. `audit-locks.md` (lock contention & deadlock)
2. `concurrency-debugging.md` (race conditions)
3. `memory-model.md` (atomic ordering & memory model)
4. `rust-async-patterns.md` (async patterns & design)
5. `rust-async-internals.md` (runtime blocking)
6. `parallel-agents.md` (benchmark design)

**Scope**: ~1174 .rs files, ~593 lock acquisition sites, 1 `tokio::sync::Mutex`, ~310× `parking_lot::Mutex`/`parking_lot::RwLock` sites, 5 ArcSwap sites, ~130+ `Relaxed` atomic sites.

---

## 1. Executive Summary

**Overall Assessment**: The codebase demonstrates above-average async concurrency hygiene. The core pattern — acquire `parking_lot` locks in short scopes, extract data, drop before `.await` — is consistently and correctly applied across all ~593 lock sites. No circular deadlock potential exists. No unsound memory ordering was found. Lock-free structures (DashMap, ArcSwap, AtomicU64) are used appropriately in hot paths.

**Primary risk area**: Seven P0 findings center on synchronous blocking operations (RSA/ECC cryptography and tokio mutex held across `.await`) running on tokio worker threads. Under concurrent connection storms or channel renewal, these can stall the async runtime, causing latency spikes across all connections on the same worker.

**Secondary risk area**: The single `Mutex<Connection>` in `SqliteHistoryBackend` serializes all SQLite I/O, preventing concurrent reads even when SQLite WAL mode supports them. Three independent agents flagged this as P1. Under concurrent history queries, throughput is capped at single-threaded SQLite performance.

| Severity | Count | Definition |
|----------|-------|------------|
| **P0** | 7 | Guaranteed problems under load — blocking on async threads, deadlock risk |
| **P1** | 10 | Likely problems under contention — hotspot contention, TOCTOU, missing spawn_blocking |
| **P2** | 18 | Design improvements — suboptimal patterns, style inconsistencies, migration candidates |
| **P3** | 1 | Minor — std::sync::Mutex inconsistency (low priority) |
| **LOW** | 8 | Verified-safe patterns worth documenting |

---

## 2. Per-Crate Breakdown

### 2.1 `async-opcua-core` (comms/secure_channel, handle, lib)

| ID | File | Severity | Category | Issue |
|----|------|----------|----------|-------|
| P0-CRYPTO-01 | `secure_channel.rs:1266–1395` | P0 | crypto-on-async | RSA signing + encryption blocks tokio worker 1–20ms |
| P0-CRYPTO-02 | `secure_channel.rs:1502–1660` | P0 | crypto-on-async | RSA decryption + verification blocks tokio worker 5–20ms |
| P2-MUTEX-01 | `secure_channel.rs:123` | P2 | type-inconsistency | `std::sync::Mutex<Vec<u8>>` instead of `parking_lot::Mutex` |
| P2-SPAWN-01 | `secure_channel.rs:873–940` | P2 | spawn-blocking-gap | Symmetric crypto not wrapped in `spawn_blocking` |
| P2-ATOMIC | `handle.rs:72–94` | P2 | relaxed-theoretical | Overflow path in `AtomicHandle::next()` uses Relaxed (practically unreachable) |
| LOW-TRACE | `lib.rs:94–112` | LOW | benign | `TRACE_LOCKS_STATE` double-check safe despite Relaxed |

**Hot-path summary**: Every message encode/decode acquires `read(encoding_context: Arc<RwLock<ContextOwned>>)`. This RwLock is the single most frequently acquired lock in the system.

### 2.2 `async-opcua-server` (session manager, subscriptions, address space)

| ID | File | Severity | Category | Issue |
|----|------|----------|----------|-------|
| P0-LONG-01 | `session/manager.rs:1046–1130` | P0 | long-scope+crypto | 84-line read lock scope containing RSA signature verify on async thread |
| P0-LONG-02 | `session/manager.rs:1096–1101` | P0 | crypto-on-async | `verify_client_signature` (RSA) inside read lock on async thread |
| P0-LONG-03 | `session/manager.rs:1053–1128` | P0 | long-scope | 76-line session read lock with RSA verify (0.2–3ms hold) |
| P0-CRYPTO-03 | `session/manager.rs:370–416` | P0 | crypto-on-async | CreateSession RSA signing + ECDH key generation on async thread (1–5ms) |
| P0-WAKER-01 | `subscriptions/mod.rs (notify path)` | P0 | waker-miss | Notification push doesn't wake queued PublishRequests; up to one interval delay |
| P1-SESSION-01 | `session/manager.rs:952–1034` | P1 | TOCTOU+scope | `close_session`: state can change between lock release and re-acquisition |
| P1-SESSION-02 | `session/manager.rs:720–825` | P1 | contention | `commit_create_session_draft`: O(n) eviction scan under write lock |
| P1-LONG-01 | `session/manager.rs:923–946` | P1 | long-scope | `check_session_expiry`: O(n) scan of all sessions with per-session read locks |
| P1-SUB-01 | `subscriptions/mod.rs:887–932` | P1 | contention | `create_subscription`: write lock held during SessionEntry construction (tokio::spawn inside lock) |
| P1-SUB-02 | `subscriptions/notify.rs:370–377` | P1 | waker-miss | Notifications pushed to ring buffers without waking actor |
| P2-SESSION-01 | `session/manager.rs:637–662` | P2 | contention | O(n) session iteration for `refresh_client_response_body_limit_for_channel` |
| P2-SESSION-02 | `session/manager.rs:1036–1321` | P2 | nesting | Nested `read(mgr)` inside `write(session)` — reverse ordering |
| P2-SUB-01 | `subscriptions/mod.rs:1606–1637` | P2 | scope | Two sequential `write(inner)` in `teardown_session` |
| P2-SUB-02 | `subscriptions/mod.rs:1337–1365` | P2 | scope | Write lock for reverse index update in `create_monitored_items` |
| P2-SUB-03 | `subscriptions/mod.rs:1063–1070` | P2 | long-scope | `data_route_snapshot` read lock during O(items) iteration |
| P2-SUB-04 | `subscriptions/mod.rs:1133–1181` | P2 | poll-block | `notify_data_change` sync call from async context; acquires read lock |
| P2-AS-01 | `address_space/mod.rs:391–394` | P2 | TOCTOU | `build_browse_name_index` TOCTOU — two threads may both build |
| P2-AS-02 | `address_space/mod.rs:362` | P2 | contention | O(nodes) `build_browse_name_index` holds write lock |
| P2-PROG-01 | `programs/engine.rs:151` | P2 | contention | `address_space.write()` acquired per program iteration |
| P2-TYPE-01 | `node_manager/memory/memory_mgr_impl.rs:2282` | P2 | contention | Multiple `type_tree.write()` calls during import |
| LOW-SESSION-01 | `session/manager.rs:720–825` | LOW | verified-safe | `commit_create_session_draft` `&mut self` is correct (sync method) |
| LOW-CONTROLLER-01 | `session/controller.rs:296–360` | LOW | verified-safe | `tokio::select!` in controller is safe |

### 2.3 `async-opcua-history-sqlite`

| ID | File | Severity | Category | Issue |
|----|------|----------|----------|-------|
| P1-SQLITE-01 | `backend.rs:22–23` | P1 | contention | Single `Mutex<Connection>` serializes ALL SQLite I/O |
| P1-SQLITE-02 | `backend.rs:22–24` | P1 | redundant-mutex | `Arc<Mutex<Connection>>` is redundant inside `spawn_blocking` closures |
| P1-SQLITE-03 | `backend.rs:813–989` | P1 | contention | `update_data` holds transaction inside `spawn_blocking`, serializing writes |
| P2-SQLITE-01 | `backend.rs:268–291` | P2 | poll-block | Continuation point Mutex acquires on async thread |

### 2.4 `async-opcua-client`

| ID | File | Severity | Category | Issue |
|----|------|----------|----------|-------|
| P0-ASYNC-01 | `transport/channel.rs:195–218` | P0 | deadlock-risk | `tokio::sync::Mutex` held across `.await` in `renew_secure_channel` |
| P1-RACE-01 | `transport/state.rs:173–178` | P1 | data-race | Non-atomic read-modify-write on `client_offset` ArcSwap |
| P1-RACE-02 | `transport/channel.rs:225–253` | P1 | TOCTOU | `send()` uses stale channel sender after `.await` |
| P1-ASYNC-01 | `session/services/subscriptions/service.rs:2487–2503` | P1 | cancellation | `PendingClientDeliveryGuard::Drop` acquires Mutex on async thread |
| P2-MUTEX-02 | `transport/channel.rs:44,194–199` | P2 | mixed-types | `tokio::Mutex` nested with `parking_lot` locks |
| P2-SUB-05 | `session/services/subscriptions/service.rs:204` | P2 | migration | 27 Mutex acquisition sites in SubscriptionState — candidate for actor model |
| P2-RACE-01 | `transport/channel.rs:397–409` | P2 | TOCTOU | `close_channel()` may send on stale channel |
| P2-ATOMIC-01 | `session/mod.rs:362–381` | P2 | fragile | `should_reconnect` Relaxed ordering depends on mpsc happens-before |
| P2-ATOMIC-02 | `session/mod.rs:308–313` | P2 | benign-window | `reset()` split stores create nanosecond inconsistency window |
| LOW-RACE-01 | `session/instance.rs:224–228` | LOW | accepted | Session timeout TOCTOU is microseconds-wide; acceptable |
| LOW-RACE-02 | `subscriptions/mod.rs:197–206` | LOW | verified-safe | `RefCell<HashMap>` is stack-local and `!Sync` — correct |
| LOW-RACE-03 | `session/instance.rs:242–244` | LOW | benign | Stale deadline read is harmless for polling-based expiry |

---

## 3. Consolidated Findings Table

Findings are sorted by severity then by file. When multiple agents flagged the same location, the consolidated severity reflects the most conservative assessment and the summary incorporates all perspectives.

| ID | File:Line | Sev | Category | Summary | Source Agents |
|----|-----------|-----|----------|---------|---------------|
| **C-001** | `secure_channel.rs:1266–1395` | **P0** | crypto-on-async | `asymmetric_sign_and_encrypt` (RSA 2–5ms signing + 0.1–1ms encrypt) runs on tokio worker; stalls runtime under concurrent OpenSecureChannel requests | internals (#1, #9), async-patterns (#5) |
| **C-002** | `secure_channel.rs:1502–1660` | **P0** | crypto-on-async | `asymmetric_decrypt_and_verify` (RSA 5–20ms decryption + verify) runs on tokio worker; connection storms cause cross-connection latency spikes | internals (#2) |
| **C-003** | `manager.rs:1046–1130` | **P0** | long-scope+crypto | `activate_session` holds 84-line manager read-lock + 76-line session read-lock containing RSA signature verification (0.2–3ms) on async thread; combined with INTERN-003/009/011 | internals (#3, #9, #11), locks (#10) |
| **C-004** | `manager.rs:370–416` | **P0** | crypto-on-async | `CreateSessionServerSignature::preflight` does RSA signing (2–5ms) + ECDH key generation (1–3ms) on async thread during CreateSession | internals (#18) |
| **C-005** | `channel.rs:195–218` | **P0** | deadlock-risk | `tokio::sync::Mutex` guard held across `.await` in `renew_secure_channel`; if transport handler re-enters `send()`, deadlock. Also serializes all traffic during renewal (single-flight pattern is intentional but blocking) | async-patterns (#1), locks (#6) |
| **C-006** | `subscriptions/mod.rs` (notify path) | **P0** | waker-miss | `push_pending_data_notifications` pushes to actor ring buffers without waking queued PublishRequests; notification latency up to one publishing interval (100–500ms) | internals (#15) |
| **C-007** | `subscriptions/mod.rs:1133–1181` | **P0** | waker-miss | `notify_data_change` sync path called from async context; pushes notifications without waking actors | internals (#15, #16) |
| **C-008** | `backend.rs:22–24` | **P1** | contention+redundant | Single `Arc<Mutex<Connection>>` serializes all SQLite I/O despite WAL-mode concurrency support. Mutex redundant inside `spawn_blocking` closures. `spawn_blocking` pool exhaustion risk under heavy history load | locks (#3), async-patterns (#2), internals (#5, #20) |
| **C-009** | `manager.rs:952–1034` | **P1** | TOCTOU+scope | `close_session`: extracts data under read lock, drops, awaits actor termination, re-acquires write lock. Session state can change between lock scopes. `unactivated_by_channel` counter may underflow if expiry loop already decremented | locks (#1), concurrency (#4), internals (#13) |
| **C-010** | `manager.rs:720–825` | **P1** | contention | `commit_create_session_draft`: holds exclusive access during O(n) eviction candidate scan + O(n) client response body limit refresh. Every CreateSession blocks all other dispatches | locks (#2), internals (#12) |
| **C-011** | `state.rs:173–178` | **P1** | data-race | `set_client_offset()` read-modify-write on ArcSwap acknowledged by developer comment; safe only because serialized by `issue_channel_lock` tokio mutex. Future call site would introduce lost-update bug | concurrency (#2), memory-model (#1) |
| **C-012** | `channel.rs:225–253` | **P1** | TOCTOU | `send()` loads `request_send` channel sender, then `.await`s, then uses stale sender. During reconnection, old sender's receiver is dropped; request fails with `BadConnectionClosed` when new transport is available | concurrency (#3) |
| **C-013** | `service.rs:2487–2503` | **P1** | cancellation | `PendingClientDeliveryGuard::Drop` acquires `parking_lot::Mutex` on async thread during future cancellation; can conflict with publish retry loop | async-patterns (#3) |
| **C-014** | `manager.rs:923–946` | **P1** | long-scope | `check_session_expiry` iterates all sessions (O(n)) with per-session read locks on timer thread; for N=10,000 sessions: ~1–5ms | internals (#4) |
| **C-015** | `subscriptions/mod.rs:887–932` | **P1** | contention | `create_subscription`: write lock on `SubscriptionCacheInner` held during `SessionEntry::new()` which spawns tokio actor; fragile — future actor init changes could deadlock. Also two write lock acquisitions with async gap | locks (#4), async-patterns (#4), internals (#6) |
| **C-016** | `backend.rs:813–989` | **P1** | contention | `update_data` holds SQLite transaction inside `spawn_blocking` with Mutex held; serializes all concurrent writes and blocks all readers | internals (#20) |
| **C-017** | `manager.rs:1036–1321` | **P1** | contention | `activate_session`: session write lock at :1227, then reads manager read lock at :1308 inside session write — reverse ordering from outer scope. Also `activate_session` holds long session read lock (76 lines) during validation | locks (#10), internals (#11) |
| **C-018** | `service.rs:204` | **P2** | migration | `Mutex<SubscriptionState>`: 22–27 lock sites in single file. Single mutex guards all subscription state. Notification delivery callbacks run inside lock. Actor/channel model would eliminate all locks | locks (#7), async-patterns (#6) |
| **C-019** | `secure_channel.rs:123` | **P2** | type-inconsistency | `std::sync::Mutex<Vec<u8>>` for `first_request_signature` — only `std::sync::Mutex` in codebase; poisoning boilerplate. Hold time trivial (<100ns) | locks (#5), async-patterns (#8), internals (#21) |
| **C-020** | `address_space/mod.rs:391–394` | **P2** | TOCTOU | `ensure_browse_name_index`: read-then-write TOCTOU — two threads may both build index, second rebuilds unnecessarily | locks (#8) |
| **C-021** | `address_space/mod.rs:362` | **P2** | contention | `build_browse_name_index` O(nodes) under write lock (10–100ms for 10K+ nodes); blocks all browse operations | locks (#11) |
| **C-022** | `manager.rs:637–662` | **P2** | contention | `refresh_client_response_body_limit_for_channel`: O(n) session iteration with N read locks per call. Called from CreateSession, ActivateSession, CloseSession | locks (#9), internals (#8) |
| **C-023** | `subscriptions/mod.rs:1606–1637` | **P2** | scope | `teardown_session`: two separate write lock acquisitions on `inner` with async gap. Could be merged | locks (#12) |
| **C-024** | `programs/engine.rs:144–151` | **P2** | contention | Program engine acquires `address_space.write()` per loop iteration; for long-running programs, starves all browse readers | locks (#13) |
| **C-025** | `subscriptions/mod.rs:1063–1070` | **P2** | long-scope | `data_route_snapshot`: read lock held during O(monitored_items) HashMap iteration; for 10K items on one node: 0.5–2ms | internals (#7) |
| **C-026** | `subscriptions/mod.rs:1337–1365` | **P2** | scope | `create_monitored_items`: write lock for reverse index update proportional to batch size (up to ~50µs for 100 items) | internals (#14) |
| **C-027** | `backend.rs:268–291` | **P2** | poll-block | Continuation point Mutex acquires on async thread (µs-scale, low risk) | internals (#10), locks (#3) |
| **C-028** | `channel.rs:44,194–199` | **P2** | mixed-lock | `tokio::sync::Mutex` nested with `parking_lot::RwLock` — safe because parking_lot locks dropped before `.await`. Only async mutex in codebase | locks (#6) |
| **C-029** | `channel.rs:397–409` | **P2** | TOCTOU | `close_channel()` may send `CloseSecureChannel` on stale/disconnected channel; best-effort close is acceptable | concurrency (#6) |
| **C-030** | `session/mod.rs:362–381` | **P2** | fragile-ordering | `should_reconnect` uses Relaxed on both sides; correctness depends on mpsc channel happens-before. Fragile to future refactor | memory-model (#3) |
| **C-031** | `secure_channel.rs:873–940` | **P2** | spawn-blocking-gap | Symmetric crypto (AES/HMAC) runs on async threads without `spawn_blocking`. For typical OPC UA chunk sizes (1–64KB), <100µs — acceptable but worth profiling | async-patterns (#5) |
| **C-032** | `node_manager/memory/memory_mgr_impl.rs:2282` | **P2** | contention | Multiple `type_tree.write()` during startup import; safe during single-threaded startup, but risky if type tree becomes lazily loaded | locks (#14) |
| **C-033** | `secure_channel.rs:1333–1341` | **P3** | type-inconsistency | `std::sync::Mutex` on async thread can syscall-block under contention (ECC first request only — low frequency) | internals (#21) |
| **C-034** | `instance.rs:224–228` | **LOW** | accepted-TOCTOU | Session timeout validation TOCTOU is microseconds-wide; spurious `BadSessionIdInvalid` under rare conditions. Acceptable for polling-based expiry | concurrency (#1), memory-model (#2) |
| **C-035** | `handle.rs:72–94` | **LOW** | verified-safe | `AtomicHandle::next()` overflow path (~4B calls to reach) correct under Relaxed; bounded retry loop | concurrency (#5), memory-model (#4) |
| **C-036** | `manager.rs:547–1310` | **LOW** | exemplar | `unactivated_by_channel` correctly paired Release/Acquire for create/expiry/limit-check. Model for future atomic enforcement | memory-model (#8) |
| **C-037** | `subscriptions/mod.rs:197–206` | **LOW** | verified-safe | `SubscriptionDataNotifier` uses `RefCell` correctly — stack-local, `!Sync`, never shared across threads | concurrency (#9) |
| **C-038** | `controller.rs:296–360` | **LOW** | verified-safe | `tokio::select!` in controller: no locks held across branches; no problematic Drop impls | async-patterns (#9) |
| **C-039** | `manager.rs:720–825` | **LOW** | verified-safe | `commit_create_session_draft` takes `&mut self` (sync method); borrow checker statically prevents concurrent access | async-patterns (#10) |
| **C-040** | `channel.rs:261–323` | **LOW** | verified-safe | `request_send` ArcSwap publication ordering correct (SeqCst); stale sender benign (error propagated to caller) | memory-model (#9) |
| **C-041** | `metrics.rs`, `server.rs`, `discovery/` | **LOW** | verified-safe | All metrics counters (Relaxed), `port: AtomicU16`, `service_level`, `TRACE_LOCKS_STATE` — all correct for statistical/monitoring/single-writer use | memory-model (#6, #7, #12, #13) |

---

## 4. Cross-Cutting Themes

### Theme A: Synchronous Blocking on Async Runtime Threads (P0)

The most impactful systemic issue. RSA/ECC cryptographic operations run inline on tokio worker threads without `spawn_blocking` wrappers. Under concurrent connection setup (many clients opening/renewing secure channels simultaneously), the cumulative blocking stalls the cooperative scheduler.

**Affected sites**: `secure_channel.rs:1266` (sign+encrypt), `secure_channel.rs:1502` (decrypt+verify), `manager.rs:370` (CreateSession signing), `manager.rs:1096` (ActivateSession verify)

**Timing**: RSA-2048 signing: 2–5ms. RSA-2048 decryption: 5–20ms. ECC P-256: 1–3ms. Under 10 concurrent OpenSecureChannel requests, this is 20–200ms of cumulative blocking.

**Consensus**: All three agents that examined these paths (internals #1, #2, #9, #18; async-patterns #5; memory-model — no finding) agree these should be wrapped in `spawn_blocking`. The async-patterns agent noted that for the common small-chunk case, the impact is minimal, but under connection storms the effect is measurable.

### Theme B: Write-Lock Contention on `SubscriptionCacheInner` (P1)

Three agents independently flagged `create_subscription` for holding the write lock on `subscription_cache.inner` while constructing `SessionEntry` (which spawns a tokio actor). The write lock blocks all concurrent subscription operations.

**Mitigation consensus**: Pre-construct `SessionEntry` outside the write lock, then acquire the write lock solely for HashMap insertion.

**Additional subscription hotspot**: `data_route_snapshot` holds a read lock during O(monitored_items) iteration. Notifications at high item counts (10K+) can hold the read lock for 0.5–2ms, starving concurrent write lock attempts (subscription create/delete).

### Theme C: SQLite Single-Connection Bottleneck (P1)

The most cross-referenced P1 finding. Four agents (locks #3, async-patterns #2, internals #5, internals #20) flagged the same root cause: `Arc<Mutex<Connection>>` prevents concurrent SQLite reads despite WAL-mode support.

**Consensus**: All agents recommend connection pooling (r2d2-sqlite, deadpool-sqlite) with separate read connections. The async-patterns agent additionally notes the `Mutex` is technically redundant inside `spawn_blocking` closures (the Connection is used single-threaded within each closure).

### Theme D: Session Manager Lock Scope & TOCTOU (P1–P2)

`close_session` and `activate_session` both use the "extract-under-read-lock → await → re-acquire-write-lock" pattern. The core pattern is correct (both locks dropped before `.await`), but three issues arise:

1. **TOCTOU**: Session state can change between lock scopes (LOCK-001, RACE-004)
2. **Scope**: The first read lock scope is too long, containing nested locks and validation (INTERN-003, INTERN-011)
3. **Reverse nesting**: `activate_session` nests `read(mgr)` inside `write(session)` at line 1308 — safe but fragile (LOCK-010)

**Mitigation consensus**: Pre-compute `was_unactivated` flag before write lock; move RSA signature verify outside read lock scope.

### Theme E: `std::sync::Mutex` vs `parking_lot::Mutex` Inconsistency (P2–P3)

The codebase uses `parking_lot::Mutex`/`RwLock` everywhere (re-exported as `opcua_core::sync::{Mutex, RwLock}`) except one site: `first_request_signature` in `secure_channel.rs:123` uses `std::sync::Mutex`. Three agents flagged this.

**Consensus**: Low priority (hold time <100ns, infrequent ECC path), but inconsistency creates maintenance confusion and requires poisoning error handling.

### Theme F: Atomic Ordering Is Correct and Conservative (LOW)

The memory-model audit exhaustively verified all 130+ Relaxed sites, 5 ArcSwap sites, and 20 SeqCst sites. All correct. The `unactivated_by_channel` counter (manager.rs) was highlighted as the exemplar of proper Acquire/Release pairing. The 5 ArcSwap sites use default SeqCst ordering — safe, but could be relaxed to Acq/Rel for 4 of 5 sites if profiling shows benefit.

### Theme G: ArcSwap Usage Pattern (Mixed)

The codebase uses ArcSwap at 5 sites. Four are correct (but have TOCTOU windows between load and use across `.await`). One (`client_offset` in state.rs:173–178) has an acknowledged non-atomic read-modify-write bug, currently mitigated by external serialization.

---

## 5. Deep Dive: Four Critical Systems

### 5.1 SqliteHistoryBackend

**Architecture**: `Arc<Mutex<Connection>>` + `Arc<Mutex<HashMap<continuation_points>>>`. All 10 history operations go through `spawn_blocking`.

**Primary finding** (C-008, P1): The single `Mutex<Connection>` is the bottleneck. SQLite WAL mode supports concurrent readers, but the Mutex prevents any concurrency. Two concurrent `read_raw_modified` calls for different nodes serialize on the Mutex.

**Secondary finding**: The Mutex is redundant inside `spawn_blocking`. Each `spawn_blocking` closure clones the `Arc<Mutex<Connection>>` then calls `conn.lock()` — but inside the blocking closure, the Connection is single-threaded. The Mutex provides isolation between closures but at the cost of serialization.

**Impact under load**:
- 16 concurrent disjoint history readers → aggregate throughput ~same as single reader (serialized)
- `update_data` with 100-value batch holds Mutex for 1–10ms → blocks all readers during write
- `spawn_blocking` thread pool exhaustion: if all threads are blocked on Mutex, new queries stall

**Consensus recommendation**: Replace `Mutex<Connection>` with a connection pool (r2d2-sqlite). Enable WAL mode. Read operations use any available pooled connection; writes use a single write connection. Continuation points can use `DashMap` + periodic pruning.

### 5.2 Session Manager

**Architecture**: `RwLock<SessionManager>` (outer) containing `HashMap<sessions>`, `DashMap<auth_tokens>`, `DashMap<actor_senders>`. Per-session `Arc<RwLock<Session>>`.

**Primary findings**:
- C-009 (P1): `close_session` TOCTOU — state changes between lock scopes
- C-010 (P1): `commit_create_session_draft` O(n) eviction scan under exclusive access
- C-003 (P0): `activate_session` read lock (84 lines) with RSA verify on async thread
- C-014 (P1): `check_session_expiry` O(n) scan of all sessions on timer thread
- C-017 (P1): `activate_session` reverse nesting `write(session) → read(mgr)`

**Lock ordering hierarchy** (verified safe):
```
read(mgr) → read(session) → DROP → await → write(mgr) → read(session)   [close_session]
read(mgr) → read(session) → DROP → await → write(session) → read(mgr)   [activate_session — REVERSE at write(session)→read(mgr)]
write(mgr) → read(session₀..sessionₙ) → write(evicted_session)           [commit_create_session_draft]
```

No circular deadlock potential. Reverse at activate_session safe because it's read(mgr) inside write(session), not write(mgr).

### 5.3 Secure Channel

**Architecture**: `SecureChannel` owns `encoding_context: Arc<RwLock<ContextOwned>>` (read on every encode/decode) and `first_request_signature: std::sync::Mutex<Vec<u8>>` (ECC only, brief).

**Primary findings**:
- C-001, C-002 (P0): RSA/ECC crypto operations on async threads — highest priority fix
- C-031 (P2): Symmetric crypto (AES/HMAC) on async threads without `spawn_blocking` — for typical chunk sizes (<64KB) this is <100µs, acceptable but document assumption
- C-019, C-033 (P2–P3): `std::sync::Mutex` inconsistency — low priority

**Impact under load**: Under connection storms, RSA operations (5–20ms each) on tokio workers cause request latency spikes across all connections. The cost is per-OpenSecureChannel message; under normal operation (infrequent), acceptable. Under reconnection storms or many concurrent clients, problematic.

**Consensus recommendation**: Wrap `asymmetric_sign_and_encrypt`, `asymmetric_decrypt_and_verify`, and the CreateSession signature path in `spawn_blocking`. This requires making the security processing pipeline async-aware (currently synchronous).

### 5.4 Subscriptions (Server-Side)

**Architecture**: `SubscriptionCache` wraps `RwLock<SubscriptionCacheInner>` containing `session_subscriptions: HashMap`, `subscription_to_session: HashMap`, `monitored_items: HashMap`. Subscription actors use ring buffers + tokio channel messaging.

**Primary findings**:
- C-015 (P1): `create_subscription` write lock scope includes actor spawn
- C-025 (P2): `data_route_snapshot` read lock during O(items) iteration
- C-006, C-007 (P0): Waker miss — notifications pushed without waking queued PublishRequests
- C-023 (P2): `teardown_session` two write lock acquisitions
- C-026 (P2): `create_monitored_items` write lock for reverse index

**Lock request breakdown** (typical Read request):
```
read(mgr) → read(session) → DashMap(node) → read(type_tree) + read(encoding_context)
```
3x RwLock reads, 0x writes, all O(1).

**Lock request breakdown** (CreateSubscription):
```
read(mgr) → read(session) → write(inner) → read(session) → write(inner)
                                                                   → write(inner) [create_monitored_items follow-up]
```
2x read, 2–3x write.

**Consensus**: The subscription cache would benefit from lock splitting: separate RwLock for `monitored_items` (route lookup, read-heavy) vs `session_subscriptions`/`subscription_to_session` (lifecycle, write-occasional). For P0 waker miss: add explicit `wake()` call to subscription actor after pushing notifications.

---

## 6. Benchmark Design Summary

Agent #6 (`parallel-agents`) designed a comprehensive 4-scenario benchmark harness for measuring lock contention:

### Test Scenarios

| Scenario | Focus | Key Metric |
|----------|-------|------------|
| Session Lifecycle | `SessionManager` write lock, per-session RwLock | Creates/sec, write-lock hold time P99 |
| History Reads | `SqliteHistoryBackend` Mutex, `spawn_blocking` saturation | Reads/sec under concurrency, pool queue depth |
| Subscription Dispatch | `SubscriptionCacheInner` RwLock contention | Notifications/sec, create P99 under notification load |
| Mixed Workload | Cross-contention (history + subscriptions + sessions) | Cross-contention latency correlation |

### Instrumentation

- **Criterion benchmarks**: `session_contention.rs`, `history_read.rs`, `subscription_dispatch.rs`
- **Stress harness**: `tools/opcua-contention-bench/` — TOML-scenario-driven, JSON metrics output
- **Lock metrics**: Proposed `lock-metrics` feature flag — `AtomicU64` counters for acquisition count, wait duration, hold duration per lock
- **tokio-console**: Worker utilization, `spawn_blocking` pool saturation, task polling latency
- **perf/flamegraph**: `perf record -e cpu-clock,lock:lock_acquire,lock:lock_contended`

### Expected Baselines

- Token lookup (DashMap): <100ns P99
- CreateSession: 2000–5000 ops/sec (limited by `&mut self` + actor spawn overhead)
- Single-reader history: 500–2000 reads/sec
- 16 concurrent disjoint history readers: ~same as single reader (serialized by Mutex)
- Single-item notification: 50K–200K/sec
- 10K-item mass notification: 500–1000 batch ops/sec

### CI Integration

- Regression detection via committed baseline JSON
- Fail CI if latency >20% increase or throughput >10% drop
- Short-duration scenario (5–10 min) per PR

---

## 7. Recommendations

### Immediate Actions (Quick-Fix, low risk)

| Priority | ID | Action | Effort |
|----------|----|--------|--------|
| **Do first** | C-001, C-002, C-004, C-003 | Wrap RSA/ECC crypto in `spawn_blocking`. Requires making `apply_security`/`verify_and_remove_security` call sites async-aware | Medium |
| P1 | C-008 | Replace SQLite `Mutex<Connection>` with connection pool (r2d2-sqlite). Enable WAL mode. Two read connections + one write connection | Medium |
| P1 | C-006, C-007 | Add `wake()` call to subscription actor after `push_pending_data_notifications` | Small |
| P1 | C-015 | Pre-construct `SessionEntry` outside write lock in `create_subscription` | Small |
| P1 | C-013 | Remove `Drop` lock acquisition in `PendingClientDeliveryGuard`; call `restore()` explicitly before `.await` | Small |
| P1 | C-010 | Pre-scan eviction candidate under read lock, then upgrade to write lock for eviction+insert | Small |
| P1 | C-011 | Replace `client_offset` ArcSwap read-modify-write with `fetch_update` or wrap in `tokio::sync::Mutex` | Small |
| P1 | C-012 | Re-load `request_send` after `renew_secure_channel().await` in `send()` | Small |
| P1 | C-003 | Move `verify_client_signature` outside manager read lock in `activate_session` | Small |
| P2 | C-020 | Add double-check under write lock in `build_browse_name_index` | Small |
| P2 | C-019 | Replace `std::sync::Mutex` with `parking_lot::Mutex` for `first_request_signature` | Small |
| P2 | C-022 | Track per-channel `max_response_message_size` in DashMap instead of O(n) scan | Small |

### Medium-Term Improvements

| Priority | ID | Action | Effort |
|----------|----|--------|--------|
| P1 | C-014 | Maintain expiry heap (`BinaryHeap<(Instant, NodeId)>`) for O(log n) session expiry instead of O(n) scan | Medium |
| P2 | C-018 | Convert `SubscriptionState` to actor/channel model (eliminates 27 lock acquisitions) | Large |
| P2 | C-021 | Use `ArcSwap<AddressSpaceCold>` to eliminate browse-read contention | Medium |
| P2 | C-025, C-026 | Split `SubscriptionCacheInner` into separate locks: `monitored_items` (read-heavy) vs lifecycle (write-occasional) | Medium |
| P2 | C-024 | Batch address space mutations in program engine to reduce per-iteration write lock acquisitions | Small |
| P2 | C-005 | Replace `tokio::sync::Mutex` single-flight with `watch::channel` — waiters subscribe to renewal result without holding any lock | Medium |
| P2 | C-030 | Document implicit channel synchronization for `should_reconnect`; optionally upgrade to Release/Acquire | Small |
| P2 | C-027 | Replace `Mutex<HashMap>` continuation points with `DashMap` + periodic prune | Small |

### Architectural (Future Consideration)

| ID | Action | Rationale |
|----|--------|-----------|
| — | Extend session actor to handle `CloseSession`/`ActivateSession` as actor commands | Eliminates nested lock patterns entirely (ASYNC-007); manager's `sessions` becomes DashMap |
| — | Shard `SubscriptionCache.monitored_items` for node-level lock granularity | Eliminates global RwLock from notification hot path (INTERN-016) |
| — | Benchmark harness (`tools/opcua-contention-bench`) | Quantify improvements before/after; CI gating |

---

## 8. What Works Well (Notable)

1. **Consistent lock-scope discipline**: Every `parking_lot` lock acquisition drops the guard before `.await`. The `trace_read_lock!`/`trace_write_lock!`/`trace_lock!` macros with block scoping enforce this pattern.

2. **Lock-free hot paths**: DashMap for auth tokens, actor senders, node store. ArcSwap for session timestamps, request channel, auth token. Lock-free patterns are correctly applied at all 3 DashMap + 5 ArcSwap sites.

3. **Correct `spawn_blocking` usage**: All 10 SQLite history operations correctly clone `Arc<Mutex<Connection>>` into the blocking closure.

4. **No circular deadlock potential**: Lock ordering hierarchy is well-defined (mgr → session → cold) with one safe reverse (write(session) → read(mgr)). Subscription cache is independent.

5. **No `block_in_place` or `block_on` misuse**: Neither function exists in the codebase.

6. **`unactivated_by_channel` Acquire/Release pairing**: The only correctly paired Acq/Rel usage in production — serves as an exemplar.

7. **No `fence()` calls needed**: Zero explicit memory barriers — the synchronization primitives (Mutex, ArcSwap, channel) provide sufficient happens-before.

8. **No Send/Sync violations**: `RefCell` usage is verified stack-local and `!Sync`. All `unsafe` blocks are either test-only or scoped to memory allocation.

---

## Appendix A: Source Agent Mapping

| Agent | Report File | Focus | P0 | P1 | P2 | Other |
|-------|-------------|-------|----|----|----|-------|
| #1 | `audit-locks.md` | Lock contention, deadlock, lock ordering | 0 | 4 | 10 | — |
| #2 | `concurrency-debugging.md` | Data races, TOCTOU, ArcSwap, Send/Sync | 0 | 3 | 6 | — |
| #3 | `memory-model.md` | Atomic ordering, happens-before, memory model | 0 | 2 | 11 | — |
| #4 | `rust-async-patterns.md` | Async patterns, lock-in-async, spawn_blocking | 1 | 2 | 5 | 2 LOW |
| #5 | `rust-async-internals.md` | Runtime blocking, poll/waker, lock scopes | 4 | 5 | 9 | 1 P3 |
| #6 | `parallel-agents.md` | Benchmark design (no findings) | — | — | — | 4 scenarios |

## Appendix B: Lock Hierarchy Diagram

```
                          SessionManager.write ─────────────────────────────┐
                              │                                             │
                          Session.read/write                                │
                                                                     (no nested locks)
                          
                          SessionManager.read ──────────────────────────────┐
                              │                                             │
                          Session.read/write  ──[REVERSE: line 1308]──► SessionManager.read
                                                                     (safe: read lock only)
                          
                          SubscriptionCache.write ──────────────────────────┐
                              │                                             │
                          Session.read (in SessionEntry::new)               │
                                                                     (safe, brief)

                          SubscriptionCache.read ───────────────────────────┐
                              │ (no nested locks, spawns actor messages)     │
                                                                     
                          AddressSpace.write/read ──────────────────────────┐
                              │ (flat, no nesting)                          │
                                                                     
                          tokio::Mutex (channel renewal) ───────────────────┐
                              │                                             │
                          SecureChannel.read/write (parking_lot)            │
                                                                     (safe: parking_lot dropped before .await)
                          
                          continuation_points.lock() ───────────────────────┐
                              │                                             │
                          connection.lock() (SQLite)                        │
                                                                     (consistent ordering)
```

**Verdict**: No AB/BA circular deadlock. All lock orderings form a DAG.

---

## Appendix C: Lock-Free Primitive Inventory

| Primitive | Sites | Usage | Assessment |
|-----------|-------|-------|------------|
| `ArcSwap` | 5 | Transport channel, session ID, auth token, client offset, session timestamp | 4/5 correct; 1 acknowledged RMW bug (C-011) |
| `DashMap` | 3 | Node store, auth tokens, actor senders | Correct — only `get`/`insert`/`remove`, no iteration-decisions |
| `AtomicU64` | 13 | Metrics counters (Relaxed) | Correct for statistics |
| `AtomicUsize` | 4 | Metrics, unactivated counter | Correct (Acq/Rel for counter) |
| `AtomicU32` | 3 | Session IDs (Relaxed) | Correct |
| `AtomicBool` | 3 | should_reconnect, diagnostics enabled | Correct (fragile happens-before for reconnect) |
| `AtomicU8`/`AtomicU16` | 4 | TRACE_LOCKS_STATE, service_level, port | Correct (single-writer / idempotent) |
| `AtomicHandle` | 2 | Request handle generator | Correct (Relaxed sufficient for opaque IDs) |
| `RefCell` | 1 | `SubscriptionDataNotifier` | Correct (stack-local, `!Sync`) |
