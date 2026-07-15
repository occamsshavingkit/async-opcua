# Interface Contracts: Time Sync Profile Decision

These are the public/internal contracts the implementation MUST honor. Signatures
are the intended shape; minor naming/error-type adjustments are allowed at
implementation time so long as the behavioral contract holds.

---

## C1. `TimeSyncSource` trait (async-opcua-server, public)

```rust
/// A source of time-synchronization status for the server.
///
/// The server derives its timestamps from the OS clock; a `TimeSyncSource`
/// reports *how* that clock is (or is not) being kept synchronized, for
/// conformance and diagnostics. Implement this to integrate a platform time-sync
/// mechanism (NTP daemon, PTP hardware, gPTP stack) that already exists outside
/// this library.
pub trait TimeSyncSource: Send + Sync {
    /// A cheap, non-blocking snapshot of current sync status.
    fn status(&self) -> TimeSyncStatus;
}
```

**Contract**:
- `status()` MUST NOT block, panic, or perform I/O — it reads cached state.
- Object-safe: no generic methods, no `async fn`.

---

## C2. `TimeSyncMechanism` / `TimeSyncStatus` (async-opcua-server, public)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSyncMechanism {
    OsClock,
    Ntp,
    Ptp,
    Gptp,
    UaHeaderBased,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct TimeSyncStatus {
    pub mechanism: TimeSyncMechanism,
    pub synchronized: bool,
    pub last_sync: Option<opcua_types::DateTime>,
    pub observed_skew: Option<std::time::Duration>,
}
```

**Contract**:
- `synchronized == false` whenever no successful sync/observation has occurred yet
  or the most recent attempt failed.
- When `synchronized == false` due to a never-completed poll, `last_sync` and
  `observed_skew` are `None` (no stale zero).

---

## C3. `OsClockSource` (async-opcua-server, public, default)

```rust
#[derive(Debug, Default, Clone)]
pub struct OsClockSource;

impl TimeSyncSource for OsClockSource {
    fn status(&self) -> TimeSyncStatus { /* OsClock, true, now, None */ }
}
```

**Contract**:
- Always available (no feature gate, no optional deps).
- `status().mechanism == TimeSyncMechanism::OsClock`, `synchronized == true`,
  `last_sync == Some(current system time)`, `observed_skew == None`.
- Satisfies CU 2478.

---

## C4. `ServerBuilder::with_time_sync_source` (async-opcua-server, public)

```rust
impl ServerBuilder {
    pub fn with_time_sync_source(mut self, source: Arc<dyn TimeSyncSource>) -> Self;
}
```

**Contract**:
- Mirrors `with_authenticator` / `with_type_tree_getter`.
- If never called, `build()` installs `Arc::new(OsClockSource)` so
  `ServerInfo.time_sync_source` is always populated (satisfies FR-003).
- `ServerInfo` gains `pub time_sync_source: Arc<dyn TimeSyncSource>`.

---

## C5. `ServerConfig` clock-skew field (async-opcua-server, public)

```rust
// in ServerConfig
#[serde(default = "defaults::max_acceptable_clock_skew_ms")]
pub max_acceptable_clock_skew_ms: u64,

// accessor
impl ServerConfig {
    pub fn max_acceptable_clock_skew(&self) -> std::time::Duration;
}

// in the `constants` module (lib.rs)
pub const DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_MS: u64 = 5000;
```

**Contract**:
- Missing in deserialized config → default applied via `#[serde(default)]`.
- Stored value of `0` → accessor returns the default `Duration` (FR-005), and the
  effective value is documented.
- Satisfies CU 3802.

---

## C6. Client discovery timestamp method (async-opcua-client, public)

```rust
impl Client {
    /// Perform a `GetEndpoints` call against `server` and return the server's
    /// `ResponseHeader.timestamp` (the server's notion of current UTC time).
    ///
    /// Unlike `get_endpoints`, this surfaces the response header timestamp used
    /// for UA-based time synchronization (OPC 10000-4 §7.33, §5.5.4).
    pub async fn get_server_time_via_endpoints(
        &self,
        server: impl ConnectorBuilder,
    ) -> Result<opcua_types::DateTime, Error>;
}
```

**Contract**:
- Issues exactly one `GetEndpoints` (session-less) request.
- Returns `response_header.timestamp` on success.
- Propagates connection/service errors as `Err(Error)` — never panics.
- Name is indicative; the essential contract is "single discovery round-trip →
  server response-header timestamp, no Session required."

---

## C7. `UaHeaderTimeSyncSource` (async-opcua-server, `cfg(feature = "time-sync-ua")`)

```rust
#[cfg(feature = "time-sync-ua")]
pub struct UaHeaderTimeSyncSource { /* endpoint_url, poll_interval, state */ }

#[cfg(feature = "time-sync-ua")]
impl UaHeaderTimeSyncSource {
    pub fn new(endpoint_url: impl Into<String>, poll_interval: std::time::Duration) -> Arc<Self>;
}

#[cfg(feature = "time-sync-ua")]
impl TimeSyncSource for UaHeaderTimeSyncSource { /* status() reads shared state */ }
```

**Contract**:
- `mechanism == UaHeaderBased`.
- A background poll loop (spawned by the server at startup, `MissedTickBehavior::Skip`,
  driven by a `CancellationToken`) calls C6 each `poll_interval`:
  - success → `synchronized = true`, `last_sync = server_ts`,
    `observed_skew = Some(|server_ts − local_now|)`;
  - failure → `synchronized = false` (FR-009); MUST NOT panic.
- Gated so nano/micro/default builds without `time-sync-ua` never compile it and
  never pull in `async-opcua-client`.

---

## C8. Cargo feature (async-opcua-server/Cargo.toml)

```toml
[features]
time-sync-ua = ["async-opcua-client"]
```

**Contract**:
- Off by default.
- Enabling it turns on the already-optional `async-opcua-client` dependency (same
  mechanism `discovery-server-registration` uses).

---

## C9. Report-tool classification (tools/cu-coverage-report/src/lib.rs)

```rust
enum EvidenceStatus { Implemented, Partial, Gap, NeedsProof, SourceIssue, Extensible }
```

**Contract**:
- `Extensible` label string: `extensible`; note: "Satisfiable via user-supplied
  `TimeSyncSource`; documented extension point, not implemented in-library."
- After reclassification: 2478/3802/5505/5793 ∈ `implemented`; 2479/2480/2786 ∈
  `extensible`; none of the 7 remain `gap`.
- `specs/conformance-tester/CU-COVERAGE.md` is regenerated by running the tool
  binary (FR-012) — the file MUST NOT be hand-edited.
- Existing unit tests updated to assert the new statuses.

---

## C10. Documentation (docs/)

**Contract**:
- `docs/time-synchronization.md` exists and contains a per-canonical-profile table
  (Nano/Micro/Embedded/Standard) stating which Time Sync CUs are claimed and by
  which mechanism (FR-010), plus an explicit statement that PTP/gPTP/NTP are not
  implemented in-library and require a user-supplied `TimeSyncSource`, citing
  OPC-10000-84 §6.6.3.6 (FR-011).
- `docs/opcua-foundation-profile-roadmap.md` Time Sync section updated to reference
  it and reflect the resolved statuses.
