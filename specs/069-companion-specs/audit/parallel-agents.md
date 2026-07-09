# Contention Benchmark Design

## 1. Overview

This document specifies a high-contention benchmark harness for the `async-opcua`
server/client Rust codebase. The objective is to measure lock contention
characteristics on the four hottest code paths and quantify cross-path interference.
Results will guide lock-splitting, queue-depth tuning, and sharding decisions.

**Scope**: Server-internal lock paths, not wire-protocol benchmarks.
**Non-goal**: End-to-end OPC UA compliance benchmarks (those exist in existing
integration tests).

## 2. Hot Path Analysis

### 2.1 Session Lookup & Lifecycle

**Lock sites**:
- `SessionManager.auth_tokens: DashMap<NodeId, Arc<RwLock<Session>>>` — sharded,
  `find_by_token` is near lock-free for disjoint tokens.
- `Arc<RwLock<Session>>` — **per-session write contention**. During
  `activate_session`, the target session's write lock is held for the full
  duration of signature verification, identity resolution, and role resolution.
  This blocks concurrent service calls for the same session.
- `activate_session` holds `mgr_lck: RwLock<SessionManager>` as read lock —
  shared across all activations, low contention.
- `commit_create_session_draft` requires `&mut self` on `SessionManager` —
  serializes all CreateSession calls. When sessions expire en masse (e.g., DDoS
  recovery), the session cleanup path iterates all sessions holding the write
  lock on the manager.

**Contention model**:
- DashMap token lookup: ~O(1), shard count scales with concurrency. Bottleneck
  only under extreme collision (same shard).
- Per-session `Arc<RwLock<Session>>`: When N service calls target the same
  session simultaneously, all but one wait on the write lock. For read-dominant
  workloads (read, browse) this is fine; for write-mix (activate, close) it
  serializes.
- Manager write lock: `create_session` and `expire_session` are serialized
  globally. Under 1000+ creates/sec, this becomes the bottleneck.

### 2.2 History Reads & Writes (SQLite Backend)

**Lock sites**:
- `SqliteHistoryBackend.connection: Arc<Mutex<Connection>>` — all read/write SQL
  operations serialize on this single mutex.
- `SqliteHistoryBackend.continuation_points: Arc<Mutex<HashMap<...>>>` — separate
  mutex for continuation point storage, acquired briefly.
- `tokio::task::spawn_blocking` — every SQLite operation is dispatched to the
  blocking thread pool. High-contention scenarios can saturate `spawn_blocking`
  threads, causing async task starvation.

**Contention model**:
- The `Mutex<Connection>` is the primary bottleneck. All history reads and writes
  are serialized through a single SQLite connection. Under concurrent history
  reads (e.g., 10 clients each reading different nodes), throughput is capped
  at the speed of sequential SQLite queries.
- `spawn_blocking` thread pool exhaustion: If more concurrent queries are issued
  than there are `spawn_blocking` threads, the excess queue in tokio's global
  blocking pool, delaying all async operations that depend on history.
- Continuation point mutex is negligible (short critical sections).

### 2.3 Subscription Dispatch (Server Side)

**Lock sites**:
- `SubscriptionCache.inner: RwLock<SubscriptionCacheInner>` — one
  `parking_lot::RwLock` protecting all subscription state.
  - **Read lock**: route lookup for data notifications, enqueue publish, get
    session subscriptions, modify/republish/set_publishing_mode.
  - **Write lock**: `create_subscription` (inserts `session_subscriptions` entry,
    inserts `subscription_to_session` mapping), `delete_monitored_item_refs`
    (per-item cleanup), `run_cleanup` (remove expired subscriptions).
- `SubscriptionActorHandle` — internal actor channels. Not a lock but subject to
  queue depth backpressure.

**Contention model**:
- `create_subscription` acquires `write_lock` **twice**: once to ensure the
  `SessionEntry` exists, and again after the actor creates the subscription to
  insert `subscription_to_session`. Between these two locks, the actor does
  async work (spawning a subscription actor). This means the write lock is
  held, released, and re-acquired — a gap where another create could interleave.
- Under heavy notification load (100k+ monitored items firing), the route
  lookup `read_lock` dominates. Each `data_notifier` drop acquires the read
  lock **per node** to look up routes _and again_ in `notify_for` via
  `data_route_snapshot`.
