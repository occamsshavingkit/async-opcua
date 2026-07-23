# Feature Specification: Complete the 27 Partial Conformance Units

**Feature Branch**: `109-partial-cu-completion`
**Created**: 2026-07-23
**Status**: Draft
**Input**: User description: "Complete the 27 partial OPC UA conformance units (CUs) — raising each from Partial to Implemented in the CU-coverage ledger by adding the missing tests and the small missing wiring."

## Overview

The CU-coverage ledger (`tools/cu-coverage-report/src/lib.rs` `AUDIT_TABLE`, mirrored to `specs/conformance-tester/CU-COVERAGE.md`) currently classes 27 conformance units as **Partial**. Each Partial is one of two kinds:

- **Type A — test-only**: the code path already exists and is believed correct, but no dedicated test proves the specific CU behavior. Closing it means writing an independent test that asserts the spec-mandated observable behavior.
- **Type B — implementation-gap**: a value, reference, or audit event is genuinely never populated/wired/emitted. Closing it means a small, surgical implementation fix **plus** a test.

This feature closes all 27 and flips their ledger rows to **Implemented** with fresh evidence. It is deliberately shaped so a small local coding model can execute each task mechanically: every task is scoped to one CU, names the symbol to locate, names the OPC UA specification section to read first (via the OPC-UA reference MCP), and states the exact behavior to implement/assert.

The three time-sync extension-point CUs (2479 PTP, 2480 gPTP, 2786 NTP) are **out of scope** — they are deliberate user-supplied extension points, correctly classified Extensible, not Partial.

## User Scenarios & Testing *(mandatory)*

Note: the "users" of this feature are (a) OPC UA client applications and conformance-test tooling that exercise the server, and (b) maintainers who read the CU-coverage ledger to know what is genuinely implemented. Each acceptance scenario below corresponds to exactly one CU and is independently testable.

### User Story 1 - Alarms & Conditions completion (Priority: P1)

Close the five A&C partials: prove the Trip discrete-alarm kind, wire the two missing state-machine reference/property sets, wire the event-source hierarchy, and cover Respond2.

**Why this priority**: three of the five (2811, 2814, 2918) are real wiring gaps that leave the address space less navigable/complete than the spec requires; A&C is a heavily-audited profile area.

**Independent Test**: run the A&C server test suite; each CU has a dedicated test that a reviewer can run in isolation.

**Acceptance Scenarios**:

1. **CU 2275 (Trip)** — **Given** a Trip-kind discrete alarm, **When** its input crosses into the trip condition and back, **Then** the alarm activates and returns to normal and its TypeDefinition resolves to TripAlarmType.
2. **CU 2811 (GeneratesEvent)** — **Given** an instantiated ProgramStateMachine / ShelvingStateMachine, **When** a client browses its GeneratesEvent references, **Then** the reference(s) to the event type(s) it emits are present and browsable.
3. **CU 2814 (AvailableStates/AvailableTransitions)** — **Given** an instantiated finite state machine, **When** a client reads its AvailableStates and AvailableTransitions Properties, **Then** they return the machine's real set of states and transitions (not null).
4. **CU 2918 (HasEventSource)** — **Given** an event-source object under the Server notifier hierarchy, **When** a client browses HasEventSource references, **Then** the source hierarchy is navigable per Part 3 §7.
5. **CU 4466 (Respond2)** — **Given** an active dialog condition, **When** a client calls Respond2, **Then** the dialog transitions/validates exactly as the existing Respond path does.

---

### User Story 2 - Historical Access completion (Priority: P1)

Close the two history partials: cover event-history Update (upsert) mode on both backends, and fix + prove distinct server/source timestamps on read.

**Why this priority**: 2950 contains a genuine bug (the in-memory backend ignores `timestamps_to_return`), which is a correctness defect, not just a coverage gap.

**Independent Test**: run the history test suites for both in-memory and sqlite backends.

**Acceptance Scenarios**:

1. **CU 2289 (UpdateEvent)** — **Given** an existing historical event, **When** a client issues HistoryUpdate with PerformUpdateType::Update, **Then** the event is upserted (updated in place if present, inserted if absent) — verified on both the in-memory and sqlite backends.
2. **CU 2950 (distinct timestamps)** — **Given** a historical value stored with a source timestamp distinct from its server timestamp, **When** a client reads it back requesting both timestamps, **Then** both distinct timestamps are returned; **and** when requesting only one, only that one is returned — verified on both backends (the in-memory backend must honor `timestamps_to_return` rather than ignoring it).

---

### User Story 3 - Audit event coverage (Priority: P1)

Close the four audit partials: verify audit delivery over an encrypted channel, cover the three untested NodeManagement audit events, and emit the two audit event types that currently never fire (role-mapping-changed, history-update).

**Why this priority**: two are real emission gaps (3542, 3968) — audit event types that exist in the nodeset but no code ever raises.

**Independent Test**: run the audit and RBAC/history test suites.

**Acceptance Scenarios**:

