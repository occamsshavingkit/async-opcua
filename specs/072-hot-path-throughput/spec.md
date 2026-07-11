# Feature Specification: Hot-Path Per-Request Throughput

**Feature Branch**: `072-hot-path-throughput`
**Created**: 2026-07-10
**Status**: Draft
**Input**: User description: "Reduce async-opcua's per-request overhead so single-core throughput comes close to peer OPC UA servers, and extend the multi-core linear-scaling region — without changing wire behavior or weakening correctness. The initial lock-removal hypothesis was investigated and refuted; the verified cause is per-request async-architecture overhead."

## Overview

A benchmark comparing async-opcua to peer OPC UA servers found it processes a single client's
requests **~3× slower** than minimal peers (async-opcua 68,968 read ops/s vs 168,915 and 204,128 for
the peers). Investigation refuted an initial "remove a lock" hypothesis and established the real cause:
async-opcua performs a large amount of **per-request work** — heap allocations, an internal
message-passing round-trip per request, a per-request timer, repeated parsing of already-known message
headers, and per-request clock reads — that the minimal peer servers do not.

This feature reduces that trimmable per-request work so single-core throughput comes **close** to the
peers, and extends how far the server scales near-linearly as cores are added. It is a performance and
resource-efficiency change only: it must not alter any observable protocol behavior (byte-identical
wire output; the conformance matrix stays green) and must not weaken correctness or security.

Every change is **measured before/after** and accepted only if the measurement shows a real
improvement (or is neutral for a safety cleanup) — never on projection.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Single-core per-request reduction (Priority: P1)

An operator runs the server on a single core serving one busy client. The server should spend
meaningfully less effort per request, so its single-client throughput comes close to the peer servers
rather than trailing them ~3×.

**Why this priority**: This is the largest and most clearly diagnosable gap (single client = no
contention, so it is purely per-request cost), and it is where the bulk of the improvement lives.

**Independent Test**: On one pinned CPU core, drive the server with a single serial client issuing reads
(and writes) and compare throughput before and after the change.

**Acceptance Scenarios**:

1. **Given** a single client issuing serial reads against a single-core server, **When** the per-request
   reductions are applied, **Then** single-client read throughput increases by at least the target
   margin over the current baseline, with no correctness or protocol change.
2. **Given** the same setup for writes, **When** the reductions are applied, **Then** single-client write
   throughput increases similarly.
3. **Given** any applied reduction, **When** it is measured, **Then** it is kept only if it improves or is
   neutral for single-client throughput — a reduction that regresses throughput is reverted.

*This story is delivered in two stages:* a first stage of **low-risk cuts that always land** (removing
per-request allocations, redundant work, and unnecessary clock reads), and a second stage of a **higher-
leverage read fast-path** that is kept only if measurement shows it helps and only with the server's
crash-isolation and request-cancellation behavior fully preserved.

---

### User Story 2 - Multi-core linear scaling (Priority: P2)

An operator runs the server across many cores under concurrent client load. The server should keep
scaling near-linearly for more cores before its throughput plateaus, so added hardware keeps yielding
added capacity.

**Why this priority**: Multi-core scaling is the deployment-relevant metric for a concurrent server, but
the addressable contention is a smaller effect than the single-core per-request cost, and it requires
measurement the team does not yet have — so it follows US1.

**Independent Test**: Run a concurrency sweep (1..32 clients across N cores) before and after the change
and compare per-core efficiency and the point at which aggregate throughput plateaus.

**Acceptance Scenarios**:

1. **Given** a concurrency sweep across N cores, **When** the scaling improvements are applied, **Then**
   per-core efficiency degrades less across the core range than the current baseline (currently ~11%
   degradation over 2→7 cores).
2. **Given** the sweep, **When** the improvements are applied, **Then** the near-linear region extends
   and/or the aggregate-throughput plateau moves higher.
3. **Given** any scaling change, **When** it is proposed, **Then** it ships only with a cross-core
   contention measurement confirming it reduces the specific contention it targets — never on projection.

*This story is measure-first*: it requires a multi-core cache-coherence and off-CPU measurement **before**
any change, because the existing profile is single-threaded and cannot reveal cross-core contention.

---

### Edge Cases

- **A read handled on the fast path encounters a failing/panicking custom node manager**: the request must
  still return the same fault as today (an internal-error status), and the connection and other sessions
  must survive — the fast path must not turn a per-request fault into a dropped connection.