- Write lock starvation: If notifications are constant (many read-locks held),
  `create_subscription` and cleanup can be starved of the write lock for
  extended periods. `parking_lot::RwLock` uses a task-fair algorithm but under
  sustained read pressure, writers can still be delayed.

### 2.4 Client Subscription State

**Lock sites**:
- `SubscriptionState` wrapped in `Mutex<SubscriptionState>` — acquired on
  **every** subscription operation: create, modify, delete, set_publishing_mode,
  set_triggering, publish response processing, notification delivery.

**Contention model**:
- The Mutex serializes all subscription operations within a single client
  session. When a client manages 50+ subscriptions each receiving frequent
  notifications, the Mutex hold time for `on_subscription_notification`
  includes callback execution — user code runs inside the lock.
- The `PendingClientDeliveryGuard` pattern moves work outside the lock for
  notification delivery, but the initial capture and final restore both
  acquire the lock.

---

## 3. Benchmark Scenarios

### 3.1 Session Lifecycle Contention

**Purpose**: Measure throughput of CreateSession/ActivateSession/CloseSession
under concurrent load. Identify whether the `SessionManager` `&mut self` commit
or per-session write lock is the primary bottleneck.

**Setup**:
- Server: in-memory (no SQLite), anonymous auth, no subscriptions.
- One endpoint: `opc.tcp://127.0.0.1:4850`.
- Pre-configured server with `max_sessions = 10000`.

**Workloads**:

| Scenario | Operation Mix | Concurrency | Duration | Description |
|----------|--------------|-------------|----------|-------------|
| Create-only burst | 100% CreateSession | 1, 10, 50, 100 | 30s | Measure pure create throughput, serialized on manager write lock |
| Create+Activate+Close cycle | Create → Activate → Close (one session) | 100, 500, 1000 sessions total | 30s | Measure full lifecycle throughput |
| Token lookup under load | find_by_token only (pre-populated) | 1, 10, 100 threads | 30s | Micro-benchmark DashMap contention |
| Hot-session reads | Read on same session | 10, 50, 100 concurrent reads | 10s | Per-session RwLock read scaling |
| Hot-session mixed | 50% read, 30% write, 20% browse on same session | 10, 50, 100 | 10s | Per-session RwLock read/write contention |

**Metrics**:
- Creates/sec, Activates/sec, Closes/sec.
- P50/P95/P99 session manager write-lock hold time.
- P50/P95/P99 per-session write-lock hold time.
- Token lookup latency distribution (DashMap get + Arc::clone).

**Expected baseline**: Create-only ~2000-5000 ops/sec (limited by `&mut self`
serialization and actor spawn overhead). Token lookup ~10-20M ops/sec
(DashMap is sharded, near lock-free).

**Target**: Token lookup < 100ns P99. CreateSession > 5000 ops/sec with 50
concurrent creators. Per-session read scaling near-linear to 4x core count.

### 3.2 History Read Contention

**Purpose**: Measure history read throughput degradation under concurrent readers
and reader-writer interference. Quantify `spawn_blocking` saturation point.

**Setup**:
- SQLite in-memory database.
- Pre-populated with 1000 nodes × 10,000 values each = 10M historical data
  points. Each value is a simple Double.
- History reads request 1000-value pages with `num_values_per_node = 0`
  (unbounded, single page per request).
- Separate tasks for reading and writing.

**Workloads**:

| Scenario | Concurrent Readers | Concurrent Writers | Node Distribution | Duration |
|----------|-------------------|--------------------|--------------------|----------|
| Read scaling | 1, 4, 8, 16, 32, 64 | 0 | Disjoint nodes | 30s |
| Same-node reads | 1, 4, 8, 16 | 0 | All read same node | 30s |
| Read-write contention | 8 | 1, 2, 4 | Mixed | 30s |
| Write-during-continuation | 4 readers with cp | 2 writers | Same node | 30s |
| spawn_blocking saturation | 64, 128, 256 | 0 | Disjoint nodes | 15s |

**Metrics**:
- History reads/sec (aggregate and per-reader).
- P50/P95/P99 `Mutex<Connection>` lock acquisition time.
- P50/P95/P99 query execution time (from lock acquire to result).
- `spawn_blocking` queue depth (peak and sustained).
- Continuation point Mutex hold time.
- Write success rate and write latency under read load.

**Expected baseline**: Single-reader single-writer: ~500-2000 reads/sec
(depending on page size and query complexity). Under 16 disjoint concurrent
readers: near-linear degradation (serialized through single Mutex), ~same
aggregate throughput as single reader.