1. **CU 2422 (encrypted audit)** — **Given** a session on a SignAndEncrypt channel, **When** an audit-generating action occurs, **Then** the audit event is delivered over the encrypted channel.
2. **CU 3224 (NodeManagement audit)** — **Given** DeleteNodes / AddReferences / DeleteReferences service calls, **When** each executes, **Then** the corresponding audit event type is emitted (each covered by its own test; AddNodes already covered).
3. **CU 3542 (RoleMappingRuleChanged)** — **Given** a change to a role's identity-mapping rules, **When** the change is applied via the role-management method, **Then** RoleMappingRuleChangedAuditEventType is emitted.
4. **CU 3968 (HistoryUpdate audit)** — **Given** a HistoryUpdate service call, **When** it executes, **Then** the correct AuditHistory*UpdateEventType subtype is emitted.

---

### User Story 4 - RBAC well-known role permissions (Priority: P2)

Close the three RBAC partials: assert the permission bitsets of ConfigureAdmin, AuthenticatedUser, and Observer/Engineer/Supervisor against their Part 3 well-known-role definitions.

**Why this priority**: pure test-only; the presets exist and are used, only assertions are missing.

**Independent Test**: run the RBAC test suite.

**Acceptance Scenarios**:

1. **CU 3539 (ConfigureAdmin)** — **Given** the ConfigureAdmin preset, **When** its permission bitset is inspected, **Then** it matches the Part 3 well-known-role definition.
2. **CU 3540 (AuthenticatedUser)** — **Given** the AuthenticatedUser preset, **When** its permission bitset is inspected, **Then** it matches the Part 3 definition.
3. **CU 3541 (Observer/Engineer/Supervisor)** — **Given** each of the Observer, Engineer, and Supervisor presets, **When** its permission bitset is inspected, **Then** it matches the Part 3 definition.

---

### User Story 5 - Subscriptions & MonitoredItems (Priority: P2)

Close the five subscription partials: prove queue-size clamping, structured-value monitoring, XML/JSON data-encoding on monitored items, MonitoredItem-level IndexRange, and ResendData.

**Why this priority**: all test-only; the pipelines exist and are believed correct.

**Independent Test**: run the subscription/monitored-item test suite.

**Acceptance Scenarios**:

1. **CU 2318 (queueSize clamp)** — **Given** a monitored item requested with a queueSize above the server maximum, **When** it is created, **Then** the revised queueSize is clamped to the server maximum. (Event-monitored-item queueing expansion is out of scope.)
2. **CU 2818 (structured value)** — **Given** a node holding a Structure value, **When** a client monitors it, **Then** the notification carries the structured value.
3. **CU 3142 (data encoding)** — **Given** a monitored item created with an XML (and/or JSON) dataEncoding, **When** it publishes, **Then** the notification value is encoded as requested.
4. **CU 5208 (IndexRange)** — **Given** a monitored item created with an IndexRange over an array node, **When** it publishes, **Then** only the ranged sub-value is delivered.
5. **CU 3544 (ResendData)** — **Given** a subscription with monitored items, **When** a client calls ResendData, **Then** the next publish resends current values for those items.

---

### User Story 6 - Read/Write/Call value handling (Priority: P3)

Close the three value-handling partials: writing a structured value, calling a method with a Structure argument, and enforcing MaxNodesPerMethodCall.

**Why this priority**: all test-only; generic Variant handling already accepts these.

**Independent Test**: run the read/write/call service test suites.

**Acceptance Scenarios**:

1. **CU 2203 (Write structure)** — **Given** a writable node of a structured DataType, **When** a client writes an ExtensionObject value and reads it back, **Then** the value round-trips.
2. **CU 2454 (Call structure arg)** — **Given** a method with a Structure input argument, **When** a client calls it with an ExtensionObject argument, **Then** the argument round-trips into the method body.
3. **CU 3605 (MaxNodesPerMethodCall)** — **Given** a Call request exceeding MaxNodesPerMethodCall, **When** it is issued, **Then** the server enforces the limit with the spec-defined status.

---

### User Story 7 - Server metadata, event fields, filter limits, custom types (Priority: P3)

Close the four remaining partials: read Server_LocalTime, populate BaseEventType.localTime, live-wire the two event-filter limit nodes, and add a custom-EventType/encoding completeness test.

**Why this priority**: two are test-only (2476, 3201); two are small value-population gaps (3546, 3194).

**Independent Test**: run the server-metadata / event / custom-codegen test suites.

**Acceptance Scenarios**:

1. **CU 2476 (Server_LocalTime)** — **Given** a running server, **When** a client reads Server_LocalTime, **Then** it returns a plausible TimeZoneDataType.
2. **CU 3546 (event localTime)** — **Given** an emitted event, **When** a client reads its localTime field, **Then** it carries the server's timezone value (not null).
3. **CU 3194 (filter limits)** — **Given** the MaxSelectClauseParameters and MaxWhereClauseParameters ServerCapabilities nodes, **When** a client reads them, **Then** they return the server's real event-filter limits (not null).
4. **CU 3201 (custom types)** — **Given** the custom-codegen sample's custom namespace, **When** it is browsed, **Then** every custom EventType is exposed alongside its corresponding Encoding object(s).

