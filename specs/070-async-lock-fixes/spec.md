# Feature Specification: Async Lock Audit Remediation

**Feature Branch**: `070-async-lock-fixes`  
**Created**: 2026-07-07  
**Status**: Draft  
**Input**: User description: "Fix all P0, P1, and P2 issues catalogued in the async lock audit report (specs/069-companion-specs/audit/report.md)."

The audit examined ~1174 source files, ~593 lock acquisition sites, and found 7 P0, 10 P1, and 18 P2 issues. The primary risk areas are synchronous blocking operations on the async runtime, lock contention hotspots, and TOCTOU windows.

## User Scenarios & Testing

### User Story 1 — Offload RSA/ECC Cryptography from Async Runtime Threads (Priority: P0)

The server performs RSA signing (2–5ms), RSA decryption (5–20ms), ECC P-256 operations (1–3ms), and RSA signature verification (0.2–3ms) inline on tokio worker threads during OpenSecureChannel, CreateSession, and ActivateSession. Under concurrent connection storms (10+ clients opening secure channels simultaneously), cumulative blocking can hold a worker for 20–200ms, causing latency spikes across all unrelated connections sharing that worker.

**Why this priority**: This is the most impactful finding. Every connection during setup or renewal stalls the async runtime cooperatively. Under reconnection storms after network flaps, cascading latency can cause timeouts and failed connections.

**Independent Test**: Run a multi-client connection storm benchmark (50 clients simultaneously opening secure channels). Before fix, P99 connection setup latency should show spikes correlating with worker blocking. After fix, P99 latency remains within 2x of median. Verify `spawn_blocking` pool remains unsaturated.

**Acceptance Scenarios**:

1. **Given** an OpenSecureChannel request arrives, **When** `asymmetric_sign_and_encrypt` or `asymmetric_decrypt_and_verify` is invoked, **Then** the RSA operation runs inside `tokio::task::spawn_blocking`, not on a tokio worker thread.
2. **Given** a CreateSession request arrives, **When** RSA signing and ECDH key generation occur, **Then** both operations run inside `spawn_blocking`.
3. **Given** an ActivateSession request arrives, **When** `verify_client_signature` (RSA verify) runs, **Then** it runs inside `spawn_blocking` AND outside the manager read-lock scope.
4. **Given** symmetric crypto (AES/HMAC) for message chunks under 64KB, **When** encoding/decoding messages, **Then** the operation may remain on the async thread since hold time is <100µs, documented with the rationale.
5. **Given** all crypto is offloaded, **When** the existing test suite runs, **Then** all 618+ tests pass with no regression.

**Covers audit findings**: C-001, C-002, C-003, C-004, C-031

---

### User Story 2 — Fix Waker Miss in Subscription Notification Path (Priority: P0)

When `push_pending_data_notifications` pushes notifications to subscription actor ring buffers, it does not wake queued PublishRequests. Notifications can be delayed by up to one full publishing interval (100–500ms) because no explicit wake is sent. Additionally, `notify_data_change` in the sync path pushes notifications without waking actors.

**Why this priority**: This is a semantic correctness issue. The OPC UA specification requires timely notification delivery within the publishing interval. A one-interval delay violates the standard's expectations for real-time monitoring.

**Independent Test**: Set up a subscription with a 100ms publishing interval. Trigger a monitored item change. Measure the time between the data change and the PublishResponse delivery. Before fix, latency can reach 100–200ms (up to one full interval). After fix, latency is under 10ms (next event loop wake).

**Acceptance Scenarios**:

1. **Given** a monitored item changes value, **When** `push_pending_data_notifications` pushes to the actor ring buffer, **Then** the subscription actor is explicitly woken, and the queued PublishRequest is triggered immediately.
2. **Given** `notify_data_change` is called from the sync path, **When** it pushes notifications, **Then** relevant subscription actors are woken.
3. **Given** all existing subscription tests, **When** run after the fix, **Then** they pass without timing-related flakiness.

