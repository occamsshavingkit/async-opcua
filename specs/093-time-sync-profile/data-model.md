# Phase 1 Data Model: Time Sync Profile Decision

The "data" here is runtime state and configuration, not persisted storage. Types
below are the design intent for `/speckit-tasks`; exact signatures are pinned in
`contracts/`.

## Entities

### TimeSyncMechanism (enum)

Identifies the mechanism a `TimeSyncSource` represents.

| Variant | Meaning | Related CU |
|---|---|---|
| `OsClock` | System/OS clock (operator keeps it synced) | 2478 |
| `Ntp` | Network Time Protocol (user-supplied) | 2786 |
| `Ptp` | IEEE 1588 PTP (user-supplied) | 2479 |
| `Gptp` | IEEE 802.1AS gPTP (user-supplied) | 2480 |
| `UaHeaderBased` | Periodic UA response-header timestamp sync | 5505 |
| `Custom(&'static str)` or `Custom(String)` | Any other operator mechanism | — |

- Derives: `Debug`, `Clone`, `PartialEq`, `Eq` (and `Copy` if the `Custom` payload
  is `&'static str`).

### TimeSyncStatus (struct)

Snapshot of a source's state, returned by `TimeSyncSource::status()`.

| Field | Type | Rules |
|---|---|---|
| `mechanism` | `TimeSyncMechanism` | required |
| `synchronized` | `bool` | `false` until a successful sync/observation; `false` on poll failure (FR-009) |
| `last_sync` | `Option<DateTime>` | `opcua_types::DateTime`; `None` if never synced (edge case) |
| `observed_skew` | `Option<Duration>` | `std::time::Duration` magnitude of |local − source| offset; `None` if not measured (e.g. pure `OsClock`, or never polled) |

- Derives: `Debug`, `Clone`.
- Invariant: if `synchronized == false` due to a never-completed poll, `last_sync`
  and `observed_skew` are `None` (no stale zero — spec edge case).

### TimeSyncSource (trait)

The extensibility seam. `Send + Sync` (held as `Arc<dyn TimeSyncSource>` on
`ServerInfo`, read from async tasks).

- `fn status(&self) -> TimeSyncStatus;` — cheap, non-blocking snapshot read.
- Object-safe (no generics, no `async fn` in the trait — the poll loop lives in the
  concrete `UaHeaderTimeSyncSource`, not the trait).

### OsClockSource (struct, built-in, always compiled)

Default `TimeSyncSource`. `status()` returns `{ mechanism: OsClock, synchronized:
true, last_sync: Some(DateTime::now()), observed_skew: None }`.

- Zero configuration; no internal mutable state.

### UaHeaderTimeSyncSource (struct, `cfg(feature = "time-sync-ua")`)

Built-in `TimeSyncSource` backed by a background poll loop.

| Field | Type | Notes |
|---|---|---|
| `endpoint_url` | `String`/`UAString` | well-known source to poll (CU 5505) |
| `poll_interval` | `Duration` | configurable (FR-008) |
| `state` | `Arc<RwLock<TimeSyncStatus>>` (or atomics) | written by loop, read by `status()` |

- `mechanism` is always `UaHeaderBased`.
- `status()` reads the shared `state`.
- The poll loop (separate, spawned at startup) each tick: call the client
  discovery method → on success compute `observed_skew = |server_ts − local_now|`,
  set `synchronized = true`, `last_sync = server_ts`; on failure set `synchronized
  = false` (FR-009), leaving prior `observed_skew`/`last_sync` as `None`-or-prior
  per the no-stale-freshness rule.

## Configuration additions

### ServerConfig.max_acceptable_clock_skew_ns

| Aspect | Value |
|---|---|
| Type | `u64` (**nanoseconds**), serde field with `#[serde(default)]` |
| Default | `DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_NS` (constant in `constants` module, lib.rs); 5_000_000_000 (5s) |
| Validation | `0` → fall back to default (FR-005) |
| Accessor | returns `Duration` (nanosecond-precise) |
| CU | 3802 |

**Correction (post-initial-implementation)**: originally shipped as `_ms`
(milliseconds). Milliseconds cannot express a meaningful tolerance for
PTP/gPTP-class `TimeSyncSource`s (typically sub-microsecond precision) — the
finest expressible non-zero value, 1ms, is orders of magnitude too coarse.
Changed to nanoseconds before this branch was pushed/merged.

### ServerInfo.time_sync_source

| Aspect | Value |
|---|---|
| Type | `Arc<dyn TimeSyncSource>` (non-optional; defaulted to `OsClockSource` at build) |
| Set via | `ServerBuilder::with_time_sync_source(...)` |

## Report-tool classification (tools/cu-coverage-report)

### EvidenceStatus gains `Extensible`

| Bucket (after this feature) | CUs |
|---|---|
| `implemented_cus()` (adds) | 2478, 3802, 5505, 5793 |
| `extensible_cus()` (new) → `Extensible` | 2479, 2480, 2786 |
| `time_sync_gaps()` | emptied / removed |

`Extensible` label: `extensible`; note: "Satisfiable via user-supplied
`TimeSyncSource`; documented extension point, not implemented in-library."

## Relationships / state flow

```
ServerBuilder.with_time_sync_source? ──build()──▶ ServerInfo.time_sync_source: Arc<dyn TimeSyncSource>
        (default OsClockSource if None)                         │
                                                                ├─ OsClockSource.status() ─▶ TimeSyncStatus{OsClock,true,now,None}
                                                                │
   server startup (cfg time-sync-ua) ── spawn ──▶ UaHeaderTimeSyncSource poll loop
        │  each interval tick                                   │
        │    client GetEndpoints → ResponseHeader.timestamp     ▼
        └────────────────────────────────────────▶ RwLock<TimeSyncStatus> ◀── status() reads
                                                                │
   ServerConfig.max_acceptable_clock_skew (Duration) ──────────┘  (compared against observed_skew by caller)
```
