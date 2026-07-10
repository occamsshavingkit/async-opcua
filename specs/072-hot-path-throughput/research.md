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

## Task 0 — recorded HEAD baseline (from `~/scratch/ASYNC-OPCUA-IMPROVEMENT-BENCHMARK.md`)

Source: the improvement-benchmark run **on commit `0f7bd5e6`** (= current `master`, the base of branch
`072`; my 072 commits touch only `specs/` + agent markers, no Rust source → the bench clone
`~/scratch/async-opcua` @ `0f7bd5e6` is byte-identical to the 072 code baseline). Machine: 12-core x86_64,
kernel 6.18.35; release (LTO, codegen-units=1); server `taskset`-pinned, clients on the rest; bench client =
open62541 C client. So the throughput baseline below **is** the HEAD baseline (T002/T004 satisfied); only
the *profiles* below are still to (re)capture.

**Single-client, sequential (server on cores 5,11; median of 3, 5 s):**
| Server | Read ops/s | Write ops/s |
|--------|-----------:|------------:|
| async-opcua | **68,968** | **66,547** |
| open62541 | 168,915 | 169,589 |
| micro-opcua | 204,128 | 208,780 |
→ async-opcua ~2.4× slower than open62541, ~3× slower than micro-opcua single-client. **US1 target
(SC-001): ≥1.5× → ~103,000+ read ops/s.**

**Concurrency sweep, 7 server cores (5–11), clients on 0–4:**
| Clients | 1 | 2 | 4 | 8 | 16 | 24 | 32 |
|---------|--:|--:|--:|--:|---:|---:|---:|
| Aggregate ops/s | 77,026 | 148,155 | 259,301 | 360,976 | 491,435 | 508,095 | 507,202 |
Per-core efficiency 82.0K (2c) → 72.6K (7c) = **11% degradation**; plateau **~508K** at 24 clients.
(2-core sweep plateaus ~164K.) **US2 target (SC-003): reduce the 11% and/or lift the plateau.**

**CPU utilization (32 clients, 7 cores, `pidstat`):** %usr ~46, %system ~24, **%idle ~30**. Idle-under-load =
off-CPU wait (the actor `mpsc`/`oneshot` pipeline), not lock contention (confirmed: R0/R1).

**Still to capture (the genuine Task-0 remainder):**
- **US1 profile**: a *HEAD* single-thread `perf record -e cycles:u` read profile (the existing profiles are
  June-30 / pre-062-063). Runnable here (`perf_event_paranoid=2` allows `cycles:u`; bench server builds).
  Diagnostic, not the US1 gate — the US1 gate is single-client throughput via the harness (`taskset`).
- **US2 profile**: the **never-captured** multi-thread `perf c2c`/HITM + off-CPU/wakeup profile. **Blocked
  here**: `perf c2c` needs `perf_event_paranoid ≤ 0` (privileged) and the fuller core isolation from
  `OPCUA-BENCHMARK.md` (only CPU 11 is currently isolated). US2 cannot gate until this exists.

**Harness note**: `~/scratch/opcua-localhost-bench/async-opcua-bench-server/Cargo.toml` path-depends on
`~/scratch/async-opcua` (a distinct clone), not the working repo. Repointed to
`/home/quackdcs/async-opcua/async-opcua` so each rebuild compiles the live 072 changes.

### R11. MEASURED clean-core baseline + the HT-pinning artifact (2026-07-11)

Re-measured on this machine (6 physical cores / 12 logical; CPUs 5 & 11 are SMT siblings on physical
CORE 5). Bench server built against the working repo; single client; `bench_client read`, warmup 1s,
measure 5s, median of 3. **A third of the "3× gap" was a measurement artifact.**

| Server | **Clean core** (CPU 11 isolated, sibling 5 offline) | `5,11` (both HT siblings) = recorded config |
|--------|---------------------------------:|---------------------:|
| async-opcua | **102,679 read / 96,792 write** | 70,304 / 70,503 (≈ recorded 68,968 — harness validated) |
| open62541 | 153,846 | 168,915 (recorded) |
| micro-opcua | 200,070 | 204,128 (recorded) |

**Finding**: async-opcua gains **~45%** (70K→102.7K) purely from giving it an exclusive physical core.
Its `#[tokio::main]` multi-thread runtime spreads work (reactor, actor, timers, wakers) across BOTH
logical CPUs of CORE 5, so pinning to `5,11` makes the runtime **self-contend on one physical core's
execution units**. The single-threaded peers use one thread, so `5,11` doesn't penalize them — they barely
move. (Deployment corollary: do not pin a multi-threaded tokio server to both HT siblings of a core.)