- **A read and a write on the same session are in flight together** once reads no longer serialize through
  the per-session queue: the result must remain memory-safe and each operation individually correct;
  cross-service-call read-after-write ordering is not guaranteed by the protocol and is not required.
- **Request cancellation / deadline expiry** must continue to abort an in-flight request exactly as today,
  including on the fast path.
- **Diagnostics/metrics**: reductions that gate or move internal timing must not change any protocol-visible
  diagnostic value a client can read; internal-only counters may change.
- **A change that looks cheaper but measures slower** must be reverted rather than kept on reasoning.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST reduce the per-request work on the Read path such that single-client,
  single-core read throughput improves measurably over the current baseline.
- **FR-002**: The server MUST NOT change any bytes on the wire or any protocol-visible behavior as a result
  of these changes; the full conformance matrix MUST remain green and byte-identical.
- **FR-003**: Every performance change MUST be accepted or rejected by a before/after measurement on a
  pinned CPU; a change that regresses the target metric MUST be reverted.
- **FR-004**: A Read serviced by any faster internal path MUST return the same status codes for the same
  inputs as today, and a failing/panicking node manager MUST fault only that request (not close the
  connection or affect other sessions).
- **FR-005**: Request cancellation and deadline behavior MUST be preserved for every request regardless of
  which internal path handles it.
- **FR-006**: Multi-core scaling changes MUST preserve address-space read/structural-write consistency
  (a concurrent reader/browser MUST NOT observe a partially-applied structural change).
- **FR-007**: The change MUST NOT introduce new locks, mutexes, or blocking primitives beyond replacing
  existing ones with equal-or-cheaper mechanisms.
- **FR-008**: Security and cryptographic behavior MUST be unchanged.
- **FR-009**: Before any code change, current-HEAD performance baselines MUST be captured and recorded
  (single-core for US1; multi-core with cross-core contention and off-CPU measurement for US2), because the
  existing measurements predate recent optimizations and cannot be trusted as the baseline.

### Key Entities

- **Per-request baseline**: the recorded current-HEAD single-client throughput (read + write) on one pinned
  core, plus the profile, against which every US1 change is measured.
- **Scaling baseline**: the recorded current-HEAD concurrency-sweep results (per-core efficiency, plateau
  point) plus a cross-core contention and off-CPU profile, against which every US2 change is measured.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Single-client, single-core read throughput improves by **at least 1.5×** over the recorded
  HEAD baseline (target band ~110,000–130,000 ops/s from the ~68,968 baseline), finalized after the
  re-baseline.
- **SC-002**: Single-client, single-core write throughput improves measurably over the HEAD baseline.
- **SC-003**: Multi-core per-core efficiency degrades **less** across the core sweep than the current ~11%
  (2→7 cores), and/or the aggregate-throughput plateau moves higher.
- **SC-004**: Zero change in protocol behavior: the conformance matrix passes with byte-identical wire
  output across every security-policy × mode × identity-token combination.
- **SC-005**: The full existing test suite passes; read correctness across all attributes and all security
  policies is unchanged.
- **SC-006**: Every shipped change has a recorded before/after measurement justifying it; no change ships on
  projection alone.

## Assumptions

- Single-client throughput is a per-request **latency** proxy (one request in flight = 1/round-trip); it is
  used as the US1 metric deliberately, with the understanding that the deployment-relevant metric is
  aggregate throughput and per-core scaling (US2).
- A modest amount of per-request overhead is **inherent** to a feature-complete asynchronous, multi-core-
  scalable, secure server and will not be removed; the goal is to trim the *addressable* overhead, not to
  match a minimal single-threaded server.
- The benchmark harness with CPU pinning is available and is the accepted measurement instrument.
- Recent optimizations already landed (batched request draining, deferred response encoding, address-space
  hot/cold split); this feature builds on them and does not redo them.

## Out of Scope

- Matching the single-client throughput of a minimal microcontroller-targeted server (a small binary with
  no security, subscriptions, or multi-core scaling) — a different class of software and the wrong target.
- Removing the outer address-space lock as a standalone change; it is only considered as a **measured**,
  correctness-preserving multi-core (US2) candidate that keeps the structural-write/browse consistency
  barrier intact.
- Any wire-format, encoding, or protocol-behavior change.
- Any change to security or cryptographic behavior.
