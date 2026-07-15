# Feature Specification: Time Sync Profile Decision

**Feature Branch**: `093-time-sync-profile`
**Created**: 2026-07-15
**Status**: Draft
**Input**: User description: "Close the Time Sync profile decision TODO item by resolving the 7 open Time Sync conformance units (CU 2478, 2479, 2480, 2786, 3802, 5505, 5793) currently reported as `gap` in specs/conformance-tester/CU-COVERAGE.md, either by implementing a real mechanism or by documenting an explicit, spec-grounded exclusion."

## Context

`specs/conformance-tester/CU-COVERAGE.md` (feature 092/PR #294) reports all 7 Time
Sync conformance units as `gap` across every profile closure that references them
(Nano/Micro/Embedded/Standard 2025). `TODO.md` calls this out as a "Time Sync
profile decision": close or explicitly exclude each CU, and add server-profile docs
and tests for whichever mechanisms are claimed.

The 7 CUs, per the canonical profile snapshot
(`/home/quackdcs/micro-opcua/profiles/opcua-profile-normalized-snapshot.json`):

| CU | Name | Description |
|---:|---|---|
| 2478 | Time Sync – OS based support | Application supports time synchronization via features of a standard operating system. |
| 2479 | Time Sync – IEEE 1588 (PTP) | Application supports time synchronization via the Precision Time Protocol. |
| 2480 | Time Sync – IEEE 802.1AS | Application supports time synchronization via IEEE 802.1AS (gPTP). |
| 2786 | Time Sync – NTP | Application supports time synchronization via the Network Time Protocol. |
| 3802 | Time Sync - Configure Clock Skew | Supports configuration of the acceptable clock skew. |
| 5505 | Time Sync – UA based support | Application supports time synchronization by use of the request/response header timestamps of a configured well-known source (e.g. a Discovery Server), applied periodically and configurably. |
| 5793 | Time Sync - Support | Support at least one of the optional ConformanceUnits for time synchronization mechanisms; documentation shall specify which mechanisms with which profiles are supported. |

### Specification Grounding

| Decision | OPC UA Section | What it establishes |
|---|---|---|
| Extensibility seam over embedded wire-protocol clients | OPC-10000-84 §6.6.3.6 IA-station Facet | Treats gPTP time synchronization as a dependency on the underlying network/device stack, not something the OPC UA application layer re-implements. The same reasoning applies to PTP and NTP: the mechanism lives below the OPC UA server, which only needs to report on it. |
| UA-based periodic sync source | OPC-10000-4 §7.33 ResponseHeader | Defines the `timestamp` field returned on every Service response, the basis for CU 5505. |
| Well-known, session-less polling target | OPC-10000-4 §5.5.4 GetEndpoints | A Discovery-class Service invokable without a Session, suitable as the periodic poll target for CU 5505. |

No base OPC UA information model (Part 5) defines a standard AddressSpace
ObjectType for generic time-synchronization status — unlike companion specs such as
MDIS (`OPC-30020` §6.13, `MDISTimeSyncObjectType`) or BACnet
(`OPC-30030` §8.2.1). Conformance for these CUs is therefore established through
server behavior, configuration, and documentation, not through new standard nodes.

Prior work this builds on: feature 024 (LDS RegisterServer / Discovery client
machinery) and the existing client stack's Service call path, both reused rather
than duplicated.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - OS Clock Time Sync and Configurable Skew Tolerance (Priority: P1)

As a server operator deploying async-opcua on a host with a synchronized OS clock
(via whatever mechanism the OS itself uses), I want the server to correctly claim
OS-based time synchronization support and let me configure the acceptable clock
skew, so that the server's conformance documentation accurately reflects its
default behavior without requiring any extra wiring.

**Why this priority**: This is the foundation every other story extends: it
introduces the `TimeSyncSource` extensibility trait and ships its only
always-available implementation. Every other CU in this feature is either an
alternative implementation of the same trait (5505) or a documented way to supply
one (2479/2480/2786). Without this story, there is no seam to hang the rest on.

**Independent Test**: Start a server with default configuration (no
`TimeSyncSource` configured). Query the server's reported time-sync status and
confirm it reports `OsClock`, synchronized, with a `last_sync` timestamp. Set
`max_acceptable_clock_skew` in server config to a specific value and confirm it is
loaded, validated, and retrievable.

**Acceptance Scenarios**:

1. **Given** a server started with no `TimeSyncSource` configured, **When** its
   time-sync status is queried, **Then** it reports mechanism `OsClock`,
   `synchronized = true`, and a non-null `last_sync` derived from the system
   clock (OPC UA CU 2478).
2. **Given** a server config with `max_acceptable_clock_skew` set to a duration,
   **When** the server starts, **Then** the configured value is loaded and
   available via the server's runtime state (OPC UA CU 3802).
3. **Given** a server config with an invalid `max_acceptable_clock_skew` (e.g.
   zero or negative where the type permits it), **When** the server starts,
   **Then** it falls back to a documented default rather than silently accepting
   a nonsensical tolerance.

---

### User Story 2 - UA-Based Periodic Time Sync (Priority: P2)

As a server operator without OS-level time sync (or wanting a second, OPC-UA-native
mechanism), I want to configure a well-known OPC UA endpoint (such as a Discovery
Server) that the server periodically polls, using the response header timestamp to
detect and report clock skew, so that CU 5505 is a real, tested capability rather
than a documentation claim.

**Why this priority**: This is the one mechanism the library can fully own without
any new external dependency — it only needs the existing client Service-call path.
It's the second-highest-value CU after the free 2478 claim, and it's what makes CU
5793 ("support at least one optional mechanism") true independent of the OS.

**Independent Test**: Configure a `UaHeaderTimeSyncSource` pointed at a running
OPC UA endpoint (e.g. a second local server instance). Verify it polls
`GetEndpoints` at the configured interval, computes an offset from the response
header timestamp vs. local system time, and updates the reported time-sync status
accordingly. Verify behavior when the configured endpoint is unreachable.

**Acceptance Scenarios**:

1. **Given** a server configured with a `UaHeaderTimeSyncSource` pointed at a
   reachable OPC UA endpoint, **When** the configured poll interval elapses,
   **Then** the source issues a Service call to that endpoint, reads the
   `ResponseHeader.timestamp` (OPC-10000-4 §7.33), and computes an observed skew
   against the local clock (OPC UA CU 5505).
2. **Given** a `UaHeaderTimeSyncSource` whose configured endpoint is unreachable,
   **When** a poll attempt fails, **Then** the source reports `synchronized =
   false` rather than panicking or leaving stale state silently marked as fresh.
3. **Given** a `UaHeaderTimeSyncSource` with `observed_skew` exceeding
   `max_acceptable_clock_skew` (US1), **When** the status is queried, **Then** the
   excess is visible in the reported status (ties CU 5505 to CU 3802).

---

### User Story 3 - Documented Extensibility for NTP / PTP / gPTP (Priority: P3)

As an implementor embedding async-opcua on a device that already has its own NTP
client, PTP hardware, or gPTP-capable network stack, I want a documented, minimal
`TimeSyncSource` implementation to plug that existing mechanism in, so that I can
claim CU 2479/2480/2786 for my deployment without the library re-implementing
those wire protocols.

**Why this priority**: Lowest priority because it produces no new runtime
behavior in the library itself — it's the documentation and example that turns
the US1 trait into a usable integration point for these three CUs. It depends on
US1 existing first.

**Independent Test**: Follow the documented extension guide to write a minimal
example `TimeSyncSource` impl (e.g. one that reads a fixed/mocked offset) and
confirm it compiles against the trait and is accepted by the server builder.

**Acceptance Scenarios**:

1. **Given** the published `TimeSyncSource` trait, **When** an implementor writes
   a type implementing it for `Ntp`, `Ptp`, or `Gptp` mechanism and passes it via
   the server builder, **Then** the server accepts it with no library changes
   required (OPC UA CU 2479, 2480, 2786; grounded in OPC-10000-84 §6.6.3.6 IA-station
   Facet's treatment of gPTP as a network-stack dependency).
2. **Given** the server-profile documentation, **When** an operator reads it,
   **Then** it states plainly that PTP/gPTP/NTP are not implemented in-library and
   must be supplied via `TimeSyncSource`, consistent with TODO.md's requirement to
   "add server-profile docs ... for whichever time-sync mechanisms are claimed."

### Edge Cases

- **No `TimeSyncSource` configured and OS clock claim disabled**: not a supported
  configuration — the built-in `OsClockSource` is always the default; there is no
  "no time sync at all" state to represent.
- **Multiple time-sync mechanisms configured**: out of scope for this feature —
  the server config accepts at most one `TimeSyncSource`. Composing multiple
  sources (e.g. falling back from UA-based to OS clock) is not required by any of
  the 7 CUs and is deferred.
- **Clock skew never observed** (e.g. `UaHeaderTimeSyncSource` never successfully
  polled yet): status must report `synchronized = false` and `observed_skew =
  None`, not a stale zero.
- **`max_acceptable_clock_skew` configured but no `TimeSyncSource` reports a skew
  value** (pure `OsClockSource`): the configured tolerance is retrievable but has
  nothing to compare against; this is not an error.

## Requirements *(mandatory)*

### Functional Requirements

#### TimeSyncSource Extensibility Point

- **FR-001**: The server MUST define a `TimeSyncSource` trait exposing sync
  status: mechanism (`OsClock` | `Ntp` | `Ptp` | `Gptp` | `UaHeaderBased` |
  `Custom`), `synchronized: bool`, `last_sync: Option<DateTime>`, and
  `observed_skew: Option<Duration>`.
- **FR-002**: The server builder MUST accept an optional `TimeSyncSource`
  implementation, following the existing builder pattern used for other
  pluggable server behaviors (e.g. `with_authenticator`).
- **FR-003**: When no `TimeSyncSource` is configured, the server MUST default to
  a built-in implementation reporting mechanism `OsClock`, `synchronized = true`,
  derived from the system clock (OPC UA CU 2478).

#### Configurable Clock Skew

- **FR-004**: The server configuration MUST accept a `max_acceptable_clock_skew`
  duration field (OPC UA CU 3802).
- **FR-005**: If `max_acceptable_clock_skew` is absent or invalid, the server
  MUST apply a documented default rather than rejecting startup or silently
  accepting an unusable value.
- **FR-006**: The server's runtime status MUST make the configured
  `max_acceptable_clock_skew` and, when available, the active
  `TimeSyncSource`'s `observed_skew` retrievable together, so a caller can
  determine whether observed skew exceeds the configured tolerance.

#### UA-Based Periodic Time Sync

- **FR-007**: The server MUST provide a built-in `UaHeaderTimeSyncSource`
  implementation of `TimeSyncSource` that periodically issues a Service call
  (GetEndpoints, OPC-10000-4 §5.5.4) to a configured well-known OPC UA endpoint
  and reads `ResponseHeader.timestamp` (OPC-10000-4 §7.33) to compute an
  observed offset against the local system clock (OPC UA CU 5505).
- **FR-008**: `UaHeaderTimeSyncSource`'s poll interval MUST be configurable.
- **FR-009**: If a poll attempt fails (endpoint unreachable, malformed response),
  `UaHeaderTimeSyncSource` MUST report `synchronized = false` rather than
  retaining stale synchronized state or panicking.

#### Documented Extension Point for Hardware/Network Mechanisms

- **FR-010**: The server-profile documentation MUST state, per profile, which
  Time Sync CUs are claimed and by which mechanism (built-in `OsClock`, built-in
  `UaHeaderBased`, or user-supplied `TimeSyncSource`), satisfying TODO.md's
  "add server-profile docs ... for whichever time-sync mechanisms are claimed."
- **FR-011**: The documentation MUST explicitly state that CU 2479 (PTP), 2480
  (gPTP), and 2786 (NTP) are not implemented in-library and are satisfiable only
  by an implementor-supplied `TimeSyncSource`, grounded in OPC-10000-84 §6.6.3.6's
  treatment of gPTP as a network-stack dependency rather than an OPC UA
  application-layer concern.
- **FR-012**: The `specs/conformance-tester/CU-COVERAGE.md` evidence sourced by
  `tools/cu-coverage-report` MUST reflect the resolved status of all 7 CUs after
  this feature: 2478/3802/5505/5793 evidenced as implemented (tested), and
  2479/2480/2786 evidenced as a documented, tested extensibility point rather
  than left as `gap`.

### Key Entities

- **TimeSyncSource**: Extensibility trait reporting time-sync mechanism and
  status; the seam every CU in this feature resolves through.
- **OsClockSource**: Built-in default `TimeSyncSource` backed by the system
  clock (CU 2478).
- **UaHeaderTimeSyncSource**: Built-in `TimeSyncSource` that periodically derives
  offset from a well-known OPC UA endpoint's response header timestamps (CU 5505).
- **TimeSyncStatus**: The reported state (mechanism, synchronized, last_sync,
  observed_skew) exposed by any `TimeSyncSource`.
- **max_acceptable_clock_skew**: Server configuration field expressing the
  operator's configured tolerance (CU 3802).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 7 Time Sync CUs (2478, 2479, 2480, 2786, 3802, 5505, 5793) have
  a non-`gap` status in `specs/conformance-tester/CU-COVERAGE.md` after this
  feature, each backed by either a passing test or an explicit documented
  extension-point statement.
- **SC-002**: A server with zero time-sync configuration correctly reports
  OS-based synchronization out of the box (CU 2478), verified by an automated
  test.
- **SC-003**: An operator can configure and retrieve an acceptable clock-skew
  tolerance (CU 3802), verified by an automated test.
- **SC-004**: A server configured with `UaHeaderTimeSyncSource` against a
  reachable endpoint detects and reports clock offset within one poll interval
  (CU 5505), verified by an automated test using two local server/client
  instances.
- **SC-005**: An implementor can write and register a custom `TimeSyncSource`
  (standing in for NTP/PTP/gPTP) using only the published trait, verified by a
  compiling example.
- **SC-006**: No existing server or client integration tests regress.

## Assumptions

- "OS-based support" (CU 2478) is satisfied by the server deriving its
  timestamps from the system clock and documenting that operators are
  responsible for keeping that clock synchronized (via whatever OS-level
  mechanism they choose); the library does not need to detect or verify that the
  OS clock is actually synchronized.
- PTP (2479), gPTP (2480), and NTP (2786) wire-protocol clients are explicitly
  out of scope for this feature and are not planned for a future one on the same
  rationale used for gPTP in OPC-10000-84 §6.6.3.6: these are network/hardware
  stack concerns, not OPC UA application-layer concerns.
- `UaHeaderTimeSyncSource`'s "well-known source" is any configured OPC UA
  endpoint the operator trusts (commonly a Discovery Server, per CU 5505's
  description) — it is not required to be a Discovery Server specifically.
- Composing multiple simultaneous `TimeSyncSource`s (e.g. automatic fallback) is
  out of scope; the server accepts exactly zero or one.
