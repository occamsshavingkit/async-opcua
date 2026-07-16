# Phase 1 Data Model: Alarms & Conditions Completion

## TwoStateVariable transition record (US1)

**Correction from initial grounding**: this module does NOT instantiate the
full generated 1.05 nodeset per condition. `ConditionStateMachine::
create_in_address_space` (`state_machine.rs:97-377`) hand-builds each
instance's nodes with dynamically-minted namespace-2 string NodeIds (e.g.
`Alarm_<device>_<type>_EnabledState`), using generated `ObjectTypeId`/
`VariableTypeId` constants only for `has_type_definition` (the type-level
reference), exactly as `unshelve_time_id`/`shelving_current_state_id`
already do for `ShelvingState`. There is no generated *instance* NodeId
(e.g. `AlarmConditionType_EnabledState_TransitionTime = 9016`) to write to —
that constant is the abstract type's ModellingRule template declaration,
not a concrete per-alarm address.

So: `TransitionTime`/`EffectiveTransitionTime`/`EffectiveDisplayName` are
each a **new** dynamically-minted child Variable node (`DataTypeId::DateTime`
for the two time properties, `DataTypeId::LocalizedText` for
`EffectiveDisplayName`), `has_type_definition(VariableTypeId::PropertyType)`,
attached via `HasProperty` (ref `NodeId::new(0, 46)`) to the owning
sub-state's own NodeId — mirroring exactly how `unshelve_time_id` is
attached to `shelving_state_id` (state_machine.rs:340-354). `ConditionState
Machine` gains new NodeId fields (e.g. `enabled_state_transition_time_id`)
alongside the existing `enabled_state_id` etc., created in
`create_in_address_space` and written by a new shared helper called from
each `set_*` method:

| Property | Attached to | Written when |
|---|---|---|
| `TransitionTime` | each sub-state's own NodeId (`enabled_state_id`, `active_state_id`, `acked_state_id`, `confirmed_state_id`, `suppressed_state_id`, `out_of_service_state_id`, `shelving_current_state_id`, new `silence_state_id`) | that specific sub-state's `Id` changes |
| `EffectiveTransitionTime` | `active_state_id` (the sub-state with sub-state-machines per §5.2) | the condition's overall effective (shelving/suppression-adjusted) state changes |
| `EffectiveDisplayName` | `active_state_id` | same trigger as `EffectiveTransitionTime` |

`LimitAlarmType` limit states (Low/LowLow/High/HighHigh) get the same new
child-node treatment, attached to each limit state's own NodeId in
`limit.rs`, keyed off `LimitEvaluator`'s existing threshold-crossing
detection.

## Alarm kind enums (US4)

Extends the existing `discrete.rs::DiscreteAlarmKind` pattern:

```text
enum LimitAlarmKind { Limit, Level }          // limit.rs — parameterizes
                                                // create_exclusive/non_exclusive_in_address_space
                                                // TypeDefinition NodeId choice only;
                                                // evaluation logic unchanged

struct DeviationAlarm { setpoint_node: NodeId, deviation_evaluator }  // new, limit.rs or sibling module
struct RateOfChangeAlarm { rate_window: Duration, rate_evaluator }    // new, limit.rs or sibling module

enum SystemAlarmKind { SystemOffNormal, CertificateExpiration, Discrepancy }  // sibling to
                                                                                // DiscreteAlarmKind,
                                                                                // discrete.rs or new file
```

Each new kind maps to its `ObjectTypeId` constant (already generated, e.g.
`ObjectTypeId::ExclusiveLevelAlarmType`, `ObjectTypeId::
SystemOffNormalAlarmType`) via a `type_id()` method mirroring
`DiscreteAlarmKind::type_id()`.

## Per-instance delay/re-alarm timing state (US4)

Extension of the existing per-instance evaluation state already tracked by
`source_monitor.rs`/the alarm structs themselves:

| Field | Type | Meaning |
|---|---|---|
| `on_delay` | `Duration` | configured delay before activation fires after trigger condition becomes true |
| `off_delay` | `Duration` | configured delay before deactivation fires |
| `realarm_time` | `Duration` | interval between re-notifications while active+unacknowledged (`ReAlarmTime` Property, OPC-10000-9 §5.8.2) |
| `realarm_repeat_count` | `i16` (runtime, server-maintained) | `ReAlarmRepeatCount` (Int16 `BaseDataVariableType`, not a Property) — counts how many times the alarm has been re-alarmed so far; per §5.8.2 it is an output counter, not a client-configured maximum. Re-alarming continues at `ReAlarmTime` intervals for as long as the alarm remains active and unacknowledged; there is no base-spec "stop after N" limit. |
| `audible_enabled` | `bool` | maps to `AudibleSound`/`SilenceState` interaction |

## Audit event constructors (US3)

New `ServerAuditEvent` constructors in `session/audit.rs`, alongside the
existing `outcome`/`method_call`/`write_update`:

| Constructor | Event type NodeId | Fired by |
|---|---|---|
| `condition_comment(...)` | `AuditConditionCommentEventType` | `AddComment` handler (closes `methods.rs:201`) |
| `condition_enable(...)` | `AuditConditionEnableEventType` | `Enable`/`Disable` handlers |
| `condition_silence(...)` | `AuditConditionSilenceEventType` | `Silence` handler |
| `condition_out_of_service(...)` | `AuditConditionOutOfServiceEventType` | `RemoveFromService`/`PlaceInService` handlers |

Each carries at minimum: `condition_id` (the `ConditionId` the Method was
called against), the outcome `StatusCode`, and — for `condition_comment` —
the `Comment` argument text, mirroring the existing `dispatch_write_audit`
shape (old/new value equivalents where applicable).
