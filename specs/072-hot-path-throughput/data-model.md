# Phase 1 Data Model: Hot-Path Per-Request Throughput

No wire types, no persisted data. The "entities" here are (1) the recorded measurement baselines that gate
every change, and (2) the small owned bundle the read fast-path carries. All internal (`pub(crate)` at most).

## Entity: HEAD performance baseline (Task 0 output → research.md)

Recorded once before any code change; every US1/US2 change is judged against it.

| Field | Meaning | Capture |
|-------|---------|---------|
| `single_core_read_ops_s` | single-client, single-core Read throughput | pinned-core bench, median of ≥3 |
| `single_core_write_ops_s` | same for Write | pinned-core bench |
| `read_self_profile` | `perf record -e cycles:u` self-time buckets | single-thread server |
| `sweep[cores][clients]` | aggregate ops/s across 1..32 clients on N cores | multi-thread server |
| `per_core_efficiency[cores]` | `sweep / cores` — the linear-scaling curve | derived |
| `plateau_point` | clients/cores where aggregate stops rising | derived |
| `c2c_hitm_profile` | `perf c2c` HITM by cache line (US2 only) | multi-thread server |
| `offcpu_profile` | off-CPU / wakeup-latency (US2 only) | multi-thread server |

**Validation rule**: a US1 change is *accepted* iff `single_core_*_ops_s` improves or is neutral; a US2
change iff `per_core_efficiency`/`plateau_point` improves **and** the targeted `c2c_hitm` line drops.

## Entity: session-activity timestamp (S1a state change)

| Before | After |
|--------|-------|
| `last_service_request: ArcSwap<Instant>` (`instance.rs:113`); store = `Arc::new(Instant::now())` per request | `last_service_request: AtomicU64` (monotonic nanos); store = one atomic write, no alloc |

**Invariant preserved**: `validate_timed_out` still rejects a session idle beyond its revised timeout; the
value is monotonic and read/written lock-free. Reads of "now" reduced from 2 → 1 per call.

## Entity: read fast-path context bundle (S2)

The free-function read extracted from `SessionActor::read` consumes owned/`Arc`-shared inputs so it needs no
actor:

| Field | Type | Source |
|-------|------|--------|
| `context` | `RequestContext` (`Arc<RequestContextInner>`) | `request_context_from_parts` (`message_handler.rs:169`) |
| `node_managers` | `NodeManagers` (`Arc<Vec<Arc<DynNodeManager>>>`, `Clone`) | `MessageHandler` (`message_handler.rs:60`) |
| `nodes` | `Vec<ReadValueId>` | request body |
| `max_age` / `timestamps_to_return` | `f64` / `TimestampsToReturn` | request |
| `diagnostics` | `DiagnosticBits` | request header |

**Output**: `Result<Vec<DataValue>, StatusCode>`, mapped to `ReadResponse` exactly as today.
**Invariants** (see contracts): panic-isolated (`catch_unwind`), cancellation via the returned `JoinHandle`,
session-activity touch already done upstream in the controller, no continuation points for Value reads.

## Entity: `ChunkInfo` accessor (S1b state change)

| Before | After |
|--------|-------|
| `cached_chunk_info: Mutex<Option<ChunkInfo>>` (`message_chunk.rs:151`); `chunk_info()` locks + clones full struct on every hit; `ChunkInfo::new` calls `secure_channel.decoding_options()` internally (RwLock+clone) | `OnceLock<Arc<ChunkInfo>>`; `chunk_info(&DecodingOptions)` computes once, returns cheap `Arc` clone; no internal channel lock |

**Invariant preserved**: `ChunkInfo` remains per-chunk and is only ever computed on a decrypted chunk;
byte-identical results (same headers parsed the same way).
