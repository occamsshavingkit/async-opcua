# Quickstart: Time Sync Profile Decision

How the delivered feature is used and verified.

## 1. Default (OS clock) — CU 2478, zero config

```rust
use async_opcua_server::ServerBuilder;

// No time-sync configuration: the server defaults to OsClockSource.
let (server, handle) = ServerBuilder::new()
    .with_config(config)
    .build()?;

let status = handle.info().time_sync_source.status();
assert_eq!(status.mechanism, TimeSyncMechanism::OsClock);
assert!(status.synchronized);
```

The operator is responsible for keeping the OS clock synchronized (chrony,
systemd-timesyncd, w32time, …); the library reports OS-based support.

## 2. Configure acceptable clock skew — CU 3802

Via config file (serde), in nanoseconds — chosen over milliseconds because
PTP/gPTP sources (§4 below) typically operate at sub-microsecond precision,
and a millisecond-granular field could never express a meaningful tolerance
for them:

```json
{ "max_acceptable_clock_skew_ns": 2000000000 }
```

Or programmatically the field is on `ServerConfig`; read it back as a `Duration`:

```rust
let tolerance = server_config.max_acceptable_clock_skew(); // Duration::from_nanos(2_000_000_000)
```

A `0` value falls back to `DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_NS`.

## 3. UA-based periodic sync — CU 5505 (feature `time-sync-ua`)

```toml
# Cargo.toml
async-opcua-server = { version = "...", features = ["time-sync-ua"] }
```

```rust
use std::time::Duration;
use async_opcua_server::{ServerBuilder, UaHeaderTimeSyncSource};

let source = UaHeaderTimeSyncSource::new("opc.tcp://lds.example:4840", Duration::from_secs(60));

let (server, handle) = ServerBuilder::new()
    .with_config(config)
    .with_time_sync_source(source)
    .build()?;

// After the server starts and one interval elapses:
let status = handle.info().time_sync_source.status();
// status.mechanism == UaHeaderBased
// status.observed_skew == Some(|server_ts - local_now|) when reachable
// status.synchronized == false when the endpoint is unreachable
```

## 4. Plug in NTP / PTP / gPTP — CU 2479 / 2480 / 2786 (user-supplied)

The library does not implement these wire protocols (grounded in OPC-10000-84
§6.6.3.6: gPTP is a network-stack dependency). If your platform already has one,
wire it in:

```rust
use std::sync::Arc;
use std::time::Duration;
use async_opcua_server::{TimeSyncSource, TimeSyncStatus, TimeSyncMechanism};

struct MyNtpSource { /* handle to the platform NTP daemon / chrony socket */ }

impl TimeSyncSource for MyNtpSource {
    fn status(&self) -> TimeSyncStatus {
        // read whatever your NTP client already tracks
        TimeSyncStatus {
            mechanism: TimeSyncMechanism::Ntp,
            synchronized: true,
            last_sync: Some(opcua_types::DateTime::now()),
            observed_skew: Some(Duration::from_millis(3)),
        }
    }
}

let (server, handle) = ServerBuilder::new()
    .with_time_sync_source(Arc::new(MyNtpSource { /* ... */ }))
    .build()?;
```

A minimal compiling version of this ships as
`async-opcua-server/examples/custom_time_sync.rs`.

## 5. Verification checklist

| CU | How verified |
|---|---|
| 2478 | Unit/integration test: default build reports `OsClock`, synchronized. |
| 3802 | Unit test: config round-trips `max_acceptable_clock_skew_ns` at sub-millisecond precision; `0` → default. |
| 5505 | Integration test (`time-sync-ua`): two local instances; poller observes skew within one interval; unreachable → `synchronized = false`. |
| 5793 | Satisfied by 2478/5505 being real; asserted in docs table. |
| 2479/2480/2786 | `examples/custom_time_sync.rs` compiles; docs state the extension-point claim. |
| all 7 | `specs/conformance-tester/CU-COVERAGE.md` regenerated: none `gap`. |

## 6. Regenerate the CU coverage report (after the report-tool change)

```bash
cargo run -p async-opcua-cu-coverage-report -- \
  "$ASYNC_OPCUA_PROFILE_SNAPSHOT_DIR/opcua-profile-normalized-snapshot.json" \
  specs/conformance-tester/CU-COVERAGE.md
```

Confirm the 7 Time Sync rows now read `implemented` (2478/3802/5505/5793) or
`extensible` (2479/2480/2786), and none read `gap`.

## 7. Pre-PR gate

```bash
tools/ci-playbook.sh --ci
# plus the feature leg the default CI would otherwise miss:
cargo clippy -p async-opcua-server --features time-sync-ua --all-targets -- -D warnings
cargo test -p async-opcua-server --features time-sync-ua
```