**Covers audit findings**: C-006, C-007

---

### User Story 3 — Eliminate Deadlock Risk in Secure Channel Renewal (Priority: P0)

The `renew_secure_channel` function holds a `tokio::sync::Mutex` guard across an `.await` point. If the transport handler re-enters `send()` during renewal (via a notification or retry), it attempts to acquire the same `tokio::sync::Mutex` again, causing a deadlock. The single-flight pattern is intentional for serializing renewal, but the async mutex makes it fragile.

**Why this priority**: A deadlock in the channel renewal path is a hard failure with no recovery — the channel becomes permanently stuck. While re-entrant calls are rare, the risk is real when notification callbacks trigger during the renewal window.

**Independent Test**: Write a targeted test that calls `send()` from within a notification callback during `renew_secure_channel`. Before fix, the test should deadlock. After fix, both the renewal and the notification call complete without blocking each other.

**Acceptance Scenarios**:

1. **Given** a secure channel renewal is in progress, **When** a notification callback triggers `send()`, **Then** the send operation completes without blocking on the renewal's lock.
2. **Given** the fixed implementation, **When** `renew_secure_channel` is called concurrently from multiple tasks, **Then** only one renewal proceeds; other callers wait without holding a lock across `.await`.
3. **Given** the fix (e.g., `watch::channel` or `tokio::sync::Notify`), **When** all existing tests run, **Then** they pass.

**Covers audit findings**: C-005

---

### User Story 4 — Replace SQLite Single-Mutex with Connection Pool (Priority: P1)

`SqliteHistoryBackend` uses a single `Arc<Mutex<Connection>>` that serializes all history I/O. Despite SQLite WAL mode supporting concurrent readers, the Mutex prevents any concurrency. Two concurrent `read_raw_modified` calls for different nodes serialize. A write operation holding a transaction blocks all readers. Under heavy history load (many concurrent clients reading history), throughput is capped at single-threaded SQLite performance.

**Why this priority**: This was the most cross-referenced finding (four independent agents flagged it). Under realistic OPC UA server workloads with historians, this bottleneck becomes the dominant latency source.

**Independent Test**: Run a benchmark with 16 concurrent history readers querying disjoint nodes. Before fix, aggregate throughput ≈ single-reader throughput (serialized). After fix, throughput scales with reader count (WAL mode concurrency).

**Acceptance Scenarios**:

1. **Given** multiple concurrent `read_raw_modified` calls for different nodes, **When** the fix is in place, **Then** reads execute concurrently without serializing on a single Mutex.
2. **Given** a write operation holds an exclusive SQLite transaction, **When** concurrent reads arrive, **Then** reads using separate pooled connections proceed without blocking (SQLite WAL semantics).
3. **Given** continuation point management, **When** pruning expired points, **Then** the continuation point store uses `DashMap` + periodic prune instead of a `Mutex<HashMap>`.
4. **Given** the fix, **When** all existing history tests run, **Then** they pass.

**Covers audit findings**: C-008, C-016, C-027

---

### User Story 5 — Fix Session Manager TOCTOU Patterns and O(n) Scans (Priority: P1)

Three session manager operations have correctness or performance issues:
- `close_session` extracts data under a read lock, drops the lock, awaits actor termination, then reacquires a write lock — session state can change between lock scopes.
- `commit_create_session_draft` has an O(n) eviction candidate scan + O(n) response body limit refresh under exclusive access. Every CreateSession blocks all other dispatches.
- `check_session_expiry` iterates all sessions (O(n)) with per-session read locks on the timer thread.
- `activate_session` holds a 76-line session read lock containing RSA signature verification (0.2–3ms hold) and nests `read(mgr)` inside `write(session)` in reverse order.

**Why this priority**: These are correctness and performance issues in the highest-frequency server operations. Under 10,000 concurrent sessions, the O(n) scans become millisecond-range blocking operations that affect all incoming requests.

