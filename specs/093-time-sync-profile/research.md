# Phase 0 Research: Time Sync Profile Decision

All unknowns from Technical Context are resolved below. Each decision is grounded
in the actual codebase and/or OPC UA spec.

## R1. Where does the `TimeSyncSource` trait and its default live?

**Decision**: `TimeSyncSource`, `TimeSyncMechanism`, `TimeSyncStatus`, and
`OsClockSource` live in a new always-compiled module `async-opcua-server/src/time_sync.rs`,
re-exported from `lib.rs`. The active source is held on `ServerInfo`
(`async-opcua-server/src/info.rs`) as `Arc<dyn TimeSyncSource>`.

**Rationale**:
- `ServerInfo` is the established instance-scoped runtime home for pluggable
  behaviors (`authenticator: Arc<dyn AuthManager>`, `type_tree_getter: Arc<dyn
  TypeTreeForUser>` — info.rs:194,215). Feature 049 explicitly moved process-global
  statics onto `ServerInfo` for multi-instance-per-process safety; putting the
  time-sync source anywhere global would regress that.
- Core trait + `OsClockSource` have no optional dependencies (system clock only),
  so they stay footprint-safe for nano/micro profiles.

**Alternatives considered**:
- A process-global `OnceCell` — rejected, violates feature 049's instance-scoping.
- Putting it in `async-opcua-core` — rejected; it's a server-config concept, and
  no other crate needs it.

## R2. How is the pluggable source registered on the builder?

**Decision**: Add `ServerBuilder::with_time_sync_source(Arc<dyn TimeSyncSource>)`
storing `Option<Arc<dyn TimeSyncSource>>`, mirroring `with_authenticator`
(builder.rs:248) and `with_type_tree_getter` (builder.rs:309). In `build()`, if
`None`, install the default `OsClockSource` before constructing `ServerInfo`, so
`ServerInfo.time_sync_source` is always a concrete `Arc<dyn TimeSyncSource>` (never
`Option`).

**Rationale**: Exactly the existing "optional at builder, defaulted at build,
non-optional at runtime" pattern used for `authenticator`. Satisfies FR-002/FR-003
with zero new machinery.

## R3. `max_acceptable_clock_skew` config representation and default.

**Decision**: Add `max_acceptable_clock_skew_ms: u64` to `ServerConfig`
(config/server.rs) with `#[serde(default = "…")]`, plus a
`DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_MS` constant in the `constants` module in
`lib.rs`. Expose it as a `Duration` via an accessor. Validation: a zero value falls
back to the default (FR-005).

**Rationale**: The config crate stores every duration as a `u64`/`u32` `_ms` field
(`subscription_poll_interval_ms`, `publish_timeout_default_ms`,
`max_session_timeout_ms` — config/server.rs:562,565,585), serde-friendly and
consistent. Default constants live in the `constants` module in lib.rs
(lib.rs:127). Matching this convention is required by the constitution's "existing
conventions" gate.

**Default value**: 5000 ms (5 s) — a conventional NTP-era "acceptable skew"
starting point; documented and operator-overridable. (Not spec-mandated; OPC UA
leaves the tolerance to the application, which is exactly why CU 3802 is
"configure the acceptable clock skew".)

**Correction (found in review, before push/merge)**: `_ms` granularity is
wrong for this feature's own stated extensibility target. CU 2479/2480
(PTP/gPTP) exist specifically because those mechanisms report sub-microsecond
skew; a millisecond-granular tolerance field cannot express anything finer
than 1ms, making the field useless for the exact use case the feature was
built to support. Changed the field to `max_acceptable_clock_skew_ns: u64`
(nanoseconds), default `5_000_000_000` (still 5s, same effective default,
now just expressed in the finer unit). `TimeSyncStatus.observed_skew` was
never affected — it was `Duration` (nanosecond-precise) from the start; only
the configurable *tolerance* had the granularity defect. The `_ms` naming
convention cited above (`subscription_poll_interval_ms`, etc.) is still the
right default for durations in this codebase — this field is a deliberate,
justified exception because of what it's being compared against.

## R4. Surfacing `ResponseHeader.timestamp` for the UA-based source (CU 5505).

**Decision**: Add a focused method to the client's discovery surface
(`async-opcua-client/src/session/client.rs`) that performs a `GetEndpoints` call
and returns the server's `ResponseHeader.timestamp` (`opcua_types::DateTime`),
rather than discarding it. `UaHeaderTimeSyncSource` calls this method each poll.

**Rationale**:
- The existing public `Client::get_endpoints` (client.rs:393) returns only
  `Vec<EndpointDescription>` and throws away the `ResponseHeader`
  (`process_service_result(&response.response_header)` at client.rs:352 is the last
  time the header is seen). CU 5505 needs precisely `ResponseHeader.timestamp`
  (OPC-10000-4 §7.33), so a small, dedicated addition is the correct fix rather
  than copy-pasting `get_server_endpoints_inner` into the server crate (Principle
  II: no copy-paste, fix the root cause).
- `GetEndpoints` (OPC-10000-4 §5.5.4) is session-less and invokable against a bare
  discovery endpoint — ideal for a lightweight periodic poll against a "well-known
  source such as a Discovery Server" (CU 5505 description).

