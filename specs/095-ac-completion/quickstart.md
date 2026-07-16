# Quickstart: Verifying Alarms & Conditions Completion

## US1 — Sub-state transition timestamps

1. Run a server instance exposing an `AlarmConditionType` instance backed
   by a monitored source value (see `async-opcua/tests/integration/alarms.rs`
   for existing fixture setup).
2. Write a value that crosses the alarm's active threshold.
3. Read `<ConditionId>/EnabledState.TransitionTime`,
   `ActiveState.TransitionTime`, `ActiveState.EffectiveTransitionTime`, and
   `EffectiveDisplayName` — confirm each reflects the transition.

## US2 — Lifecycle Methods

1. Call `Disable` (`ConditionType_Disable`) on an enabled condition; read
   `EnabledState.Id` — expect `false`.
2. Call `Enable`; expect `EnabledState.Id` back to `true`.
3. Call `Suppress`/`Unsuppress` on an alarm exposing `SuppressedState`;
   confirm `SuppressedState.Id` toggles and `SuppressedOrShelved` reflects
   it.
4. Call `RemoveFromService`/`PlaceInService` on an alarm exposing
   `OutOfServiceState`; confirm `OutOfServiceState.Id` toggles.
5. Call `Silence` on an alarm exposing `SilenceState`; confirm
   `SilenceState.Id` toggles without affecting `ActiveState`/`AckedState`.
6. Call `Suppress` on an alarm instance that does NOT expose
   `SuppressedState`; expect `Bad_MethodInvalid`, not a panic or silent
   success.

## US3 — Auditing

1. Enable server auditing (existing config flag used by other audit
   features).
2. Call `AddComment` on a condition; subscribe to `AuditConditionCommentEventType`
   or its supertype; confirm an event arrives with the condition and comment.
3. Repeat for `Enable`/`Disable` (`AuditConditionEnableEventType`),
   `Silence` (`AuditConditionSilenceEventType`), and
   `RemoveFromService`/`PlaceInService` (`AuditConditionOutOfServiceEventType`).
4. Repeat step 2 with auditing disabled; confirm `AddComment` still
   succeeds (no audit event required, action unaffected).

## US4 — Alarm subtypes

1. Instantiate an `ExclusiveLevelAlarmType` alarm; read its `TypeDefinition`
   attribute; expect `ExclusiveLevelAlarmType`, not `ExclusiveLimitAlarmType`.
2. Instantiate a `NonExclusiveDeviationAlarmType` alarm against a setpoint
   node; write a deviating value; confirm activation.
3. Instantiate an `ExclusiveRateOfChangeAlarmType` alarm; write a rapidly
   changing sequence of values; confirm activation.
4. Instantiate `SystemOffNormalAlarmType`, `CertificateExpirationAlarmType`,
   and `DiscrepancyAlarmType` alarms; drive each through its
   activation/deactivation trigger; confirm correct `TypeDefinition` and
   state changes.
5. Configure `OnDelay` on an alarm; trigger its activation condition;
   confirm the alarm does not activate until the delay elapses.
6. Configure `ReAlarmTime`; leave an alarm active and unacknowledged past
   the interval; confirm re-notification and that `ReAlarmRepeatCount`
   increments; acknowledge it and confirm `ReAlarmRepeatCount` resets.

## Regression check

Run the full existing suite: `cargo test -p async-opcua --test integration -- alarms`
(or the workspace's standard alarms test invocation) — all pre-existing
tests in `async-opcua/tests/integration/alarms.rs` must still pass (SC-005).