**Independent Test**: Run a session lifecycle benchmark with N sessions. Measure P99 CreateSession and CloseSession latency. Before fix, P99 latency grows linearly with session count. After fix, P99 remains constant (O(1) operations).

**Acceptance Scenarios**:

1. **Given** a `close_session` call, **When** the session state changes between read-lock extraction and write-lock reacquisition, **Then** the write-lock code validates and handles stale state correctly (e.g., pre-computing `was_unactivated` flag, re-checking session existence).
2. **Given** a `CreateSession` request, **When** `commit_create_session_draft` runs, **Then** the eviction scan and response body limit refresh are O(1) or pre-computed under a read lock before the exclusive access section.
3. **Given** 10,000 active sessions, **When** `check_session_expiry` runs on the timer, **Then** it uses a time-ordered data structure (e.g., `BinaryHeap`) for O(log n) expiry instead of O(n) iteration.
4. **Given** an `activate_session` call, **When** RSA signature verification runs, **Then** the verification executes outside the manager and session read lock scopes.

**Covers audit findings**: C-009, C-010, C-014, C-017

---

### User Story 6 — Fix Client Transport Races and Cancel-Safety Issues (Priority: P1)

Three client-side issues:
- `set_client_offset` performs a non-atomic read-modify-write on `client_offset` ArcSwap. Currently safe only because it's serialized by the `issue_channel_lock` tokio mutex. A future call site would introduce a lost-update bug.
- `send()` loads the `request_send` channel sender, then `.await`s, then uses the sender. During reconnection, the old sender's receiver is dropped; requests fail with `BadConnectionClosed` when a new transport is available.
- `PendingClientDeliveryGuard::Drop` acquires a `parking_lot::Mutex` on the async thread during future cancellation, which can conflict with a publish retry loop.

**Why this priority**: These are real correctness issues in the client that manifest under specific timing conditions. The stale-channel TOCTOU affects every reconnection scenario. The Drop-lock-in-cancellation can cause unexpected blocking during clean shutdown.

**Independent Test**: For the stale channel sender: trigger reconnection while a `send()` is in progress. Before fix, the request fails. After fix, the request retries on the new transport. For the ArcSwap RMW: add a test calling `set_client_offset` from multiple tasks. Before fix, updates may be lost. After fix, all updates are preserved.

**Acceptance Scenarios**:

1. **Given** a reconnection occurs during a `send()` call, **When** the operation resumes after `.await`, **Then** it reloads the `request_send` channel sender and retries on the new transport.
2. **Given** `set_client_offset` is called, **When** multiple callers invoke it concurrently, **Then** each update is applied correctly (no lost updates) via `fetch_update` or proper atomic compare-and-swap.
3. **Given** a future holding `PendingClientDeliveryGuard` is cancelled, **When** the guard's `Drop` runs, **Then** it does not acquire a blocking lock on the async thread; state restoration happens explicitly before `.await` points.

**Covers audit findings**: C-011, C-012, C-013

---

### User Story 7 — Fix Subscription Cache Write Lock Scope (Priority: P1)

`create_subscription` holds a write lock on `SubscriptionCacheInner` during `SessionEntry::new()`, which spawns a tokio actor. The write lock blocks all concurrent subscription operations (read, modify, delete, notification dispatch). If `SessionEntry` construction were ever extended to await on actor initialization, this would become a deadlock. Additionally, two separate write lock acquisitions with an async gap create a TOCTOU window.

**Why this priority**: Write locks on the subscription cache are a shared resource. Every second spent inside the write lock stalls all 46 other lock acquisition sites in the same file. Moving construction outside the lock is a low-risk, high-impact fix.

**Independent Test**: Create subscriptions while notifications are actively dispatching at high frequency. Measure notification P99 latency during subscription creation. Before fix, P99 spikes during the write lock window. After fix, P99 remains stable.

**Acceptance Scenarios**:

1. **Given** a `create_subscription` call, **When** `SessionEntry` is constructed, **Then** the construction and actor spawn happen before the write lock is acquired on `SubscriptionCacheInner`.
2. **Given** the write lock is acquired, **When** the new entry is inserted into `session_subscriptions`, **Then** only the HashMap insertion happens inside the lock, and the lock is dropped immediately after.
3. **Given** the fix, **When** all existing subscription tests run, **Then** they pass.

**Covers audit findings**: C-015

---

### User Story 8 — Clean Up P2 Lock Type Inconsistencies and Scope Issues (Priority: P2)

Eighteen P2 findings cover lock type inconsistencies (`std::sync::Mutex` instead of `parking_lot::Mutex`), wide lock scopes (O(n) iterations under lock), TOCTOU in address space index building, two-phase write lock acquisitions in `teardown_session`, per-iteration write locks in program engine, and fragile atomic ordering on `should_reconnect`. These are individually minor but collectively represent technical debt in the locking discipline.

**Why this priority**: These are design improvements, not correctness issues. They reduce contention, improve code consistency, and eliminate future footguns. No behavior change is expected; all fixes are mechanical.

**Independent Test**: All existing tests pass. No lock-related flakiness in CI after fixes. Lock tracing shows reduced hold times in affected hot paths.

**Acceptance Scenarios**:

1. **Given** `first_request_signature: std::sync::Mutex<Vec<u8>>` in `secure_channel.rs:123`, **When** replaced with `parking_lot::Mutex`, **Then** the poisoning error handling is removed and lock acquisition is consistent with the rest of the codebase.
2. **Given** `ensure_browse_name_index` has a TOCTOU (read-then-write), **When** an inner double-check under write lock is added, **Then** two threads no longer both rebuild the index unnecessarily.
3. **Given** `build_browse_name_index` holds a write lock for O(nodes) time (10–100ms for 10K+ nodes), **When** optimized (e.g., drop lock during iteration, or pre-build outside lock), **Then** browse operations are not blocked during index construction.
4. **Given** `refresh_client_response_body_limit_for_channel` does O(n) session scans, **When** replaced with a per-channel `DashMap` lookup, **Then** it becomes O(1).
5. **Given** `teardown_session` acquires two separate write locks on `inner`, **When** merged into a single acquisition, **Then** the TOCTOU window between the two locks is eliminated.
6. **Given** program engine acquires `address_space.write()` per loop iteration, **When** batched into a single acquisition, **Then** browse readers are not starved during long-running programs.
7. **Given** `data_route_snapshot` holds a read lock during O(monitored_items) iteration, **When** the iteration clones a snapshot first (drop lock, then iterate clone), **Then** the read lock hold time is O(1) instead of O(items).
8. **Given** `create_monitored_items` holds a write lock for reverse index update proportional to batch size, **When** the reverse index is built outside the lock and swapped in, **Then** the write lock hold time is minimized.
9. **Given** `type_tree.write()` is acquired multiple times during startup import, **When** the pattern is documented as safe (single-threaded startup), **Then** future maintainers understand the assumption.
10. **Given** `should_reconnect` uses Relaxed ordering depending on mpsc channel happens-before, **When** the dependency is documented or upgraded to Release/Acquire, **Then** future refactors won't inadvertently break it.

**Covers audit findings**: C-019, C-020, C-021, C-022, C-023, C-024, C-025, C-026, C-028, C-029, C-030, C-032, C-033

---

### User Story 9 — Migrate Client SubscriptionState to Actor/Channel Model (Priority: P2)

The client `SubscriptionState` has 22–27 `Mutex` acquisition sites in a single file. A single mutex guards all subscription state including notification delivery callbacks which run inside the lock. An actor/channel model would eliminate all locks and improve clarity.

