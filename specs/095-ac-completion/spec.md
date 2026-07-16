# Feature Specification: Alarms & Conditions Completion

**Feature Branch**: `095-ac-completion`
**Created**: 2026-07-16
**Status**: Draft
**Input**: User description: "Alarms & Conditions completion (feature 095). Close the largest confirmed-gap cluster from the 2026-07-15 conformance audit: 98 gaps + 7 partials out of 126 audited A&C (Part 9) CUs."

## Context

The 2026-07-15 conformance audit (`docs/conformance-audit-2026-07-15.md`,
`tools/cu-coverage-report`'s `AUDIT_TABLE`) found that prior "A&C complete"
claims (feature `feature-ac-completion`) covered only core alarm mechanics.
Of 126 audited Part 9 (Alarms & Conditions) conformance units, 98 are
confirmed gaps and 7 are partial. This feature closes the four largest,
highest-leverage gap themes; the remainder (Alarm Groups, AlarmMetrics, the
COM A&E wrapper facet, and A&C Previous-Instances niceties) is explicitly
deferred as lower-value/more niche.

**Already solid, not in scope**: ConditionType/AlarmConditionType base,
Acknowledge/Confirm/AddComment/Refresh/Refresh2, branching, shelving, dialog
respond, exclusive+non-exclusive limit alarms, and discrete OffNormal/Trip
alarms (`async-opcua-server/src/alarms/`, tested in
`async-opcua/tests/integration/alarms.rs`).

### Specification Grounding

| Area | OPC-10000-9 Section | What |
|---|---|---|
| TransitionTime/EffectiveTransitionTime/EffectiveDisplayName | §5.2 (Two-state state machines) | Sub-state timestamp/name properties on every two-state variable |
| Enable / Disable Methods | §5.5.5 / §5.5.4 (ConditionType) | Client-facing condition enable/disable |
| Suppress / Suppress2, Unsuppress / Unsuppress2 Methods | §5.8.8–§5.8.11 (AlarmConditionType) | Client-facing suppression, distinct from AlarmSuppressionGroup-driven auto-suppression |
| Silence Method | §5.8.7 (AlarmConditionType) | Client-facing audible/visible silence, requires SilenceState |
| RemoveFromService / RemoveFromService2, PlaceInService / PlaceInService2 Methods | §5.8.12–§5.8.15 (AlarmConditionType) | Client-facing OutOfServiceState set/clear |
| AuditConditionEnableEventType | §5.10.2 | Audit event for Enable/Disable |
| AuditConditionSilenceEventType | §5.10.10 | Audit event for Silence |
| AuditConditionOutOfServiceEventType | §5.10.12 | Audit event for RemoveFromService/PlaceInService |
| ExclusiveLevelAlarmType / NonExclusiveLevelAlarmType | §5.8.21.3 / §5.8.21.2 | Level alarm subtype |
| ExclusiveDeviationAlarmType / NonExclusiveDeviationAlarmType | §5.8.22.3 / §5.8.22.2 | Deviation alarm subtype |
| ExclusiveRateOfChangeAlarmType / NonExclusiveRateOfChangeAlarmType | §5.8.23.3 / §5.8.23.2 | Rate-of-change alarm subtype |
| SystemOffNormalAlarmType | §5.8.24.3 | System off-normal alarm subtype |
| CertificateExpirationAlarmType | §5.8.24.7 | Certificate expiration alarm subtype |
| DiscrepancyAlarmType | §5.8.25 | Discrepancy alarm subtype |

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Sub-State Transition Timestamps and Display Names (Priority: P1)

As an OPC UA client operator monitoring alarms, when a condition's sub-state
(enabled, active, acknowledged, confirmed, suppressed, shelved, or a limit
alarm's active limit) changes, I want to read when that sub-state was last
entered and a human-readable name for the current overall effective state, so
that I can build alarm history/timeline views and status displays without
inferring state transitions from raw event streams.

**Why this priority**: Highest CU-count leverage in the audit (58 of 98
gaps) because it's one underlying mechanism — a `TransitionTime`,
`EffectiveTransitionTime`, and `EffectiveDisplayName` addition applied
uniformly to `TwoStateVariableType`-derived sub-states — not 58 independent
features. Zero implementation exists today (grep-confirmed repo-wide).

**Independent Test**: Trigger a state change on each affected sub-state
(enable/disable a condition, activate/deactivate an alarm, acknowledge,
confirm, suppress/unsuppress, shelve/unshelve, and cross a limit-alarm
threshold). Read back `TransitionTime`, `EffectiveTransitionTime`, and
`EffectiveDisplayName` on the affected sub-state and confirm they reflect the
change.

**Acceptance Scenarios**:

1. **Given** a condition whose `EnabledState` changes from `false` to `true`,
   **When** a client reads `EnabledState.TransitionTime`, **Then** it
   reflects the time of that specific transition (OPC-10000-9 §5.2).
2. **Given** an alarm whose `ActiveState` changes, **When** a client reads
   `ActiveState.EffectiveTransitionTime`, **Then** it reflects the time the
   alarm's overall effective (shelving/suppression-adjusted) state last
   changed, which may differ from `ActiveState.TransitionTime` when the
   alarm is shelved or suppressed.
3. **Given** an alarm in any state, **When** a client reads
   `EffectiveDisplayName`, **Then** it returns a `LocalizedText` describing
   the current effective state (e.g. "Active | Unacknowledged" vs.
   "Shelved").
4. **Given** a `LimitAlarmType` instance crossing from normal into a
   `HighHigh` limit, **When** a client reads the `HighHigh` limit state's
   `TransitionTime`, **Then** it reflects that crossing.

### User Story 2 - Enable/Disable/Suppress/OutOfService/Silence Methods (Priority: P2)

As an OPC UA client operator, I want to call standard Methods to
enable/disable a condition, suppress/unsuppress an alarm, remove/place it
into service, and silence its audible/visible indicator, so that I can
manage alarm lifecycle the same way I already can for acknowledge/confirm/
shelve.

**Why this priority**: The underlying state variables mostly already exist
(`SuppressedState` wired to `SuppressedOrShelved`; `get_enabled`/`set_enabled`
and `get_out_of_service`/`set_out_of_service` on the condition state machine,
`async-opcua-server/src/alarms/state_machine.rs:455-462,574-579`) but are
unreachable from a client because no Method is registered — a small amount
of wiring unlocks real day-to-day alarm-management usability, and
`SuppressedState`/`OutOfServiceState` currently have zero test coverage.

**Independent Test**: Call each Method (`Enable`, `Disable`, `Suppress`,
`Unsuppress`, `RemoveFromService`, `PlaceInService`, `Silence`) against a
live condition/alarm instance and verify the corresponding state variable
and effective state change, with correct Bad-status responses for invalid
preconditions (e.g. calling `Suppress` on an alarm that does not expose
`SuppressedState` returns `Bad_MethodInvalid` per OPC-10000-9 §5.8.8, not a
silent no-op).

**Acceptance Scenarios**:

1. **Given** an enabled condition, **When** a client calls `Disable`
   (OPC-10000-9 §5.5.4, `ConditionType_Disable`), **Then**
   `EnabledState.Id` becomes `false` and the condition stops generating new
   alarm activity.
2. **Given** a disabled condition, **When** a client calls `Enable`
   (§5.5.5, `ConditionType_Enable`), **Then** `EnabledState.Id` becomes
   `true` and monitoring resumes.
3. **Given** an active alarm exposing `SuppressedState`, **When** a client
   calls `Suppress` (§5.8.8, `AlarmConditionType_Suppress`), **Then**
   `SuppressedState.Id` becomes `true` and the alarm's effective state
   reflects suppression (consistent with the existing shelving-adjacent
   `SuppressedOrShelved` wiring).
4. **Given** a suppressed alarm, **When** a client calls `Unsuppress`
   (§5.8.10, `AlarmConditionType_Unsuppress`), **Then** `SuppressedState.Id`
   becomes `false`.
5. **Given** an active alarm exposing `OutOfServiceState`, **When** a client
   calls `RemoveFromService` (§5.8.12,
   `AlarmConditionType_RemoveFromService`), **Then** `OutOfServiceState.Id`
   becomes `true`.
6. **Given** an out-of-service alarm, **When** a client calls
   `PlaceInService` (§5.8.14, `AlarmConditionType_PlaceInService`), **Then**
   `OutOfServiceState.Id` becomes `false`.
7. **Given** an active, audible alarm exposing `SilenceState`, **When** a
   client calls `Silence` (§5.8.7, `AlarmConditionType_Silence`), **Then**
   `SilenceState.Id` changes accordingly without affecting `ActiveState`/
   `AckedState`.

### User Story 3 - A&C Audit Events (Priority: P3)

As a security/compliance auditor, when an operator changes a condition's
state (comment, acknowledge, confirm, enable/disable, shelve), I want a
corresponding audit event recorded, so that alarm-management actions are
traceable the same way session, write, and method-call actions already are
in this server.

**Why this priority**: Smaller in CU count but reuses this codebase's
already-complete audit-event infrastructure (session/write/method/cert/cancel
audit events) rather than inventing anything new — the lowest-risk, most
pattern-consistent of the four themes. The codebase currently has an explicit
deferred marker (`async-opcua-server/src/alarms/methods.rs:201`, "Emit
AuditConditionCommentEventType when audit event support is added") that is
now stale, since the audit subsystem it was waiting on is complete.

**Independent Test**: Perform an AddComment call on a condition with server
auditing enabled; verify an `AuditConditionCommentEventType`-derived event is
emitted with the correct condition reference, comment text, and outcome
status, following the same emission pattern as existing audit events (e.g.
`AuditWriteUpdateEventType`, `AuditUpdateMethodEventType`).

**Acceptance Scenarios**:

1. **Given** server auditing is enabled, **When** a client calls
   `AddComment` on a condition, **Then** an audit event derived from
   `AuditConditionEventType` (OPC-10000-9 §5.10.2) is emitted recording the
   condition, the comment, and success/failure status.
2. **Given** server auditing is enabled, **When** a client
   acknowledges/confirms a condition, **Then** a corresponding audit event is
   emitted, consistent with the AddComment case.

### User Story 4 - Missing Alarm Subtypes (Priority: P4)

As a server implementer modeling a real plant, I want to instantiate
Level, Deviation, and RateOfChange alarms (in addition to the existing Limit
and Discrete kinds), plus SystemOffNormal, CertificateExpiration, and
Discrepancy alarms, and configure their OnDelay/OffDelay, ReAlarm, and
audible-sound properties, so that I can accurately represent the alarm types
my process actually needs instead of approximating everything as a generic
limit or discrete alarm.

**Why this priority**: Lowest CU-count and most varied/novel work — several
genuinely new alarm-kind implementations rather than one shared mechanism.
Ordered last because it has the least cross-CU leverage per unit of effort.

**Independent Test**: Instantiate one alarm of each new kind
(Level/Deviation/RateOfChange, in both exclusive and non-exclusive
variants where applicable; SystemOffNormal; CertificateExpiration;
Discrepancy), verify each reports the correct, specific `TypeDefinition`
NodeId (not a generic base type), and drive each through its
activation/deactivation semantics. Separately, verify `OnDelay`/`OffDelay`
timing properties, `ReAlarmTime`/`ReAlarmRepeatCount`, and `AudibleSound`/
audible-enabled behavior on an existing alarm instance.

**Acceptance Scenarios**:

1. **Given** a configured `ExclusiveLevelAlarmType` instance, **When** its
   monitored value crosses a level threshold, **Then** it activates and its
   `TypeDefinition` reports `ExclusiveLevelAlarmType` (OPC-10000-9 §5.8.21.3),
   not the generic `ExclusiveLimitAlarmType` the existing Limit-alarm code
   currently sets for all limit-family alarms.
2. **Given** a configured `NonExclusiveDeviationAlarmType` instance tracking
   a setpoint, **When** the monitored value deviates from setpoint beyond a
   configured threshold, **Then** it activates (OPC-10000-9 §5.8.22.2).
3. **Given** a configured `ExclusiveRateOfChangeAlarmType` instance, **When**
   the monitored value's rate of change exceeds a configured threshold,
   **Then** it activates (OPC-10000-9 §5.8.23.3).
4. **Given** a `SystemOffNormalAlarmType` instance representing a monitored
   system/subsystem, **When** that system enters an off-normal state,
   **Then** the alarm activates (OPC-10000-9 §5.8.24.3).
5. **Given** a `CertificateExpirationAlarmType` instance monitoring an
   application/trust certificate, **When** the certificate approaches its
   expiration date, **Then** the alarm activates (OPC-10000-9 §5.8.24.7).
6. **Given** a `DiscrepancyAlarmType` instance comparing a primary and
   secondary value, **When** the two diverge beyond tolerance, **Then** the
   alarm activates (OPC-10000-9 §5.8.25).
7. **Given** an alarm configured with a non-zero `OnDelay`, **When** its
   trigger condition becomes true, **Then** activation is delayed by that
   interval rather than firing immediately.
8. **Given** an alarm with `ReAlarmTime` configured, **When** it remains
   active and unacknowledged past the re-alarm interval, **Then** it
   re-notifies and increments `ReAlarmRepeatCount`; **When** it is
   subsequently acknowledged, **Then** `ReAlarmRepeatCount` resets.

### Edge Cases

- **Shelved alarm's EffectiveTransitionTime vs. TransitionTime**: when an
  alarm is shelved, its underlying `ActiveState` may not change, but its
  *effective* state does (a shelved active alarm is not "effectively"
  active for notification purposes) — `EffectiveTransitionTime` must track
  the effective-state change even when the raw sub-state's own
  `TransitionTime` does not update.
- **Method call on a condition with no source**: Enable/Disable/Suppress/
  Silence calls on a condition not attached to a live monitored value must
  fail with an appropriate Bad status, not panic, consistent with existing
  Acknowledge/Confirm error handling.
- **Silence on an already-silenced alarm**: must be idempotent (no error),
  consistent with how other already-in-target-state Method calls are
  handled elsewhere in this alarms module.
- **OnDelay/OffDelay combined with re-notification**: an alarm delayed by
  OnDelay must not start its ReAlarm timer until it actually activates, not
  from when its trigger condition first became true.
- **Auditing disabled**: A&C actions must still function correctly with
  server auditing disabled (the default) — audit emission is additive, never
  a precondition for the alarm action itself to succeed.

## Requirements *(mandatory)*

### Functional Requirements

#### Sub-State Transition Timestamps and Display Names

- **FR-001**: Every `TwoStateVariableType`-derived sub-state exposed by a
  condition (`EnabledState`, `ActiveState`, `AckedState`, `ConfirmedState`,
  `SuppressedState`, and shelving-related states) MUST expose a
  `TransitionTime` property reflecting when that specific sub-state was last
  entered (OPC-10000-9 §5.2).
- **FR-002**: Every condition's overall alarm state MUST expose an
  `EffectiveTransitionTime` reflecting when the effective (shelving- and
  suppression-adjusted) state last changed, which may differ from any single
  sub-state's own `TransitionTime`.
- **FR-003**: Every condition's overall alarm state MUST expose an
  `EffectiveDisplayName` returning a human-readable, locale-aware description
  of the current effective state.
- **FR-004**: `LimitAlarmType` instances MUST expose `TransitionTime` on each
  configured limit state (Low, LowLow, High, HighHigh) reflecting when the
  monitored value last crossed into that limit.

#### Lifecycle Methods

- **FR-005**: The server MUST expose `Enable` (§5.5.5) and `Disable`
  (§5.5.4) Methods on `ConditionType` that set a condition's `EnabledState`.
- **FR-006**: `Disable` MUST suspend new alarm activity for the condition
  while disabled.
- **FR-007**: The server MUST expose `Suppress`/`Suppress2` (§5.8.8/§5.8.9)
  and `Unsuppress`/`Unsuppress2` (§5.8.10/§5.8.11) Methods on
  `AlarmConditionType` that set an alarm's `SuppressedState`, integrating
  with the existing effective-state (shelving/suppression) computation, and
  reject calls (`Bad_MethodInvalid`) on instances that do not expose
  `SuppressedState`.
- **FR-007a**: The server MUST expose `RemoveFromService`/
  `RemoveFromService2` (§5.8.12/§5.8.13) and `PlaceInService`/
  `PlaceInService2` (§5.8.14/§5.8.15) Methods on `AlarmConditionType` that
  set an alarm's `OutOfServiceState`, and reject calls on instances that do
  not expose it.
- **FR-008**: The server MUST expose a `Silence` Method (§5.8.7) that
  changes `SilenceState` for alarms exposing it.
- **FR-009**: `SuppressedState` and `OutOfServiceState` behavior (already
  present in the state machine) MUST be covered by tests; they currently
  have none.

#### Auditing

- **FR-010**: When server auditing is enabled, `Enable`/`Disable` calls MUST
  emit `AuditConditionEnableEventType` (§5.10.2); `Silence` calls MUST emit
  `AuditConditionSilenceEventType` (§5.10.10); `RemoveFromService`/
  `PlaceInService` calls MUST emit `AuditConditionOutOfServiceEventType`
  (§5.10.12); and, at minimum, `AddComment` MUST emit
  `AuditConditionCommentEventType`, closing the deferred marker at
  `async-opcua-server/src/alarms/methods.rs:201` — all following this
  codebase's existing audit-event emission pattern
  (`async-opcua-server/src/session/audit.rs`,
  `node_manager/audit_events.rs`).
- **FR-011**: Audit emission MUST NOT be a precondition for the underlying
  alarm action's success — auditing failures or auditing-disabled MUST NOT
  block or alter the Method call's own outcome.

#### Alarm Subtypes

- **FR-012**: The server MUST support instantiating Level alarms
  (`ExclusiveLevelAlarmType`/`NonExclusiveLevelAlarmType`,
  OPC-10000-9 §5.8.21) with the correct specific `TypeDefinition`, distinct
  from the generic Limit alarm types the existing code currently assigns to
  all limit-family alarms.
- **FR-013**: The server MUST support instantiating Deviation alarms
  (`ExclusiveDeviationAlarmType`/`NonExclusiveDeviationAlarmType`,
  OPC-10000-9 §5.8.22) that activate based on deviation from a configured
  setpoint.
- **FR-014**: The server MUST support instantiating RateOfChange alarms
  (`ExclusiveRateOfChangeAlarmType`/`NonExclusiveRateOfChangeAlarmType`,
  OPC-10000-9 §5.8.23) that activate based on the monitored value's rate of
  change.
- **FR-015**: The server MUST support instantiating `SystemOffNormalAlarmType`
  (OPC-10000-9 §5.8.24.3) instances.
- **FR-016**: The server MUST support instantiating
  `CertificateExpirationAlarmType` (OPC-10000-9 §5.8.24.7) instances.
- **FR-017**: The server MUST support instantiating `DiscrepancyAlarmType`
  (OPC-10000-9 §5.8.25) instances.
- **FR-018**: Alarms MUST support configurable `OnDelay`/`OffDelay` timing
  properties that delay activation/deactivation by a configured interval.
- **FR-019**: Alarms MUST support a configurable `ReAlarmTime` interval
  (OPC-10000-9 §5.8.2) for periodic re-notification while active and
  unacknowledged, and MUST maintain `ReAlarmRepeatCount` (Int16,
  server-maintained, not client-configured) as a running count of
  re-notifications sent for the current active+unacknowledged span, reset
  when the alarm is acknowledged or becomes inactive.
- **FR-020**: Alarms supporting audible indication MUST expose an
  `AudibleSound`/audible-enabled property.

### Key Entities

- **TwoStateVariableType sub-state**: A condition's `EnabledState`,
  `ActiveState`, `AckedState`, `ConfirmedState`, `SuppressedState`, or
  shelving state — each gains `TransitionTime`; the condition as a whole
  gains `EffectiveTransitionTime`/`EffectiveDisplayName`.
- **Level/Deviation/RateOfChange alarm**: New alarm kinds alongside the
  existing Limit and Discrete kinds, each with exclusive/non-exclusive
  variants where applicable.
- **AuditConditionEvent family**: Four new audit event types —
  `AuditConditionCommentEventType`, `AuditConditionEnableEventType`,
  `AuditConditionSilenceEventType`, `AuditConditionOutOfServiceEventType` —
  each emitted for its corresponding Method call, following the existing
  audit-event pattern.
- **OnDelay/OffDelay/ReAlarm/AudibleSound**: New configurable timing and
  notification properties on existing and new alarm instances.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 58 TransitionTime/EffectiveTransitionTime/
  EffectiveDisplayName CUs (5510-5567) move from `gap` to `implemented` in
  `specs/conformance-tester/CU-COVERAGE.md` after regeneration, each backed
  by a passing test.
- **SC-002**: Enable, Disable, Suppress, Unsuppress, RemoveFromService,
  PlaceInService, and Silence are callable Methods with test coverage, and
  `SuppressedState`/`OutOfServiceState` gain their first tests.
- **SC-003**: `AuditConditionCommentEventType`, `AuditConditionEnableEventType`,
  `AuditConditionSilenceEventType`, and `AuditConditionOutOfServiceEventType`
  are each emitted and tested for their respective Method, closing the stale
  `ponytail` marker in `alarms/methods.rs:201`.
- **SC-004**: Level, Deviation, RateOfChange, SystemOffNormal,
  CertificateExpiration, and Discrepancy alarms are each instantiable with
  the correct specific `TypeDefinition` and pass an activation/deactivation
  test.
- **SC-005**: No existing A&C integration tests
  (`async-opcua/tests/integration/alarms.rs`) regress.
- **SC-006**: The A&C subsystem's confirmed-gap count in the audit ledger
  drops from 98 to a small residual (Alarm Groups, AlarmMetrics, COM
  wrapper, and Previous-Instances items explicitly deferred, not silently
  dropped).

## Assumptions

- Existing Limit-alarm code assigning the generic
  `ExclusiveLimitAlarmType`/`NonExclusiveLimitAlarmType` NodeId to all
  limit-family alarms (including what should be Level alarms) is a
  pre-existing simplification this feature corrects for the Level case
  specifically, without disturbing already-working generic Limit-alarm
  behavior for cases that are genuinely generic limit alarms.
- Deviation and RateOfChange alarms require a "setpoint" or "rate" input
  concept not previously modeled for Discrete/Limit alarms; this feature
  adds the minimal configuration surface needed (a configured setpoint
  node/value, a configured rate-of-change window) rather than a general
  computed-alarm-input framework.
- Audit coverage in this feature targets the four Method-call audit event
  types named in FR-010/SC-003 (Comment, Enable, Silence, OutOfService);
  full parity with every A&C audit event subtype in Part 9 §5.10 (e.g.
  Acknowledge/Confirm/Shelve auditing) is not required for this feature to
  be considered complete, since those Methods were out of scope for this
  audit pass and can follow the same established pattern in a later one.
- Alarm Groups, AlarmMetrics, the COM A&E wrapper facet, and A&C Previous
  Instances are out of scope, per the audit's own prioritization and the
  user's explicit direction to focus on the four themes above.