---

### User Story 8 - Security hardening: auth-failure protection (Priority: P3)

Close the one security partial: authentication-failure brute-force protection.

**Why this priority**: single CU; the existing 100ms tarpit likely already satisfies the requirement, but the exact behavior must be resolved against the spec text during research and then proven by test.

**Independent Test**: run the session-security test suite.

**Acceptance Scenarios**:

1. **CU 2823 (auth-failure protection)** — **Given** repeated failed authentication attempts, **When** they occur, **Then** the server delays/limits them per the spec-mandated behavior resolved in research (either the existing fixed tarpit proven by test, or a bounded escalating backoff if the spec mandates escalation).

---

### Edge Cases

- **Stale audit line references**: the audit's file:line citations predate features 106/107/108 and nodeset renumbering; every task re-locates code by symbol, never by the audit's line numbers.
- **Feature-gated code**: some CUs live behind Cargo features (history-sqlite, subscriptions, rbac, etc.); tests must run under the feature set that compiles the code under test, and the no-default-features leg must still build.
- **Both history backends**: CUs 2289 and 2950 must be proven on in-memory AND sqlite; a fix or test that passes on only one does not close the CU.
- **Independent test authorship**: a test that merely re-runs the implementation without asserting the spec-mandated observable outcome does not close a CU.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Each of the 27 named CUs MUST have a dedicated, independently-runnable test asserting the spec-mandated observable behavior for that CU.
- **FR-002**: Each Type-B (implementation-gap) CU MUST have its missing value/reference/audit-event added with the minimal surgical change described in its scenario — no surrounding-subsystem redesign.
- **FR-003**: Every task MUST re-locate current code by grepping for the named symbol and MUST read the cited OPC UA specification section (via the OPC-UA reference MCP) before implementing or testing.
- **FR-004**: CUs 2289 and 2950 MUST be verified on both the in-memory and sqlite history backends.
- **FR-005**: CU 2950 MUST fix the in-memory backend so it honors `timestamps_to_return` (returning only requested timestamps), matching the sqlite backend.
- **FR-006**: CU 2823 MUST be resolved to a single concrete behavior during research (tarpit-is-sufficient vs. bounded-escalating-backoff) so the implementer does not choose. **Resolved (research R1): tarpit-is-sufficient — no escalation.** Part 2 §6.6 and CR 1.11 make temporary lockout explicitly optional, and an escalating per-source map would be an attacker-influenced unbounded allocation (Principle IV liability). No new lock or per-source state is added to the authentication path.
- **FR-007**: On completion, `tools/cu-coverage-report/src/lib.rs` `AUDIT_TABLE` MUST flip all 27 rows Partial → Implemented with re-verified file:line + test-name evidence, and `specs/conformance-tester/CU-COVERAGE.md` MUST be regenerated via the cu-coverage-report tool.
- **FR-008**: The full workspace MUST pass `cargo test` (all-features, plus no-default-features where a feature gate matters), `cargo clippy --all-targets --all-features`, and `cargo fmt --all -- --check` before any PR.
- **FR-009**: The 3 extensible time-sync CUs (2479, 2480, 2786) MUST remain classified Extensible and MUST NOT be modified.

### Key Entities

- **Conformance Unit (CU)**: a numbered OPC UA Foundation conformance requirement; tracked in the ledger with a status (Implemented/Partial/Gap/Extensible/etc.) and evidence string.
- **AUDIT_TABLE**: the source-of-truth table (id, status, evidence) in `tools/cu-coverage-report/src/lib.rs` that generates CU-COVERAGE.md.
- **Type-A / Type-B CU**: test-only vs. implementation-gap; determines whether a task is "add test" or "add wiring + test".

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 27 named CUs show status `Implemented` in the regenerated CU-COVERAGE.md.
- **SC-002**: The Partial count in the ledger drops from 27 (excluding the 3 extensible) to 0; no CU regresses from Implemented to a lower status.
- **SC-003**: Each of the 27 CUs is traceable to a named test that fails if that CU's behavior is broken (verifiable by the evidence string naming the test).
- **SC-004**: All standard gates pass (test/clippy/fmt) on the full workspace.
- **SC-005**: No change touches the 141 Gap CUs, the 3 Extensible CUs, or introduces a new companion spec.

## Assumptions

- The audit's Type-A/Type-B classification per CU is a starting hypothesis; if research finds a "test-only" CU actually needs a small fix (or vice-versa), the task adapts, but the CU stays in scope.
- The OPC-UA reference MCP (search_terms/text/nodes/cu) is available to the downstream implementer for spec grounding; where it is not, the local PDFs at `~/opcua-specs/` are the fallback.
- "Minimal fix" for Type-B CUs means adding the missing reference/value/event at the existing instantiation/emission site, reusing existing machinery (e.g. the same timezone source for both Server_LocalTime and event localTime).
- The CU-COVERAGE.md regeneration uses the existing normalized-snapshot JSON the tool already consumes.
