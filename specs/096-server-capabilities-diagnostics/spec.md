# Feature Specification: Server Capabilities & Diagnostics Conformance Completion

**Feature Branch**: `096-server-capabilities-diagnostics`
**Created**: 2026-07-16
**Status**: Draft
**Input**: User description: "Server Capabilities & Diagnostics conformance completion: close 6 required CUs across the Micro/Embedded/Standard 2025 server profiles by wiring the remaining static/null ServerCapabilities and ServerDiagnostics nodes to live config/limits, and adding the missing tests. CUs: 3911, 3912, 4053, 4055, 3196, 3808."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Server Capabilities report real configured limits (Priority: P1)

An integrator connects a monitoring or engineering tool to the server and
reads the `ServerCapabilities` node (and its `OperationLimits`/subscription
sub-nodes) to discover what the server actually supports, before configuring
subscriptions, sessions, or bulk operations against it. Today several of
these nodes report a static null value instead of the server's real
configured limit, so a client cannot reliably discover `MaxSessions` or
`MaxMonitoredItemsQueueSize` without triggering the failure mode directly.

**Why this priority**: This is the largest CU cluster (CUs 3911, 3912, 4055)
and the standard mechanism OPC UA clients use to discover server limits
before configuring themselves — a null value here isn't just an unfilled
field, it's actively misleading conformance-tool and client behavior.

**Independent Test**: Configure a server with explicit
`max_sessions`/`max_monitored_items_queue_size`/other unwired `Max*` limits,
read the corresponding `ServerCapabilities` attributes over the wire, and
confirm each returns the configured value rather than null or a stale
default.

**Acceptance Scenarios**:

1. **Given** a server configured with a specific `max_sessions` limit,
   **When** a client reads `ServerCapabilities.MaxSessions`, **Then** the
   value read back matches the configured limit.
2. **Given** a server configured with a specific monitored-item queue size
   limit, **When** a client reads
   `ServerCapabilities.OperationLimits` (or the appropriate
   `MaxMonitoredItemsQueueSize` node), **Then** the value read back matches
   the limit already enforced when creating monitored items.
3. **Given** any other `ServerCapabilities`/`OperationLimits` node presently
   reporting a static null identified during implementation, **When** a
   client reads it, **Then** it reports the real configured or effective
   value instead of null.

---

### User Story 2 - Sampling interval diagnostics correctly reflect this server's variable-interval model (Priority: P2)