**Why this priority**: This is an architectural improvement with the largest code impact. While the current code is correct (no lock held across `.await`), the density of lock acquisitions creates fragility. The migration also addresses C-013 (Drop-lock-in-cancellation) since actor messages don't need Drop-time synchronization.

**Independent Test**: Run existing client subscription tests. The actor model should produce identical behavior. Measure lock acquisitions in the client subscription path — they should drop from 22+ to 0.

**Acceptance Scenarios**:

1. **Given** the `SubscriptionState` is converted to an actor, **When** subscription operations (create, modify, delete, set monitoring mode, set publishing mode) are invoked, **Then** they send messages to the actor instead of acquiring a Mutex.
2. **Given** notification delivery, **When** a publish response arrives, **Then** the notification is delivered through the actor's message handler without any lock acquisition.
3. **Given** the migration, **When** all existing client tests run, **Then** they pass with identical behavior.

**Covers audit findings**: C-018

---

### Edge Cases

- **Crypto offloading and `Send + 'static`**: The `spawn_blocking` closure must own all data it accesses. `SecureChannel` and session manager types must be restructured to support cloning `Arc<>` into the closure rather than borrowing.
- **SQLite connection pool sizing**: The pool must be sized appropriately for the target deployment. Too few connections re-create the bottleneck; too many waste resources. A configurable pool size (default 4) is appropriate.
- **Waker miss and double-wake**: Adding explicit wakes must not cause double-wake scenarios. The subscription actor's existing wake-on-ring-buffer logic must account for the new explicit wake path.
- **Channel renewal and notification re-entrancy**: The fix for C-005 must handle the case where a notification arrives during renewal. The replacement mechanism must allow notifications to proceed without waiting for renewal to complete.
- **Session TOCTOU and concurrent expiry**: In `close_session`, the session may be expired between read-lock extraction and write-lock reacquisition. The fix must handle the case where `commit_create_session_draft` took the session slot.
- **Backward compatibility**: All fixes must not change the public API. Existing users of the client and server SDK must compile and run without changes.
- **Performance regression**: Lock contention benchmarks from the audit benchmark design must be run before and after each fix to verify no performance regression.

## Functional Requirements

- **FR-001**: All RSA/ECC cryptographic operations (signing, verification, encryption, decryption, key generation) invoked from async contexts MUST run inside `tokio::task::spawn_blocking`.
- **FR-002**: Longer lock scopes containing crypto (e.g., `activate_session` 84-line read lock containing RSA verify) MUST be restructured so crypto runs outside the lock scope.
- **FR-003**: Symmetric crypto (AES/HMAC) for typical OPC UA chunk sizes (<64KB) MAY remain on async threads, with the assumption documented in code.
- **FR-004**: `push_pending_data_notifications` and `notify_data_change` MUST explicitly wake subscription actors after pushing notifications to ring buffers.
- **FR-005**: The `tokio::sync::Mutex` in `renew_secure_channel` MUST not be held across `.await`. Replacement mechanism MUST allow multiple waiters to subscribe to a single in-flight renewal without deadlock risk.
- **FR-006**: `SqliteHistoryBackend` MUST use a connection pool allowing concurrent reads via SQLite WAL mode.
- **FR-007**: Continuation point storage in the history backend MUST use a concurrent data structure (e.g., `DashMap`) instead of `Mutex<HashMap>`.
- **FR-008**: `close_session` MUST handle stale state after re-acquiring the write lock (TOCTOU-safe).
- **FR-009**: `commit_create_session_draft` eviction scan and response body limit refresh MUST be O(1) or pre-computed before the exclusive access section.
- **FR-010**: `check_session_expiry` MUST use an O(log n) data structure for session expiry instead of O(n) iteration.
- **FR-011**: RSA signature verification in `activate_session` MUST run outside the session and manager read lock scopes.
- **FR-012**: `set_client_offset` MUST use atomic `fetch_update` or proper compare-and-swap to eliminate the non-atomic read-modify-write.
- **FR-013**: `send()` in the client transport MUST reload `request_send` after `.await` in the reconnection path.
- **FR-014**: `PendingClientDeliveryGuard` MUST NOT acquire a blocking lock in its `Drop` implementation; state restoration MUST happen before `.await` points.
- **FR-015**: `create_subscription` MUST construct `SessionEntry` before acquiring the write lock on `SubscriptionCacheInner`.
- **FR-016**: `std::sync::Mutex<Vec<u8>>` (`first_request_signature`) MUST be replaced with `parking_lot::Mutex` for consistency.
- **FR-017**: `ensure_browse_name_index` MUST use double-check locking to prevent redundant index builds.
- **FR-018**: `build_browse_name_index` MUST NOT hold a write lock for the full O(nodes) iteration duration.
- **FR-019**: `refresh_client_response_body_limit_for_channel` MUST perform O(1) lookup instead of O(n) session scan.
- **FR-020**: `teardown_session` MUST use a single write lock acquisition instead of two with an async gap.
- **FR-021**: Program engine MUST batch address space write lock acquisitions instead of acquiring per iteration.
- **FR-022**: `data_route_snapshot` MUST clone data under the lock and iterate outside, reducing read lock hold time from O(items) to O(1).
- **FR-023**: `create_monitored_items` reverse index update MUST minimize write lock hold time.
- **FR-024**: `should_reconnect` ordering dependency on mpsc happens-before MUST be documented or upgraded to Release/Acquire.
- **FR-025**: Client `SubscriptionState` MUST be migrated to an actor/channel model, eliminating all 22–27 lock acquisition sites.
- **FR-026**: All existing tests (618+) MUST continue to pass after each fix.
- **FR-027**: The `spawn_blocking` thread pool MUST not be exhausted under expected load (configurable pool size, default adequate).