**Target**: Under 16 concurrent disjoint readers, aggregate throughput should
not drop more than 20% vs single reader. This requires splitting the single
Mutex into a connection pool or WAL-mode multi-reader support.

### 3.3 Subscription Dispatch Contention

**Purpose**: Measure notification throughput limitations caused by
`RwLock<SubscriptionCacheInner>` contention. Test both data notifications and
subscription lifecycle operations.

**Setup**:
- Full server with subscriptions enabled.
- 1 session with 1 subscription.
- Variable monitored item count: 10, 100, 1000, 10000.
- All items on a single variable node that updates at a controlled rate.
- Notification dispatch via `data_notifier().notify_for(...)`.

**Workloads**:

| Scenario | Items | Update Rate (Hz) | Concurrent Creates | Duration |
|----------|-------|-------------------|---------------------|----------|
| Single-item notification throughput | 1 | max (tight loop) | 0 | 10s |
| Batch notification scaling | 100 | max | 0 | 10s |
| Mass notification | 10,000 | max | 0 | 10s |
| Notification + create contention | 100 | 1000 | 1, 5, 10 creates/sec | 30s |
| Create subscription burst | N/A | N/A | 100, 500 concurrent creates | 30s |
| Mixed subscription ops | 500 items | 500 | 5 creates + 5 deletes + 5 mods per sec | 30s |
| Multi-session subscription isolation | 100/session × 50 sessions | 100/session | 0 | 30s |

**Metrics**:
- Notifications/sec (aggregate notifications delivered to subscription actors).
- P50/P95/P99 `data_notifier` critical section time (route lookup).
- P50/P95/P99 `create_subscription` total time (including both write-lock phases).
- `create_subscription` write-lock acquisition latency (time from request to
  first write-lock acquired).
- Inter-write-lock gap time (between releasing first write lock and acquiring
  second in `create_subscription`).
- `SubscriptionActorHandle` queue depth (peak, sustained).

**Expected baseline**: Single-item tight-loop notification: ~50k-200k
notifications/sec (limited by RwLock read acquire + route lookup + actor
channel send). 10k-item mass notification: ~500-1000 batch operations/sec
(each notifying 10k items). Under concurrent creates: write-lock starvation
causes create latency spikes beyond 100ms.

**Target**: Mass notification throughput > 5000 notifications/sec with 10k
items. Create subscription P99 < 10ms even under sustained notification load.
This requires lock-splitting: separate RW locks for `monitored_items` (route
lookup) vs `session_subscriptions`/`subscription_to_session` (lifecycle).

### 3.4 Mixed Workload End-to-End Contention

**Purpose**: Measure cross-contention — whether blocking history operations
degrade subscription notification latency, and whether session lifecycle
operations interfere with subscription dispatch.

**Setup**:
- Full server with subscriptions + history (in-memory SQLite).
- 50 active sessions, each with 2 subscriptions and 50 monitored items.
- Background notification loop (1000 updates/sec across 500 nodes).
- Concurrent history reads (8 readers, different nodes, 1000-value pages).
- Periodic session creates (1/sec) and closes (1/sec).

**Workloads**:

| Scenario | Sessions | Notification Rate | History Readers | Session Creates/sec | Duration |
|----------|----------|-------------------|-----------------|---------------------|----------|
| Steady-state mixed | 50 | 1000/sec | 8 | 1 | 60s |
| History storm | 50 | 1000/sec | 64 | 0 | 30s |
| Subscription storm | 10 → 100 sessions in 10s | 100 → 10,000/sec | 4 | 10/sec | 30s |
| Session storm | 100 | 500/sec | 4 | 100/sec burst | 30s |

**Metrics**:
- Per-component throughput (notifications/sec, reads/sec, creates/sec)
  measured simultaneously.
- Cross-contention latency correlation: P99 notification latency vs P99
  history read latency, measured in the same time windows.
- Tokio worker thread utilization (% time blocked vs polling).
- `spawn_blocking` thread saturation during history + notification overlap.
- Memory usage under sustained mixed load.

**Expected baseline**: Under steady-state mixed, notification P99 latency
should remain < 5ms since notification dispatch only needs a short
read-lock on the subscription cache. History reads run on `spawn_blocking`,
so they should not directly block async tasks — but they can saturate
tokio's blocking thread pool, causing backpressure on new history requests
and potentially affecting the global scheduler if all workers are busy.

