# Feature Specification: Hot-Path and Lock Optimization

**Feature Branch**: `063-hot-path-and-locks`  
**Created**: 2026-07-05  
**Status**: Draft  
**Input**: User description: "Optimize hot-path CPU overhead identified in feature 062 profiling — remove redundant locking, cache lookup results, consolidate per-request timers, and investigate ArcSwap debt."

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Eliminate Redundant AddressSpace Locking (Priority: P1)

The OPC UA server's `AddressSpace` wraps a `DashMap` (already lock-free inside) with an outer `Arc<RwLock<>>`. Every Read operation acquires the outer `parking_lot::RwLock::read()` just to call `DashMap::get()` — pure overhead that causes cross-core cache-line bouncing of the lock state word. Reads under load should not contend on a single lock word across cores.

**Why this priority**: This is the highest-frequency hot path — every client read hits AddressSpace. Removing the redundant lock eliminates cache-line contention for all concurrent readers, delivering per-core throughput wins at scale. The `namespaces`, `references`, and `browse_name_index` fields are not touched by simple Read operations, so they can be factored into a cold struct.

**Independent Test**: Run the `localhost-bench` read benchmark on a multi-core machine before and after the change. Verify that per-core throughput increases and that `perf` shows no `parking_lot::RwLock::read` contention on the read path.

**Acceptance Scenarios**:

1. **Given** the server is handling concurrent Read requests, **When** profiling the read path, **Then** `parking_lot::RwLock::read()` on AddressSpace no longer appears in the hot path, replaced by direct `DashMap::get()` calls.
2. **Given** the AddressSpace has been split into hot/cold components, **When** a Read arrives, **Then** it accesses only the `Arc<DashMap<NodeId, NodeType>>` without acquiring any lock.
3. **Given** a Write or namespace modification, **When** the server writes to `references` or `browse_name_index`, **Then** the cold fields remain accessible through the original `RwLock` path without changes.

---

### User Story 2 — Cache Session Arc in Request Dispatch Context (Priority: P2)

`SessionManager::find_by_token` costs ~2.4% CPU per request. The token-to-session mapping does not change during a single request's lifetime, yet the lookup is repeated for each operation on the same request. The looked-up `Arc<RwLock<Session>>` should be cached in the request dispatch context and reused across subsequent operations.

**Why this priority**: Eliminates ~2.4% CPU for every request by avoiding a redundant hash-table lookup. The cached lookup has no correctness risk because the token→session mapping is stable during a request's lifetime.

**Independent Test**: Profile a high-throughput read benchmark. Confirm that `SessionManager::find_by_token` no longer appears in the top CPU consumers per request.

**Acceptance Scenarios**:

1. **Given** a request being dispatched, **When** `find_by_token` is called for the first time on a request, **Then** the result is cached in the request context.
2. **Given** a subsequent operation on the same request, **When** it needs the session, **Then** it retrieves the cached `Arc<RwLock<Session>>` without calling `find_by_token`.
3. **Given** the session is terminated or the request completes, **When** the request context is dropped, **Then** the cached session Arc is released.

---

### User Story 3 — Replace Per-Request Timers with Shared Deadline Queue (Priority: P3)

Each inflight request spawns a `tokio::time::sleep_until` that costs ~2.8% CPU (`TimerEntry::drop` + `TimerEntry::reset` per operation). Replace the per-request timers with a single shared deadline queue that is checked once per event loop tick in `controller.rs::run()`.

**Why this priority**: Consolidates timer overhead from O(n) (one timer per inflight request) to O(1) (one queue check per tick), reducing CPU pressure on the tokio runtime's timer wheel.

**Independent Test**: Run a benchmark with many concurrent inflight requests. Profile the timer subsystem; confirm `TimerEntry::drop`/`TimerEntry::reset` no longer dominate CPU time.

**Acceptance Scenarios**:

1. **Given** N inflight requests, **When** the event loop ticks, **Then** at most one deadline check is performed per tick instead of N individual `sleep_until` futures.
2. **Given** a request exceeds its deadline, **When** the shared queue is checked, **Then** the request is timed out and a timeout response is sent.
3. **Given** the shared deadline queue implementation, **When** a request completes before its deadline, **Then** its entry is lazily cleaned (no explicit cancellation overhead).

---

### User Story 4 — Investigate and Resolve ArcSwap Debt Overhead (Priority: P4)

`arc_swap::Debt::pay_all` consumes ~2.5% CPU in the hot path. ArcSwap wraps shared state (likely `Arc<ServerInfo>` or diagnostics configuration). If writes to this state are rare, the overhead of ArcSwap's slot-based debt mechanism may be avoidable.