**Alternatives considered**:
- Reading `find_servers` response header instead — equivalent; GetEndpoints chosen
  because it is the most universally available discovery service and already has a
  clean single-shot entry point.
- A full session Read of `Server_ServerStatus_CurrentTime` — rejected: heavier
  (needs a Session), and CU 5505 explicitly describes using request/response header
  timestamps, not an address-space read.

## R5. Crate/feature gating for `UaHeaderTimeSyncSource`.

**Decision**: New off-by-default Cargo feature in `async-opcua-server/Cargo.toml`:
`time-sync-ua = ["async-opcua-client"]`. `time_sync_ua.rs` and the poll-loop spawn
site in `server.rs` are `#[cfg(feature = "time-sync-ua")]`.

**Rationale**:
- The server already declares `async-opcua-client` as an **optional** dependency
  (server Cargo.toml:287) enabled by `discovery-server-registration`
  (Cargo.toml:105, feature 024). The UA time-sync poller needs the same client
  capability, so it reuses that optional dep behind its own feature flag.
- A **separate** feature (not reusing `discovery-server-registration`) is chosen so
  that enabling UA time sync does not force LDS registration and vice-versa — they
  are independent capabilities. Off-by-default keeps nano/micro footprints
  unchanged (constitution footprint concern; memory `todo-embedded-profiles`).

**Alternatives considered**:
- Reuse `discovery-server-registration` — rejected: conflates two unrelated
  capabilities.
- Always-on — rejected: would pull the client crate into every server build,
  regressing footprint.

## R6. Poll-loop structure for `UaHeaderTimeSyncSource`.

**Decision**: Model the poll loop exactly on
`async-opcua-server/src/discovery.rs`: a `tokio::time::interval` loop with
`MissedTickBehavior::Skip`, spawned at server startup in `server.rs` (cfg-gated),
using a `CancellationToken` for shutdown, calling the R4 client method each tick and
updating an internal `RwLock`/atomic-backed `TimeSyncStatus`.

**Rationale**: `discovery.rs` (lines 88–113) is the in-repo precedent for a
feature-gated, client-using periodic server background task with clean shutdown.
Reusing its shape satisfies "existing conventions" and minimizes novel concurrency
surface. The shared status is read by the status accessor and written by the loop;
poll failure sets `synchronized = false` (FR-009).

## R7. Report-tool evidence category for documented extension points.

**Decision**: Extend `EvidenceStatus` in `tools/cu-coverage-report/src/lib.rs` with
a new `Extensible` variant (label e.g. `extensible`, note: "Satisfiable via
user-supplied `TimeSyncSource`; documented extension point, not implemented
in-library."). Reclassify:
- Remove all 7 CUs from `time_sync_gaps()` (delete that bucket or empty it).
- Add 2478, 3802, 5505, 5793 to `implemented_cus()`.
- Add 2479, 2480, 2786 to a new `extensible_cus()` bucket → `Extensible`.
Update the unit-test assertions (lib.rs:220,234) and **regenerate**
`specs/conformance-tester/CU-COVERAGE.md` via the tool binary (never hand-edit —
FR-012).

**Rationale**: The current classifier has only implemented/partial/gap/needs-proof/
source-issue; none expresses "we deliberately don't implement this but it's
reachable via a documented extension point." FR-012 requires distinguishing that
from `gap`. A dedicated variant keeps the report honest (Principle IV: don't
overclaim) while removing the misleading `gap`.

**Alternatives considered**:
- Marking 2479/2480/2786 as `implemented` — rejected: dishonest; the library ships
  no NTP/PTP/gPTP code.
- Leaving them `gap` and only documenting — rejected: FR-012 explicitly wants them
  off `gap` since a documented, compiling extension point is real evidence.

## R8. Documentation surface (FR-010/FR-011).

**Decision**: New `docs/time-synchronization.md` stating, per canonical profile
(Nano/Micro/Embedded/Standard), which Time Sync CUs are claimed and by which
mechanism, plus the extension guide for NTP/PTP/gPTP via `TimeSyncSource`. Update
the Time Sync section of `docs/opcua-foundation-profile-roadmap.md` to point at it
and reflect the new statuses.

**Rationale**: `docs/` already holds the profile/conformance narrative
(`opcua-foundation-profile-roadmap.md`, `ctt-conformance.md`, `compatibility.md`).
A dedicated time-sync doc is the natural home for the per-profile claim table
TODO.md asks for, and keeps the roadmap doc as an index rather than bloating it.

## R9. OPC UA grounding for the "don't implement the wire protocol" decision.

**Decision**: Cite OPC-10000-84 §6.6.3.6 (UAFX IA-station Facet) as the normative
basis that gPTP time synchronization is a dependency on the underlying network
stack, extended by analogy to PTP and NTP.

**Rationale**: Confirmed via the opc-ua-reference MCP: OPC-10000-84 §6.6.3.6 lists
"gPTP Time Synchronization" as an *optional* conformance unit of the IA-station
facet delivered by "UAFX Networking / UAFX EthernetServices" — i.e. the networking
layer, not the OPC UA application. No base Part-3/Part-5 information model defines a
generic time-sync ObjectType (only companion specs like MDIS `OPC-30020` §6.13 do),
confirming that generic time-sync conformance is behavioral/documentation-based,
not node-based.
