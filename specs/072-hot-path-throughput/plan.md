# Implementation Plan: Hot-Path Per-Request Throughput

**Branch**: `072-hot-path-throughput` | **Date**: 2026-07-10 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/072-hot-path-throughput/spec.md`

## Summary

Reduce async-opcua's per-request async-architecture overhead so single-core throughput comes close to
peer OPC UA servers (US1), and extend the multi-core linear-scaling region (US2) — measure-first, with
zero protocol/wire change and no correctness regression. A lock-removal hypothesis was investigated and
**refuted** (the outer `RwLock<AddressSpace>` is 0.03–0.06% self-time; the projected numbers were absent
from the source data; naive removal breaks `node_map`↔`cold` atomicity `browse()` depends on). The
verified cause is per-request work: two heap-allocated futures, an `mpsc`+`oneshot` round-trip through the
per-session `SessionActor`, a per-request deadline timer, redundant chunk-header parses, and per-request
clock reads plus a heap `Arc` allocation in the session-activity touch.

**Approach (staged big-first, every change behind a before/after measurement gate):**
- **Task 0 (blocking):** re-baseline on HEAD — the existing profiles are June-30 vintage, pre-062/063.
- **US1 Stage 1 (safe cuts, always land):** S1a session-activity `ArcSwap<Instant>`→`AtomicU64` (kill the
  per-request `Arc` alloc + double clock read); S1b `ChunkInfo` — thread `&DecodingOptions` in, collapse the
  3 redundant header parses in `apply_security`, `OnceLock`+`Arc<ChunkInfo>` instead of `Mutex`+clone; S1c
  gate always-on actor timing behind `diagnostics`; S1d reduce controller `sleep_until` re-arming.
- **US1 Stage 2 (gated big lever):** a read fast-path letting pure Value-attribute reads bypass the
  per-session actor `mpsc`+`oneshot`, kept only if measured; `catch_unwind` panic isolation + cancellation
  preserved.
- **US2 (measure-first):** capture multi-thread `perf c2c`/off-CPU first; then pool fan-out `Vec`s and
  reduce the outer-lock double-acquisition correctness-preservingly (value reads via lock-free `node_map` +
  `TypeTreeSnapshot`, keeping the `browse`/structural-write barrier), shipped only with a HITM number.

## Technical Context

**Language/Version**: Rust (workspace edition 2021), stable `rustc` 1.96.0
**Primary Dependencies**: existing only — `tokio`, `parking_lot`, `arc-swap`, `dashmap`, `bytes`. No new
deps. (`dashmap` already used; available if US2 converts callback maps.)
**Storage**: N/A
**Testing**: `cargo test` (workspace + `async-opcua-server`); conformance smoke
(`async-opcua/tests/integration/conformance.rs`) as the byte-identical equivalence guard; new fast-path
panic-isolation + all-attributes/all-policies read-correctness tests; `tools/ci-playbook.sh --ci`.
**Measurement instrument**: `tools/opcua-localhost-bench` (and the `~/scratch/opcua-localhost-bench` C
harness) with `taskset -c` pinning per `~/scratch/OPCUA-BENCHMARK.md`; `perf record -e cycles:u` (US1
self-time) and `perf c2c` + off-CPU/wakeup profile (US2 contention).
**Target Platform**: any; reference numbers on x86-64 Linux with isolated pinned cores.
**Project Type**: Rust library workspace (transport → secure-channel → session → node-manager layers).
**Performance Goals**: US1 single-client single-core read throughput ≥1.5× HEAD baseline (~68,968 →
~110–130K), write measurably up; US2 per-core efficiency degrades <11% over the sweep and/or the plateau
moves higher. Latency-metric caveat acknowledged (single-client = 1/round-trip).
**Constraints**: zero wire change (conformance byte-identical); no new locks/mutexes/blocking primitives
beyond replacing existing with equal-or-cheaper (AGENTS.md); network paths never panic, fail closed
(constitution IV); crypto/security unchanged; every change accepted/rejected by measurement, never
projection (constitution I + spec FR-003).
**Scale/Scope**: ~6 server files + ~4 core/comms files; staged; no new crate or public API.

## Constitution Check

*GATE: evaluated against constitution v1.0.0 — PASS (re-check after Phase 1).*

- **I. Correctness Over Completion (NON-NEGOTIABLE)**: this feature is *defined* by measure-first — no
  change ships on projection (FR-003/SC-006); the refuted lock hypothesis is the cautionary example.
  Correctness gated three ways: conformance byte-identical (SC-004), full suite green (SC-005), and a
  fast-path panic-isolation + all-attributes/all-policies read test (FR-004). No story ships with a known
  gap; a change that measures slower is reverted.
- **II. Do It Right Once**: reductions are real (owned-input threading, `OnceLock`, `AtomicU64`), not
  `#[allow]`-suppressed; the read fast-path *reuses* `invoke_service_concurrently_mut` +
  `request_context_from_parts` rather than duplicating; US2 reuses `DataChangeNotificationVecPool` and
  `TypeTreeSnapshot`. No `// TODO` on a reachable path.