**Target**: Cross-contention impact < 10%: notification P99 latency should
not increase more than 10% when history reads are added to the workload.

---

## 4. Instrumentation Plan

### 4.1 Criterion Benchmarks (Micro-benchmarks)

**Location**: `async-opcua-server/benches/` and `async-opcua-history-sqlite/benches/`

- **`session_lookup.rs`** (existing): Extend with concurrent lookup scenarios.
  Add benches for `activate_session` under varying concurrency using
  `tokio::runtime::Runtime` with multi-threaded scheduler.
- **`session_contention.rs`** (new): Create + Activate + Close cycle
  micro-benchmark. Pre-build server infrastructure, measure cycle throughput
  with 1-100 concurrent clients.
- **`history_read.rs`** (new): Single-threaded and concurrent history reads.
  Use `criterion::BenchmarkGroup` with throughput measurement.
  Pre-seed SQLite with configurable data sizes.
- **`subscription_dispatch.rs`** (new): Create subscription, create monitored
  items, then measure `data_notifier().notify()` throughput with controlled
  item counts.

**Key criterion configuration**:
```toml
[dev-dependencies]
criterion = { workspace = true, features = ["html_reports"] }

[[bench]]
name = "session_contention"
harness = false

[[bench]]
name = "history_read"
harness = false

[[bench]]
name = "subscription_dispatch"
harness = false
```

### 4.2 Custom Stress Test Harness

**Location**: `tools/opcua-contention-bench/` (new binary crate)

A standalone binary that:
1. Starts an in-process server with configurable limits.
2. Spawns N client tasks, each managing S sessions/subscriptions.
3. Runs a controlled workload defined by a JSON/YAML scenario file.
4. Collects metrics at configurable intervals.
5. Outputs JSON metrics to stdout (compatible with the existing
   `opcua-localhost-bench` JSON format).

**Scenario file format** (proposed `scenarios/session_storm.toml`):
```toml
[server]
port = 4860
max_sessions = 10000
max_subscriptions_per_session = 100

[[phases]]
name = "warmup"
duration_secs = 10
concurrent_clients = 10
operations.create_session_per_sec = 10.0

[[phases]]
name = "burst"
duration_secs = 30
concurrent_clients = 100
operations.create_session_per_sec = 100.0
operations.activate_session_per_sec = 100.0
operations.close_session_per_sec = 50.0

[metrics]
output_interval_secs = 1
collect_lock_traces = true
collect_tokio_stats = true
```

### 4.3 Lock Tracing via `OPCUA_TRACE_LOCKS`

The existing `trace_lock!` / `trace_read_lock!` / `trace_write_lock!` macros
already emit `tracing::trace!` events when `OPCUA_TRACE_LOCKS=1`.

**Enhancements needed for benchmarking**:
1. Add structured fields to trace events: `lock_name`, `file`, `line`,
   `wait_duration_ns`, `hold_duration_ns`.
2. Add a benchmark-only feature flag `lock-metrics` that records histograms
   of lock acquisition and hold times to a shared `metrics::Histogram` registry,
   even when `OPCUA_TRACE_LOCKS` is not set (to avoid the overhead of full
   tracing in benchmarks).
3. Expose a `LockMetricsSnapshot` that the benchmark harness can sample:
   ```rust
   pub struct LockMetrics {
       pub lock_name: &'static str,
       pub acquisitions: AtomicU64,
       pub wait_ns: AtomicU64,
       pub hold_ns: AtomicU64,
       pub max_wait_ns: AtomicU64,
       pub max_hold_ns: AtomicU64,
   }
   ```

### 4.4 tokio-console Integration

For runtime-level observability during stress tests:
- Enable `tokio_unstable` and `console-subscriber` in the benchmark harness.
- Use `tokio-console` to observe:
  - Tokio worker utilization (busy vs idle time).
  - `spawn_blocking` thread pool saturation.
  - Task polling latency (time between wake and poll).
  - Budget exhaustion events (when tasks yield due to budget).

**Integration approach**: The benchmark harness spawns the server with
`console_subscriber::init()` and the operator attaches `tokio-console` to
the process via a TCP port during manual profiling runs.

### 4.5 perf/Flamegraph Integration

For CPU-level profiling of lock contention:
- Use `perf record -g -F 99 -p <pid>` during benchmark runs to capture call
  stacks.
