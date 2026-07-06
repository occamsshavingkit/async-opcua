# Implementation Plan: Hot-Path and Lock Optimization

**Branch**: `063-hot-path-and-locks` | **Date**: 2026-07-05 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/063-hot-path-and-locks/spec.md`

## Summary

Four independent hot-path optimizations grounded in performance profiling from feature 062. Each targets a specific source of CPU overhead in the server's request-processing pipeline:

1. **Split AddressSpace hot/cold** (US1): Eliminate redundant `RwLock::read()` on every Read by exposing the already-lock-free `DashMap` directly, moving cold fields behind a separate `RwLock`.
2. **Cache session Arc in dispatch context** (US2): Store the `find_by_token` result in the per-request context to avoid repeated hash-table lookups.
3. **Shared deadline queue** (US3): Replace N individual `tokio::time::sleep_until` futures with a single checked-once-per-tick deadline queue.
4. **ArcSwap debt investigation** (US4): Profile, identify, and replace/reduce `arc_swap::Debt::pay_all` overhead.

Each task is independently verifiable and can be implemented in parallel if desired (though they share no state).

## Technical Context

**Language/Version**: Rust (edition 2021, workspace resolver = "2")
**Primary Dependencies**: parking_lot (RwLock), dashmap (lock-free concurrent map), arc_swap (RCU-like pointer), tokio (async runtime, time::sleep_until), bimap (BiMap for token→session mapping)
**Storage**: N/A (in-memory optimization, no persistence changes)
**Testing**: cargo test --locked --all-features, cargo clippy --workspace --all-targets --all-features, tools/opcua-localhost-bench (throughput benchmark)
**Target Platform**: Linux server (localhost benchmark via tokio/TCP)
**Project Type**: library (workspace of 15+ crates) consumed as a network server
**Performance Goals**: ≥3% aggregate throughput improvement on localhost-bench; elimination of locking contention on read path
**Constraints**: All existing tests must pass; no protocol-level behavior change; lock safety must be preserved
**Scale/Scope**: Four changes across 3-4 crates: async-opcua-server (address_space, session, controller), async-opcua-core (ArcSwap usage)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Status |
|-----------|-----------|--------|
| I. Correctness Over Completion | Each optimization preserves lock safety and protocol semantics. The AddressSpace split must maintain read-after-write visibility for cold field mutations. The session cache must handle session termination. All tests must pass after each individual change. | PASS |
| II. Do It Right Once | Each change is architecturally minimal — one concept, one file/area. The AddressSpace split is the most invasive but follows an established pattern (hot/cold separation). No new abstractions introduced. | PASS |
| III. Individual Task Discipline | The four user stories are independent — they touch different code paths and can be implemented and verified separately. Tasks will be one per user story. | PASS |
| IV. Security Is Paramount | No changes to crypto, auth, decode, or transport. The AddressSpace split preserves access semantics; the session cache is an internal optimization; the deadline queue is timer-only; ArcSwap investigation may touch ServerInfo but not crypto. | PASS |
| V. Leave It Better Than You Found It | Removes redundant locking, eliminates wasted timer allocations, caches redundant lookups. Each change makes the touched code cleaner and more efficient. | PASS |

**Gate Result**: All principles pass. No violations to justify.

## Project Structure

### Documentation (this feature)

```text
specs/063-hot-path-and-locks/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── spec.md              # Feature specification
└── tasks.md             # Phase 2 output (speckit.tasks)
```

### Source Code (repository root)

```text
async-opcua-core/src/
├── comms/buffer.rs         # ArcSwap usage (US4 investigation)

async-opcua-server/src/
├── address_space/mod.rs    # AddressSpace hot/cold split (US1)
├── node_manager/memory/mod.rs  # Read-site changes for US1
├── session/controller.rs   # Deadline queue (US3), session cache (US2)
├── session/manager.rs      # find_by_token changes (US2)
└── transport/tcp.rs        # Potential polling changes (US3)

tools/opcua-localhost-bench/  # Before/after benchmark verification
```

**Structure Decision**: Single workspace project. Changes are confined to the server crate's session/address_space modules and the core crate's ArcSwap usage. No new crates or modules needed.

## Complexity Tracking

> No constitutional violations to justify.