**Corrected baseline & gap (fair, clean single physical core)**:
- async-opcua **102,679 read / 96,792 write**; open62541 ~153.8K; micro ~200K.
- Real single-clean-core gap: **1.50× vs open62541, 1.95× vs micro** (was 2.45× / 2.96× under HT pinning).
- The old SC-001 target (≥1.5× over 68,968 → ~103K) is **already met by the measurement correction alone**
  — it must be reset against the clean baseline. Proposed: close a substantial fraction of the *remaining*
  clean-core gap, e.g. **102.7K → ~130–150K read (≥1.27–1.45×)**, i.e. materially toward open62541's ~154K.

**All US1 code-change measurements use the clean isolated core** (CPU 11, sibling 5 offline;
`perf_event_paranoid=0` set for the later US2 `c2c`). Reference to beat: **102,679 read / 96,792 write.**
The remaining gap is the genuine async per-request tax (R1) — so US1's cuts are still worth doing, just
smaller in magnitude than the HT-inflated numbers suggested.

### R12. US1 measured results (clean core CPU 11, median of ≥5; baseline 102,679 read / 96,792 write)

| Change | Read ops/s | Δ read | Write ops/s | Δ write | Decision |
|--------|-----------:|:------:|------------:|:-------:|----------|
| baseline (0f7bd5e6) | 102,679 | — | 96,792 | — | — |
| **S1a** ArcSwap<Instant>→AtomicU64 | 102,420 | ~0% (noise) | 99,240 | **+2.5%** | **keep** (write up, read neutral, removes per-request `Arc` alloc; session tests green) |
| **S1a + S2** read fast-path (bypass actor mpsc+oneshot) | 104,686 | **+2.0%** | 99,260 | **+2.6%** | **keep** (real; 38 read + hardening + actor tests green, panic-isolation covered via new path) |

**Key finding**: a single per-request micro-cut moves single-client throughput only **~1–2%**, right at the
run-to-run noise floor (~3.5% spread). Implication: (a) individual S1x gates are near-unresolvable
single-client — better to judge the *stacked* Stage-1 delta; (b) the per-request allocation/clock cuts help
*multi-core* (allocator/coherence) more than single-client; (c) **the real single-client lever is S2** (the
actor-bypass — removing the per-request `tokio::spawn` + `mpsc` + `oneshot` = 2 scheduler hops + a heap
future + a channel per request), not the micro-cuts. Strategy adjusted accordingly: stack S1a/S1b/S1d, then
S2 as the headline change.

### R13. US1 conclusion — the per-request cuts are marginal single-client; the gap is structural

**Measured, not projected.** S1a + S2 together = **read +2.0% / write +2.6%** single-client. The surprise:
**S2 (the actor-bypass — the supposed big lever) added only ~+2% read over S1a**, even though it removes an
`mpsc` send + a `oneshot` round-trip + an actor wakeup per request. Why: the read still pays a per-request
`tokio::spawn` (kept for cancellation), and single-client throughput is dominated by costs the US1 cuts do
**not** touch — the spawn itself (heap future + scheduler dispatch), the recv/send **syscalls**, and
serialization/`memcpy`. The actor hop was only ~2% of the per-request budget.

**Implication**: the ~1.5× clean-core gap to open62541 is **not closable by these per-request refactors** —
it is structural (async-runtime spawn + syscall boundary + serialization). Closing it needs bigger levers
outside US1's scope: removing the per-request spawn (resolve reads synchronously, at the cost of mid-flight
cancellation), syscall batching (`recvmmsg`/io_uring), or a thread-per-core pipeline. The US1 cuts (S1a, S2)
are still worth keeping — real (+2–3%), remove per-request allocations/hops, and pay off more under
**multi-core** (less allocator/scheduler contention as it scales — to confirm in the US2 `c2c` sweep).
**Remaining S1b (ChunkInfo) / S1d (controller timer) are ~1% single-client each and will not change this
conclusion**; S1c is a diagnostics-off-only hygiene win.

### R14. US1 multi-client sweep — the cuts ARE meaningful (the R13 prediction confirmed)

Concurrency sweep, server on cores 5–11, clients on 0–4 (`run_concurrency_sweep.py`):

| Clients | Recorded baseline (0f7bd5e6) | **S1a+S2** | Δ |
|--------:|-----------------------------:|-----------:|:---:|
| 1 | 77,026 | 85,717 | +11.3% |
| 2 | 148,155 | 164,998 | +11.4% |
| 4 | 259,301 | 282,979 | +9.1% |
| 8 | 360,976 | 383,524 | +6.4% |
| 16 | 491,435 | 548,112 | **+11.6%** |
| 24 | 508,095 | 565,431 | +11.3% |
| 32 | 507,202 | **568,764** | **+12.1%** |

**Conclusion corrected**: the US1 per-request cuts (S1a, S2) are ~2% single-client but **+12% aggregate
under load** — plateau 508K→569K. The allocation/hop reductions cut per-request allocator + scheduler
contention, which scales. So US1 delivers real value on the *deployment-relevant* metric (aggregate /
per-core scaling), even though single-client (the R13 metric) barely moved. This is the "before" the LIFO
worker-pool experiment must beat: **plateau ~569K, 16-client 548K**.