**Why this priority**: Investigative task — the 2.5% overhead is real but the root cause and fix need to be confirmed before implementation. May be resolved by replacing ArcSwap with a plain `Arc` + occasional atomic reload, or by changing the access pattern.

**Independent Test**: Profile the system to identify which ArcSwap instances consume debt overhead. If a viable replacement is found, implement it. Verify the ~2.5% overhead is eliminated or significantly reduced.

**Acceptance Scenarios**:

1. **Given** a profiling session, **When** investigating `arc_swap::Debt::pay_all`, **Then** the specific ArcSwap instance(s) responsible are identified.
2. **Given** the identified instance, **When** a replacement is implemented (e.g., plain `Arc` + atomic reload), **Then** correctness is preserved (all existing tests pass).
3. **Given** the fix is applied, **When** profiling again, **Then** `arc_swap::Debt::pay_all` CPU time is reduced by at least 50%.

---

### Edge Cases

- **Concurrent reads/writes to split AddressSpace**: Cold field writes must not block hot-path reads. The hot `DashMap` stays lock-free; cold fields remain behind `RwLock` but their access is infrequent.
- **Session termination during request dispatch**: Cached session Arc must survive session termination gracefully — the `Arc` keeps the session alive while the request holds it.
- **Deadline queue overflow under load**: The shared deadline queue must handle worst-case inflight request counts without unbounded growth.
- **ArcSwap replacement correctness**: The RCU-like semantics of ArcSwap must be preserved if the data is updated at runtime (e.g., diagnostics config changes).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST expose `AddressSpace.node_map` (`DashMap`) directly on the hot read path without requiring an outer `RwLock` acquisition.
- **FR-002**: The server MUST retain `Arc<RwLock<>>` protection for `AddressSpace`'s cold fields (`references`, `browse_name_index`, `namespaces`).
- **FR-003**: The server MUST cache the `Arc<RwLock<Session>>` returned by `find_by_token` in the request dispatch context for reuse within a single request's lifetime.
- **FR-004**: The server MUST replace per-request `tokio::time::sleep_until` timers with a single shared deadline queue checked once per event loop tick.
- **FR-005**: The server MUST identify and investigate `arc_swap::Debt::pay_all` overhead sources and apply a performance fix if viable.
- **FR-006**: All existing functional tests (unit, integration, interop) MUST continue to pass after each optimization.
- **FR-007**: The `localhost-bench` read benchmark throughput MUST not regress (and ideally improve).
- **FR-008**: All optimizations MUST maintain correctness of the OPC UA protocol layer per OPC-10000-4 (Services) and OPC-10000-6 (Mappings).

### Key Entities

- **AddressSpace**: The server's node repository. Currently `Arc<RwLock<AddressSpace>>` where `AddressSpace` contains `DashMap<NodeId, NodeType>` (hot), plus `references`, `browse_name_index`, and `namespaces` (cold).
- **SessionManager**: Manages authenticated sessions. `find_by_token` performs an `ArcSwap`-backed lookup returning `Arc<RwLock<Session>>`.
- **DeadlineQueue**: A shared, ordered collection of (deadline, request_id) pairs checked once per event loop tick in `controller.rs::run()`.
- **InflightRequest**: A pending service request tracked by the controller, currently using an individual `tokio::time::sleep_until`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The `localhost-bench` read benchmark shows measurable throughput improvement (at least 3% combined, summing individual gains from each of the 4 optimizations) on a multi-core machine.
- **SC-002**: `parking_lot::RwLock::read` contention on AddressSpace drops to zero on the read path (confirmed by perf).
- **SC-003**: `SessionManager::find_by_token` CPU time per request drops to zero (confirmed by perf, as it's cached after first call).
- **SC-004**: `tokio::time::TimerEntry` drop/reset CPU overhead drops by at least 50% (confirmed by perf).
- **SC-005**: `arc_swap::Debt::pay_all` CPU overhead drops by at least 50% OR a documented finding explains why it cannot be reduced.
- **SC-006**: All CI checks pass (cargo test, clippy, format, interop: node-opcua 71/71, interop: open62541 34/34).
- **SC-007**: No new allocations or memory regressions in the hot path (confirmed by `dhall` or equivalent heap profiling).

## Assumptions

- The `namespaces`, `references`, and `browse_name_index` fields are indeed cold (read-only or rarely written after startup) as indicated in the TODO.md lock audit finding.
- The `ArcSwap` instances identified are for configuration data that changes rarely, making a plain `Arc` with occasional atomic reload viable.
- The tokio runtime version provides `sleep_until` semantics that can be replaced without affecting timeout accuracy.
- Existing benchmarks in `tools/opcua-localhost-bench` provide adequate before/after measurement capability.
