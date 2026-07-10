# Phase 0 Research: Hot-Path Per-Request Throughput

All findings are grounded in real `perf` data (`~/scratch/opcua-localhost-bench`), the benchmark doc
(`~/scratch/OPCUA-BENCHMARK.md`), and code at HEAD. No open `NEEDS CLARIFICATION`.

## R0. The refuted hypothesis (recorded so it is not re-litigated)

**Decision**: Do NOT remove the outer `RwLock<AddressSpace>` as the throughput fix. **Rationale** (verified
by two adversarial workflows, high confidence): (a) the lock is **0.03–0.06% self-time** — invisible in two
`perf` captures; `trace_locks` is instrumentation, not contention; (b) the "+10% / 508K→560K / 23% idle"
figures are **absent from the benchmark doc** — projections; the profiled server is single-thread
`current_thread`, which cannot exhibit cross-core contention, and idle is off-CPU wait which a non-blocking
read lock cannot cause; (c) naive removal is **unsafe** — the outer lock is the only coupling between
`node_map` and `cold` that `browse()` reads atomically → torn reads. Same shape as feature #244 (measured
~1.0× neutral). The lock's read-side shared-word RMW is a *legitimate but small* multi-core (US2) candidate,
addressed correctness-preservingly and only on a measured HITM number.

## R1. Verified cause — per-request cost map (single-thread `cycles:u` profile)

**Decision**: attack per-request work, not the lock. Profile self-time buckets: ~23% tokio async plumbing
(waker clone/drop, mpsc fairness `thread_rng`, task state transitions, timer-wheel insert/remove,
`OwnedTasks::bind_inner`), ~9.5% chunk (`ChunkInfo::new` 3.35% = top symbol), memcpy 4.12%, `clock_gettime`
3.45%. **Micro-opcua diff** (the 3× peer): micro does the same Read with **0 heap allocs, 0 async runtime** —
a flat synchronous alloc-free loop where the syscall boundary is the floor. async-opcua's per-Read tax
(confirmed at HEAD by recon): two heap futures (`tokio::spawn` `message_handler.rs:732` + boxed pending
`controller.rs:977`), an `mpsc`+`oneshot` round-trip through the per-session actor (`actor.rs:126-151`), a
per-request deadline timer + abort oneshot, fan-out `Vec`s, redundant chunk-header parses, and a per-request
`Arc::new(Instant::now())` in `validate_timed_out`.

## R2. S1a — session-activity touch (instance.rs:224-227)

**Decision**: replace `last_service_request: ArcSwap<Instant>` (`instance.rs:113`) with an `AtomicU64` of
monotonic nanos; read the clock once. **Rationale**: `validate_timed_out` reads the clock **twice** and does
`store(Arc::new(Instant::now()))` — a heap `Arc` alloc + 2 `clock_gettime` **per request**, on the hot
validation path (`controller.rs:1124`). An `AtomicU64` store is lock-free, alloc-free, one clock read.
**Alternatives**: keep `ArcSwap` but read clock once (smaller win, still allocates); coarse clock
(`CLOCK_MONOTONIC_COARSE`) — deferred, may reduce precision of timeout bookkeeping.

## R3. S1b — ChunkInfo redundant work (message_chunk_info.rs, secure_channel.rs)

**Decision**: (1) thread `&DecodingOptions` into `ChunkInfo::new`/`chunk_info()` so it stops calling
`secure_channel.decoding_options()` internally (`message_chunk_info.rs:45` = a `parking_lot` RwLock read +
`DecodingOptions` clone incl. an `AtomicU64` construction, per call); (2) collapse the **3** header parses in
`apply_security` (`secure_channel.rs:883` `encrypted_data_offset`, `:896` via `chunk_info`, `:905`
`is_open_secure_channel`) — 2 of 3 bypass the cache — into one `chunk_info()`; (3) make `chunk_info()` a
`OnceLock` returning `Arc<ChunkInfo>` instead of `Mutex<Option<ChunkInfo>>` + full clone on every hit
(`message_chunk.rs:151,440`). **Rationale**: `ChunkInfo::new` is the top self-time symbol; the real cost is
the repeated `decoding_options()` lock+clone and redundant re-parsing, not the ~20-byte header decode.
**Alternatives**: precompute `ChunkInfo` arithmetically at encode time in `ChunkingStream` — larger change,
deferred to a follow-up if S1b's simpler cuts don't suffice.