- Generate flamegraphs with Brendan Gregg's FlameGraph tools.
- In CI, use `cargo-flamegraph` for automated profiling of criterion benches.

**Recommended profiling command**:
```bash
perf record -g -F 997 -e cpu-clock,lock:lock_acquire,lock:lock_contended \
  -p $PID -o perf.data --call-graph dwarf -- sleep 30
perf script | stackcollapse-perf.pl | flamegraph.pl > contention.svg
```

---

## 5. Implementation Notes

### 5.1 Crate Structure

```
tools/opcua-contention-bench/
  Cargo.toml
  src/
    main.rs          -- CLI entry point, scenario runner
    scenario.rs      -- TOML scenario parser
    server.rs        -- In-process server builder
    client.rs        -- Multi-session client task
    metrics.rs       -- Metrics collection and aggregation
    reporter.rs      -- JSON output formatting
  scenarios/
    session_storm.toml
    history_read_storm.toml
    subscription_flood.toml
    mixed_workload.toml
```

### 5.2 Dependencies

```toml
[dependencies]
async-opcua = { path = "../../async-opcua", features = ["all", "json"] }
async-opcua-history-sqlite = { path = "../../async-opcua-history-sqlite" }
tokio = { workspace = true, features = ["full", "tracing"] }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
toml = "0.8"
hdrhistogram = "7.5"              # latency histograms
tracing = { workspace = true }
tracing-subscriber = "0.3"
parking_lot = { workspace = true }
console-subscriber = { version = "0.4", optional = true }

[features]
tokio-console = ["console-subscriber", "tokio/tracing"]
lock-metrics = ["async-opcua/lock-metrics"]  # proposed new feature in async-opcua-core
```

### 5.3 Integration with Existing Test Infrastructure

- **Criterion benchmarks**: Add to existing `async-opcua-server/benches/` and
  `async-opcua-history-sqlite/` test structure. Run via `cargo bench`.
- **Stress harness**: New `tools/` binary, similar to `tools/opcua-localhost-bench`.
  Run via `cargo run --release -p async-opcua-contention-bench -- scenario --file scenarios/session_storm.toml`.
- **CI integration**: Add a new CI job that runs the stress harness with a
  short-duration scenario (5-10 minutes) on each PR. Use `tools/ci-playbook.sh`
  patterns for result thresholds.
- **Regression detection**: Store P50/P95/P99 baseline numbers in a JSON file
  committed to the repo. The CI job compares current results and fails if
  latency increases more than 20% or throughput drops more than 10%.

### 5.4 Feature Flags (Proposed)

New feature flags in `async-opcua-core`:

- `lock-metrics`: Enables `LockMetrics` counters (zero-overhead when
  disabled). Uses `AtomicU64` counters updated after each lock operation.
  Exposed via `opcua_core::lock_metrics::snapshot()`.
- `lock-timing`: Enables `Instant::now()` timing around each lock operation.
  Higher overhead than `lock-metrics`, suitable for benchmarks only.

### 5.5 Data Population Utilities

The benchmark harness should include helpers for:
- `seed_history_data(backend, node_count, values_per_node)` — bulk-insert
  historical data for N nodes.
- `create_monitored_items_bulk(session, subscription_id, node_ids)` — create
  monitored items for many nodes in a single batch.
- `wait_for_stable_load(threshold_percent, window_secs)` — stabilize detector
  that waits until throughput variance drops below threshold before starting
  measurement.

### 5.6 Expected File Changes Summary

| Change | Crate | Description |
|--------|-------|-------------|
| New crate | `tools/opcua-contention-bench` | Stress test harness binary |
| New bench | `async-opcua-server/benches/session_contention.rs` | Session lifecycle criterion bench |
| New bench | `async-opcua-history-sqlite/benches/history_read.rs` | History read criterion bench |
| New bench | `async-opcua-server/benches/subscription_dispatch.rs` | Subscription dispatch criterion bench |
| Modified | `async-opcua-core/src/lib.rs` | Add `lock_metrics` module behind feature flag |
| Modified | `async-opcua-core/src/lock_metrics.rs` | New: LockMetrics histogram registry |
| Modified | `async-opcua-core/Cargo.toml` | Add `lock-metrics` feature flag |
| New files | `tools/opcua-contention-bench/scenarios/*.toml` | Scenario definitions |
