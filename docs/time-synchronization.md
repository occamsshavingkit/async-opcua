# Time Synchronization (OPC UA Time Sync Conformance Units)

This document states which OPC UA Foundation "Time Sync" conformance units
(CUs) this server claims, by which mechanism, and how to extend it. It
resolves `TODO.md`'s "Time Sync profile decision" (feature 093).

## Summary

| CU | Name | Status | Mechanism |
|---:|---|---|---|
| 2478 | Time Sync – OS based support | **Claimed** | Built-in `OsClockSource` (default) |
| 3802 | Time Sync – Configure Clock Skew | **Claimed** | `ServerConfig::max_acceptable_clock_skew_ns` |
| 5505 | Time Sync – UA based support | **Claimed** (opt-in) | Built-in `UaHeaderTimeSyncSource`, `time-sync-ua` feature |
| 5793 | Time Sync – Support (≥1 mechanism) | **Claimed** | Satisfied by 2478 and/or 5505 above |
| 2479 | Time Sync – IEEE 1588 (PTP) | **Extensible** | User-supplied `TimeSyncSource` |
| 2480 | Time Sync – IEEE 802.1AS (gPTP) | **Extensible** | User-supplied `TimeSyncSource` |
| 2786 | Time Sync – NTP | **Extensible** | User-supplied `TimeSyncSource` |

This applies uniformly to all four canonical 2025 server profiles this
repository builds (Nano Embedded Device, Micro Embedded Device, Embedded, and
Standard) — the mechanism set does not vary by profile; only whether the
`time-sync-ua` Cargo feature is enabled changes what's compiled in.

## The `TimeSyncSource` extensibility point

The server always derives its timestamps from the host operating system's
clock. A `TimeSyncSource` reports *how* (and whether) that clock is being
kept synchronized, for conformance and diagnostics:

```rust
pub trait TimeSyncSource: Send + Sync {
    fn status(&self) -> TimeSyncStatus;
    fn run<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> { /* default: no-op */ }
}
```

`TimeSyncStatus` reports a `mechanism` (`OsClock` / `Ntp` / `Ptp` / `Gptp` /
`UaHeaderBased` / `Custom`), whether the source is currently `synchronized`,
the `last_sync` time, and any `observed_skew`.

Register a source via `ServerBuilder::with_time_sync_source(...)`. If none is
registered, the server defaults to `OsClockSource`.

### CU 2478 — OS-based support (claimed, default, no configuration needed)

Every server timestamp already comes from the system clock. `OsClockSource`
(the default `TimeSyncSource`) reports this. The operator is responsible for
keeping the OS clock synchronized by whatever platform mechanism they choose
(chrony, systemd-timesyncd, w32time, a vendor NTP/PTP client, ...) — the
library does not verify that the OS clock is actually synchronized, only that
it derives timestamps from it, consistent with CU 2478's description
("Application supports time synchronization via features of a standard
operating system").

### CU 3802 — Configure Clock Skew (claimed)

`ServerConfig::max_acceptable_clock_skew_ns` (default 5,000,000,000ns = 5s)
sets the acceptable tolerance, in nanoseconds. Retrieve it as a `Duration` via
`ServerConfig::max_acceptable_clock_skew()`; a configured value of `0` falls
back to the default rather than being treated as zero tolerance. Compare it
against a `TimeSyncSource`'s `observed_skew` (when one reports it, e.g.
`UaHeaderTimeSyncSource`) to determine whether the server's synchronization is
within bounds.

Nanosecond granularity matters here specifically because of the PTP/gPTP
extension point below: those mechanisms typically operate at sub-microsecond
precision, and a millisecond-granular field could never express a tolerance
tight enough to be meaningful for them (the finest non-zero value expressible
would be 1ms — several orders of magnitude too coarse). `TimeSyncStatus`'s
`observed_skew` already carries full `Duration` (nanosecond) precision
regardless of this field; this only affects the configurable *tolerance*.

### CU 5505 — UA-based support (claimed, opt-in via `time-sync-ua`)

`UaHeaderTimeSyncSource` periodically polls a configured well-known OPC UA
endpoint (for example a Discovery Server) via a session-less `GetEndpoints`
call (OPC 10000-4 §5.5.4) and derives an observed clock offset from the
server's `ResponseHeader.timestamp` (OPC 10000-4 §7.33) — no new external
dependency, since it reuses the existing OPC UA client Service-call path.

```rust
use std::time::Duration;
use async_opcua_server::{ServerBuilder, UaHeaderTimeSyncSource};

let source = UaHeaderTimeSyncSource::new("opc.tcp://lds.example:4840", Duration::from_secs(60));
let (server, handle) = ServerBuilder::new()
    .with_config(config)
    .with_time_sync_source(source)
    .build()?;
```

Enable the `time-sync-ua` Cargo feature (off by default, so nano/micro
footprint builds are unaffected) to use it. The poll interval is clamped up
to a 1-second floor regardless of the configured value, so a pathologically
small interval cannot busy-loop the configured endpoint.

### CU 5793 — Support at least one mechanism (claimed, as a consequence)

Since CU 2478 is always claimed (the default `OsClockSource`) and CU 5505 is
available as a real, tested mechanism, this composite CU is satisfied
whenever either is in effect — which is always true for CU 2478 alone, even
with no configuration.

### CU 2479 / 2480 / 2786 — PTP, gPTP, NTP (extensible, not implemented in-library)

This library does **not** implement the PTP, gPTP, or NTP wire protocols.
This is a deliberate scope decision, grounded in how the OPC UA specification
itself treats these mechanisms: OPC 10000-84 §6.6.3.6 (the UAFX IA-station
Facet) lists gPTP time synchronization as delivered by the underlying
networking layer ("UAFX Networking / UAFX EthernetServices"), not the OPC UA
application layer. The same reasoning extends to PTP and NTP — they are
platform/network-stack concerns, typically already handled by an existing
daemon or hardware feature on any real deployment target.

If your platform already runs one of these mechanisms, implement
`TimeSyncSource` to report on it — the server never needs to speak the wire
protocol. See `async-opcua-server/examples/custom_time_sync.rs` for a minimal
example. A real implementation reads from whatever your platform already
tracks (e.g. `chronyc tracking` output, a PTP hardware clock, or a gPTP
daemon's status socket) rather than fabricating a status.

Claiming CU 2479/2480/2786 for your specific deployment is a statement about
your integration, not about this library — document which mechanism your
`TimeSyncSource` implementation reports for your deployed profile.