## R4. S1c — ungated actor timing (actor.rs:133,197)

**Decision**: gate the always-on `Instant::now()` + `elapsed()` feeding `actor_message_duration_ns`
(`actor.rs:133`, `record_message_processed` `:197`) behind `#[cfg(feature = "diagnostics")]`, matching
`response_metrics` (`controller.rs:367`). **Rationale**: a second per-request clock read that only exists to
populate an internal metric; free to remove when diagnostics are off. No protocol-visible value changes.

## R5. S1d — controller timer churn (controller.rs:289-297)

**Decision**: rebuild the two `sleep_until` timers only when the earliest deadline actually changes; reuse a
single `Instant::now()` per loop turn across `controller.rs:309/865`. **Rationale**: the event loop re-arms
up to two timers every iteration → timer-wheel insert/remove + clock reads per request. The `DeadlineQueue`
(`controller.rs:150-195`) already centralizes deadlines; only the outer arm needs the change.

## R6. S2 — read fast-path bypassing the SessionActor (feasible; gated)

**Decision**: extract `SessionActor::read` (`actor.rs:267-329`) into a free function `(RequestContext,
NodeManagers, nodes, max_age, tsr, diagnostics)` reusing `invoke_service_concurrently_mut`
(`services/mod.rs:62`); in `MessageHandler::read` (`message_handler.rs:730`) route pure Value-attribute reads
straight to it via `request_context_from_parts` (`message_handler.rs:169`), skipping the `mpsc` send
(`:810`) + `oneshot` (`:809/824`) + actor wakeup. **Rationale/feasibility** (recon): a Value read touches no
actor-exclusive state — the session-activity touch happens in the controller *before* the actor
(`instance.rs:227` via `controller.rs:1124`), Value reads have no continuation points, cancellation wraps the
`JoinHandle` not the actor (`controller.rs:973`). **Hard requirements**: replicate `catch_unwind`
(`actor.rs:323`) so a node-manager panic yields `BadInternalError` (not a closed connection); keep returning
`AsyncMessage(JoinHandle)`. **Explicit accepted trade-off**: a bypassed read may run concurrently with a
queued write on the same session — memory-safe via the `AddressSpace` `RwLock`; OPC UA does not mandate
cross-service-call read-after-write ordering. **Gate**: keep only if the HEAD before/after shows a real
single-client + aggregate win. **Alternatives**: keep the actor but drop `tokio::spawn` for reads (smaller);
a per-connection reader task pool (more complex) — deferred.

## R7. US2 — multi-core scaling (measure-first)

**Decision**: capture a **multi-thread** `perf c2c`/HITM + off-CPU/wakeup profile FIRST (the missing
measurement; the current profile is single-thread and cannot show contention). Then, only for confirmed
costs: pool the fan-out `Vec`s (`services/mod.rs:77-79`, `actor.rs:275/328`, `memory/mod.rs:862/881`,
`simple.rs:246`) via the existing per-session `DataChangeNotificationVecPool` pattern
(`subscriptions/subscription.rs:25`); reduce the outer-lock **double** acquisition per Value read
(`memory/mod.rs:864` + `simple.rs:247`) by routing pure Value reads through the lock-free `node_map` +
`TypeTreeSnapshot` (`info.rs:117/213`) while keeping the outer barrier for `browse`/structural writes;
convert `read_cbs`/`write_cbs` `RwLock<HashMap>` (`simple.rs:170`) to `DashMap`. **Rationale**: these are the
real cross-core shared lines, but their magnitude is unproven — ship only with a HITM before/after.

## R8. Measurement discipline (the meta-decision)

**Decision**: Task 0 re-baselines HEAD before any change (the existing profiles are June-30, pre-062/063);
every S1x/S2/US2 change is a separate before/after measurement on a pinned core; a change that regresses or
is neutral-for-a-perf-goal is reverted (kept only if safety cleanup). Conformance smoke stays byte-identical
as the standing equivalence guard. **Rationale**: constitution I + the refuted hypothesis prove projection is
not acceptable evidence here.

## R9. No new dependencies

`AtomicU64`, `OnceLock`, `Arc`, `DashMap` (already a dep), `catch_unwind` are all in-tree. No `cargo deny`
impact.
