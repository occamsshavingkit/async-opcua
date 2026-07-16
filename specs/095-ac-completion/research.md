# Phase 0 Research: Alarms & Conditions Completion

No `[NEEDS CLARIFICATION]` markers were left in spec.md — the audit
document and current codebase already resolved every open question the
feature description raised. This file records the technical decisions made
while grounding the plan, for traceability.

## Decision: US1 writes existing generated NodeIds, does not create new address-space structure

**Decision**: `TransitionTime`, `EffectiveTransitionTime`, and
`EffectiveDisplayName` are populated by writing to Variable NodeIds that
already exist in the imported 1.05 nodeset (e.g.
`AlarmConditionType_EnabledState_TransitionTime = 9016`,
confirmed in `async-opcua-types/src/generated/node_ids.rs`). No new nodes,
no address-space schema change.

**Rationale**: The generated-address-space import already carries every
optional Part 9 property defined on `TwoStateVariableType`; the audit's
"zero implementation" finding meant no server code ever *wrote* to these
nodes, not that the nodes were absent. Confirming this before planning
avoids inventing unnecessary new address-space wiring.

**Alternatives considered**: Manually constructing these properties per
sub-state instance (as `limit.rs`/`discrete.rs` do for their own custom
nodes) was rejected — unnecessary since the generic nodeset import already
provides the node structure; only the value-write path is missing.

## Decision: Level-alarm fix follows the existing `DiscreteAlarmKind` pattern, not a new abstraction

**Decision**: `LimitAlarm::create_exclusive_in_address_space`/
`create_non_exclusive_in_address_space` (`limit.rs:356,438`) take a kind
parameter (mirroring `discrete.rs`'s `DiscreteAlarmKind::{OffNormal, Trip}`
enum with a `type_id()` method, `discrete.rs:19-23,172-176`) so that
Level alarms get `ObjectTypeId::ExclusiveLevelAlarmType`/
`NonExclusiveLevelAlarmType` instead of the generic Limit type, without
touching the evaluation logic shared by both.

**Rationale**: This is the smallest correct fix for the audit's specific
finding (right evaluation logic, wrong `TypeDefinition` NodeId) and reuses
an established, already-tested pattern in the same module rather than
inventing a new one (Constitution Principle II).

**Alternatives considered**: A generic `AlarmKind` trait spanning
Limit/Level/Deviation/RateOfChange was considered and rejected as premature
abstraction — Deviation and RateOfChange need materially different
evaluation inputs (setpoint, rate window) than Limit/Level share, so forcing
them into one trait would either bloat the trait or under-specify it. Each
gets its own evaluator struct, consistent with `LimitEvaluator` today.

## Decision: Audit events extend the existing `ServerAuditEvent`/`dispatch_*_audit` pattern

**Decision**: New `dispatch_condition_comment_audit`/
`dispatch_condition_enable_audit`/`dispatch_condition_silence_audit`/
`dispatch_condition_out_of_service_audit` functions in
`session/audit.rs`, each building a `ServerAuditEvent` (via a new or
existing constructor) and calling the existing `dispatch_audit_event`
(`audit.rs:855`), mirroring `dispatch_method_audit`/`dispatch_write_audit`.

**Rationale**: `session/audit.rs` already has a working, tested emission
pipeline (`ServerAuditEvent` → `dispatch_audit_event` →
`subscriptions.notify_events`) used for method/write/certificate audits;
Part 9's `AuditConditionEventType` family is structurally an
`AuditUpdateMethodEventType` subtype (condition-scoped method-call
auditing), so it fits the existing `dispatch_method_audit`-style shape
rather than needing a new emission mechanism.

**Alternatives considered**: A parallel A&C-specific audit pipeline was
rejected — it would duplicate `dispatch_audit_event`/`notify_events` wiring
for no benefit and violate "Do It Right Once."

## Decision: OnDelay/OffDelay/ReAlarm are per-alarm-instance timing state, not new infrastructure

**Decision**: `OnDelay`/`OffDelay` are implemented as configured delay
durations checked against the alarm's own existing tick/evaluation path
(the same path `LimitAlarm`/`DiscreteAlarmKind` already use to detect state
crossings); `ReAlarmTime`/`ReAlarmRepeatCount` reuse the same per-instance
timing state, tracking elapsed-since-last-notification and a repeat
counter.

**Rationale**: This module already has a tick/evaluation path per alarm
instance (`source_monitor.rs`); OnDelay/OffDelay/ReAlarm are refinements of
*when* that path's activation/notification fires, not a new subsystem.

**Alternatives considered**: A separate central timer/scheduler for
delayed/re-alarming alarms was rejected as unnecessary complexity — the
existing per-instance evaluation path already runs on every relevant value
change and can carry the extra timing state without a new global loop.