- **III. Individual Task Discipline**: `tasks.md` = one task per S1a/S1b/S1c/S1d, one for Task-0, one for
  the Stage-2 fast-path (+ its gate), and per-item US2 tasks; each independently measured/verified.
- **IV. Security Is Paramount**: no security/crypto behavior change (FR-008); the fast path preserves
  fail-closed panic isolation (`catch_unwind`) so a node-manager panic faults one request, not the process
  or connection; no new attack surface (decode unchanged, only handling paths trimmed).
- **V. Leave It Better**: removes real per-request waste (redundant parses, per-request allocs); the
  measurement harness + recorded baselines become a durable regression guard; builds on 062/063 rather than
  redoing them.

**No violations. Complexity Tracking omitted.**

## Project Structure

### Documentation (this feature)

```text
specs/072-hot-path-throughput/
├── plan.md              # This file
├── research.md          # Phase 0 — refuted-hypothesis record, per-request cost map, cut-site decisions
├── data-model.md        # Phase 1 — the measurement baselines + the read-fast-path context bundle
├── quickstart.md        # Phase 1 — baseline capture + per-change measurement commands
├── contracts/
│   └── measurement-and-fastpath-contracts.md   # Phase 1 — measurement gates + fast-path invariants
├── checklists/requirements.md   # from /speckit-specify
└── tasks.md             # Phase 2 — /speckit-tasks (NOT created here)
```

### Source Code (repository root)

```text
async-opcua-server/src/
├── session/
│   ├── instance.rs          # S1a: last_service_request ArcSwap<Instant> → AtomicU64 (validate_timed_out :224-227)
│   ├── actor.rs             # S1c: gate timing (:133,:197); S2: extract read (:267-329) into free fn
│   ├── message_handler.rs   # S2: value-read fast path (:730) via request_context_from_parts (:169)
│   ├── controller.rs        # S1d: sleep_until re-arm (:289-297), reuse per-turn Instant (:309/:865)
│   └── services/mod.rs      # S2: reuse invoke_service_concurrently_mut (:62); US2: pool fan-out Vecs (:77-79)
└── node_manager/memory/
    ├── mod.rs               # US2: AddressSpace read-lock (:864); value-read snapshot path
    └── simple.rs            # US2: read-lock (:247), read_cbs/write_cbs RwLock<HashMap> → DashMap (:170)

async-opcua-core/src/comms/
├── message_chunk_info.rs    # S1b: ChunkInfo::new takes &DecodingOptions (:42-79, kill :45 RwLock+clone)
├── message_chunk.rs         # S1b: chunk_info() OnceLock + Arc<ChunkInfo> (:151,:434-445)
├── secure_channel.rs        # S1b: collapse 3 header parses in apply_security (:883,:896,:905)
└── buffer.rs                # (context: SendBuffer reuse, unchanged)
```

**Structure Decision**: In-place optimization across the existing server/core layers; no new project,
crate, module tree, or public umbrella API. Reuses landed infrastructure (062 batch-drain/deferred-encode,
063 hot/cold AddressSpace, `TypeTreeSnapshot`, `DataChangeNotificationVecPool`, `request_context_from_parts`).