## Success Criteria

- **SC-001**: Under a 50-client simultaneous connection storm, P99 OpenSecureChannel latency remains within 2x of median latency (no crypto-induced runtime stalling).
- **SC-002**: Notification delivery latency for a monitored item change is under 10ms for a 100ms publishing interval (no one-interval waker-miss delay).
- **SC-003**: A notification callback during secure channel renewal does not deadlock.
- **SC-004**: 16 concurrent disjoint history readers achieve at least 4x the throughput of a single reader (WAL concurrency scaling).
- **SC-005**: P99 CreateSession and CloseSession latency does not grow with session count at 10,000 active sessions (O(1) operations).
- **SC-006**: `set_client_offset` preserves all updates when called concurrently from multiple tasks.
- **SC-007**: `send()` successfully retries on a new transport after reconnection without returning `BadConnectionClosed`.
- **SC-008**: Future cancellation of a pending client delivery does not acquire a blocking lock.
- **SC-009**: Notification P99 latency during subscription creation remains stable (no write-lock-induced spike).
- **SC-010**: All 618+ existing tests pass without modification.
- **SC-011**: Lock tracing (`OPCUA_TRACE_LOCKS=1`) shows reduced hold times in: subscription cache write lock, session manager write lock, address space write lock, and browse name index build path.
- **SC-012**: Client subscription operations have zero `Mutex` acquisitions in the hot path after actor migration.

## Assumptions

- SQLite is configured in WAL mode (journal_mode=WAL) for the history backend, enabling concurrent readers.
- The `spawn_blocking` thread pool (default 512 threads) is adequate for crypto offloading under expected connection rates; pool exhaustion is handled by tokio's blocking thread spawn behavior.
- Per-channel response body limits change infrequently enough that a `DashMap` lookup (instead of O(n) scan) is semantically equivalent.
- The program engine runs infrequently enough that batching write lock acquisitions does not affect program execution correctness.
- `SubscriptionState` actor migration preserves the existing public API contract; internal state representation changes do not affect external behavior.
- Symmetric crypto hold times (<100µs for typical chunks) are acceptable on async threads barring profiling evidence to the contrary.