*(Revised after Phase 0 research — see plan.md/research.md. The standard
`SamplingIntervalDiagnosticsArray` is explicitly conditional on a server
using a **fixed** set of sampling intervals; this server negotiates a
continuously-variable, client-requested interval per monitored item, so
the array does not apply. This story is now about clearly documenting
*why* it's correctly absent, not building it.)*

An engineer reading this project's conformance documentation or source
comments, upon noticing the server does not expose
`SamplingIntervalDiagnosticsArray`, wants to immediately understand this is
a deliberate, spec-conformant choice tied to the server's sampling-interval
model — not an overlooked gap.

**Why this priority**: Second-highest priority in this cluster by original
CU-count weighting; the investigation itself (confirming the array's
precondition doesn't hold) is what closes the CU, with a short
documentation note left behind for future maintainers.

**Independent Test**: A reviewer reads the documentation note and the
`sanitize_sampling_interval` code it references, and confirms the stated
reasoning holds (this server accepts continuously-variable client-requested
sampling intervals, not a fixed set).

**Acceptance Scenarios**:

1. **Given** the project's conformance/capacity documentation, **When** a
   reviewer looks for why `SamplingIntervalDiagnosticsArray` is absent,
   **Then** they find a clear note explaining the server's variable-interval
   model and the specific spec clause that makes non-exposure conformant.

---

### User Story 3 - Locations object is reachable via Browse (Priority: P3)

A client browsing the server's information model to discover the standard
`Locations` addressing structure finds it present and correctly connected,
not just defined in the generated nodeset but unreachable from the address
space root.

**Why this priority**: Cheapest item in this cluster — the node and its
wiring already exist; this closes on verification alone.

**Independent Test**: Browse from the server object to the `Locations`
object using the standard hierarchical path and confirm it resolves.

**Acceptance Scenarios**:

1. **Given** a running server, **When** a client browses the standard path
   to the `Locations` object, **Then** the browse resolves to the expected
   node.

---

### User Story 4 - Server capacity limits are documented (Priority: P4)

A systems integrator evaluating this server for an embedded deployment
wants a single reference document enumerating the server's core capacity
limits (max secure channels, sessions, subscriptions, and similar), rather
than having to read scattered configuration source to find them.

**Why this priority**: Documentation-only, no runtime behavior changes;
lowest priority but cheap to close once the other three stories land and
their configured limits are known.

**Independent Test**: A reviewer reads the published document and confirms
it lists each core capacity limit with its current default and how it's
configured, matching the actual `ServerConfig`/`limits.rs` values.

**Acceptance Scenarios**:

1. **Given** the published capacity documentation, **When** a reviewer
   cross-checks each listed limit against the corresponding configuration
   default in the codebase, **Then** every listed value matches.

---

### Edge Cases

- A `ServerCapabilities` limit that has no configured value (server built
  with defaults) must still report the built-in default, never null, once
  wired.
- If a future change gives this server a genuinely fixed sampling-interval
  mode, the documented reasoning for US2 must be revisited — it holds only
  for the current continuously-variable model.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST report its configured maximum session count
  through the standard `ServerCapabilities` addressing structure.
- **FR-002**: The server MUST report its configured (or enforced default)
  monitored-item queue size limit through the standard `ServerCapabilities`
  addressing structure.
- **FR-003**: The server MUST report any other identified unwired
  `ServerCapabilities`/`OperationLimits` node's real configured or effective
  value rather than a static null, once identified during implementation.
- **FR-004**: The project MUST document why `SamplingIntervalDiagnosticsArray`
  is not exposed, citing the server's continuously-variable sampling-interval
  model and the specific OPC UA spec clause that makes non-exposure
  conformant.
- **FR-005**: The standard `Locations` addressing object MUST be reachable
  via a Browse from the server's root address space, with a test proving
  the path resolves.
- **FR-006**: The project MUST publish a document enumerating the server's
  core capacity limits (at minimum: max secure channels, max sessions, max
  subscriptions per session, max monitored items per subscription) with
  their defaults and how each is configured.

### Key Entities

- **ServerCapabilities**: The standard OPC UA information-model structure a
  client reads to discover what a server supports before configuring
  itself; several of its sub-nodes are the subject of this feature.
- **Core Capacity Document**: A single reference artifact enumerating the
  server's built-in capacity limits and their configuration.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All identified unwired `ServerCapabilities`/`OperationLimits`
  nodes (starting with `MaxSessions` and the monitored-item queue size
  limit) report the server's real configured or effective value in 100% of
  reads, verified by automated test.
- **SC-002**: The documented rationale for not exposing
  `SamplingIntervalDiagnosticsArray` cites a specific spec clause and holds
  up against the actual sampling-interval negotiation code (reviewer- or
  test-verifiable).
- **SC-003**: A Browse to the `Locations` object succeeds in an automated
  test.
- **SC-004**: A published capacity document exists and every value in it is
  verified (by a reviewer or automated check) to match the corresponding
  configuration default.
- **SC-005**: All six target conformance units (3911, 3912, 4053, 4055,
  3196, 3808) move from their current `gap`/`partial` evidence status to
  `implemented` in the project's conformance evidence register, each with a
  file:line/test citation.

## Assumptions

- "Real configured value" for a `ServerCapabilities` node means: the
  value the server would actually enforce, whether that comes from explicit
  user configuration or a sensible built-in default when unconfigured —
  never a placeholder null.
- `SamplingIntervalDiagnosticsArray` is conditional on the server using a
  fixed set of sampling intervals (OPC-10000-5 §7.9/§12.8); this server
  negotiates a continuously-variable, client-requested interval per
  monitored item, so non-exposure is the conformant choice — confirmed
  during planning against `sanitize_sampling_interval`'s actual behavior.
- The core capacity document is a project documentation artifact (e.g. a
  Markdown file under `docs/`), not a new runtime API — no new node or
  service is implied by FR-006.
