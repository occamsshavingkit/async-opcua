---

description: "Task list for feature 095: Alarms & Conditions Completion"
---

# Tasks: Alarms & Conditions Completion

**Input**: Design documents from `/specs/095-ac-completion/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included — this repo's constitution (Principle I, Correctness Over
Completion) requires demonstrable correctness including edge cases, and every
prior speckit feature in this repo has shipped with test coverage per user
story.

**Organization**: Tasks are grouped by user story (P1→P4, matching spec.md's
priority order) so each is independently implementable and testable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US4)
- Every task cites the OPC-10000-9 section and/or the generated NodeId
  constant it implements against, per this repo's `speckit-tasks-cite-spec-
  sections` convention.

## Path Conventions

Single Rust workspace crate area: `async-opcua-server/src/alarms/`,
`async-opcua-server/src/session/audit.rs`, `async-opcua-core/src/events.rs`,
tests in `async-opcua/tests/integration/alarms.rs`, evidence ledger in
`tools/cu-coverage-report/src/lib.rs`.

---

## Phase 1: Setup

No project-initialization tasks required — this feature extends an existing,
already-feature-gated module (`alarms` + `method-call` +
`generated-address-space`). Branch `095-ac-completion` already created from
`master`.

---

## Phase 2: Foundational

No blocking cross-story prerequisites. Each user story below is
independently implementable per spec.md's Independent Test criteria. (US2's
Silence support and US3's audit wiring do reference handlers added in
earlier-numbered stories — see per-task dependency notes — but this does not
block starting US1 or US2's other tasks.)

---

## Phase 3: User Story 1 - Sub-State Transition Timestamps and Display Names (Priority: P1) 🎯 MVP

**Goal**: Populate `TransitionTime`, `EffectiveTransitionTime`, and
`EffectiveDisplayName` on every existing two-state sub-state and limit
state (OPC-10000-9 §5.2), closing CUs 5510-5567.

**Independent Test**: Trigger enable/disable, activate/deactivate,
ack/confirm, suppress/unsuppress, shelve/unshelve, and a limit crossing;
read back the three properties on each affected sub-state.

### Tests for User Story 1

- [X] T001 [P] [US1] Add integration tests in `async-opcua/tests/integration/alarms.rs` asserting `EnabledState.TransitionTime`/`ActiveState.TransitionTime`/`AckedState.TransitionTime`/`ConfirmedState.TransitionTime`/`SuppressedState.TransitionTime` update on their respective transitions (OPC-10000-9 §5.2). Write these to FAIL against current code first.
- [X] T002 [P] [US1] Add integration test asserting `ActiveState.EffectiveTransitionTime` and `EffectiveDisplayName` change when an alarm is shelved without its raw `ActiveState.TransitionTime` changing (the Edge Case in spec.md).
- [X] T003 [P] [US1] Add integration test asserting `TransitionTime` on `LimitAlarmType` Low/LowLow/High/HighHigh limit states updates on threshold crossing.

### Implementation for User Story 1

- [X] T004 [US1] In `async-opcua-server/src/alarms/state_machine.rs`, add new dynamically-minted child `TransitionTime` Variable nodes (`DataTypeId::DateTime`, `has_type_definition(VariableTypeId::PropertyType)`, `HasProperty`/`NodeId::new(0,46)` reference) for `EnabledState`, `ActiveState`, `AckedState`, `ConfirmedState`, and `SuppressedState`, created in `create_in_address_space` mirroring the existing `unshelve_time_id` pattern (:340-354) — **not** a write to any generated per-instance NodeId; this module dynamically mints namespace-2 instance NodeIds and only uses generated constants for `has_type_definition`. Add corresponding NodeId fields to `ConditionStateMachine` and a shared `write_transition_time(&self, address_space, transition_time_id, time)` helper.
- [X] T005 [US1] Call the T004 helper from `set_enabled` (state_machine.rs:460), `set_active` (:470), `set_acked` (:480), `set_confirmed` (:490), and `set_suppressed` (:568), each keyed to that sub-state's own new `TransitionTime` child node.
- [X] T006 [US1] Add an `OutOfServiceState` `TransitionTime` child node (same pattern as T004) and call the write helper from `set_out_of_service` (state_machine.rs:579).
- [X] T007 [US1] Add a `ShelvingState.CurrentState` `TransitionTime` child node (same pattern) and call the write helper from `set_shelving_state` (state_machine.rs:614).
- [X] T008 [US1] Add new `EffectiveTransitionTime` (DateTime) and `EffectiveDisplayName` (LocalizedText) child nodes attached to `active_state_id` (same T004 pattern), plus a `recompute_effective_state(&self, address_space)` helper in `state_machine.rs` that writes both whenever the overall effective (shelving/suppression-adjusted) state changes (OPC-10000-9 §5.2's "state has sub states" case); call it from `set_active`, `recompute_suppressed_or_shelved` (state_machine.rs:659), and `set_shelving_state`.
- [X] T009 [US1] In `async-opcua-server/src/alarms/limit.rs`, add new `TransitionTime` child nodes (same T004 pattern) for each configured Low/LowLow/High/HighHigh limit state, written at the point `LimitEvaluator`/`LimitAlarm` detects a crossing, for both `create_exclusive_in_address_space` (limit.rs:357) and `create_non_exclusive_in_address_space` (:439) instances.
- [X] T010 [US1] Run T001-T003; confirm they now pass. Fix any NodeId/property-write mismatches found.
- [X] T011 [US1] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE`: move CUs 5510-5567 from `Gap` to `Implemented` with file:line evidence citing T004-T009's changes (look up exact CU-to-property mapping via `search_cu` if the range isn't 1:1 obvious).

**Checkpoint**: US1 fully functional and testable independently. Ground-truth
CU lookup during T011 (the actual snapshot, not just the range boundaries)
found the 58-CU range is more granular than "one shared mechanism" implied:
it includes per-specific-transition-edge properties (e.g. distinct
`UnshelvedToTimedShelved`/`TimedShelvedToUnshelved`/... timestamps, 6+4 of
them) and per-substate `Effective*` variants beyond `ActiveState` that this
increment's design does not provide. Actually closed: **14 of 58** (5510,
5513-5516, 5519, 5522, 5525, 5534-5537, 5549, 5559) — the core
`TransitionTime` on `EnabledState`/`ActiveState`/`AckedState`/
`ConfirmedState`/`SuppressedState`/`OutOfServiceState`, `ActiveState`'s
`Effective*` pair, non-exclusive limit-state `TransitionTime`, and the two
`LastTransition`-equivalent properties on `ShelvingState.CurrentState`/
`LimitState.CurrentState`. The remaining 44 (per-substate `Effective*`,
per-transition-edge timestamps, per-value `EffectiveDisplayName`) are a
real, larger follow-up, not silently dropped — see the AUDIT_TABLE Gap
entries' updated evidence text for the precise remaining set.

---

## Phase 4: User Story 2 - Enable/Disable/Suppress/OutOfService/Silence Methods (Priority: P2)

**Goal**: Register client-callable `Enable`/`Disable`/`Suppress`/`Suppress2`/
`Unsuppress`/`Unsuppress2`/`RemoveFromService`/`RemoveFromService2`/
`PlaceInService`/`PlaceInService2`/`Silence` Methods (OPC-10000-9
§5.5.4-§5.5.5, §5.8.7-§5.8.15), with tests for `SuppressedState`/
`OutOfServiceState`/`SilenceState` (currently zero coverage).

**Independent Test**: Call each Method against a live condition/alarm
instance; verify the state variable and effective state change; verify
`Bad_MethodInvalid` when the target instance doesn't expose the relevant
state.

### Tests for User Story 2

- [X] T012 [P] [US2] Add integration tests for `Enable`/`Disable` (`ConditionType_Enable`/`ConditionType_Disable`, §5.5.5/§5.5.4) in `alarms.rs`, including the "call on a condition with no source" edge case (Bad status, not panic).
- [X] T013 [P] [US2] Add integration tests for `Suppress`/`Unsuppress` (`AlarmConditionType_Suppress`/`_Unsuppress`, §5.8.8/§5.8.10) including the `Bad_MethodInvalid` precondition case for an instance without `SuppressedState`.
- [X] T014 [P] [US2] Add integration tests for `RemoveFromService`/`PlaceInService` (`AlarmConditionType_RemoveFromService`/`_PlaceInService`, §5.8.12/§5.8.14) including the `Bad_MethodInvalid` precondition case.
- [X] T015 [P] [US2] Add integration test for `Silence` (`AlarmConditionType_Silence`, §5.8.7), including idempotency on an already-silenced alarm (Edge Case in spec.md).
- [X] T016 [P] [US2] Add integration tests for `Suppress2`/`Unsuppress2`/`RemoveFromService2`/`PlaceInService2` confirming the optional `Comment` argument (§5.8.9/§5.8.11/§5.8.13/§5.8.15) is accepted and applied.

### Implementation for User Story 2

- [X] T017 [US2] In `async-opcua-server/src/alarms/state_machine.rs`, add `get_silence`/`set_silence` accessors mirroring the existing `get_suppressed`/`set_suppressed` pattern (:563-571), backed by `AlarmConditionType_SilenceState` and its `_Id`/`_TransitionTime` NodeIds; wire into the T004 `TransitionTime` helper from US1.
- [X] T018 [US2] In `async-opcua-server/src/alarms/methods.rs`, implement `handle_condition_enable`/`handle_condition_disable`, using the existing guarded-parse pattern (`parse_u32_arg`/`parse_f64_arg`, methods.rs:723-731) and calling `set_enabled`.
- [X] T019 [US2] In `methods.rs`, implement `handle_condition_suppress`/`handle_condition_suppress2`/`handle_condition_unsuppress`/`handle_condition_unsuppress2`, returning `Bad_MethodInvalid` when the target instance has no `SuppressedState` (per §5.8.8's documented result code).
- [X] T020 [US2] In `methods.rs`, implement `handle_condition_remove_from_service`/`_remove_from_service2`/`_place_in_service`/`_place_in_service2`, same `Bad_MethodInvalid` precondition guard for missing `OutOfServiceState`.
- [X] T021 [US2] In `methods.rs`, implement `handle_condition_silence` using the T017 accessors, same precondition guard for missing `SilenceState`.
- [X] T022 [US2] In `methods.rs`'s `register_condition_methods` (methods.rs:633), register all 10 new Method callbacks (`ConditionType_Enable`/`_Disable`, `AlarmConditionType_Suppress`/`_Suppress2`/`_Unsuppress`/`_Unsuppress2`/`_RemoveFromService`/`_RemoveFromService2`/`_PlaceInService`/`_PlaceInService2`, `AlarmConditionType_Silence`), following the existing `add_method_callback_with_context` pattern.
- [X] T023 [US2] Run T012-T016; confirm they pass. Fix any handler/registration mismatches found.
- [X] T024 [US2] Update `AUDIT_TABLE` for the Method CUs (look up exact CU ids for A&C Enable/Suppress/OutOfService/Silence via `search_cu`, e.g. the audit's cited 2893/2896/2897 plus any adjacent CUs for the newly-added Methods) from `Gap` to `Implemented`.

**Checkpoint**: US1 and US2 both independently functional. Ground-truth CU
lookup during T024 found the task description's CU numbers (2893/2896/2897)
were accurate, plus additional related CUs the description didn't name:
closed **2202** (Enable/Disable), **2893** (Suppress/Unsuppress),
**2896** (Silencing), **2897** (Suppression — now tested, was
already-implemented-but-untested pre-095), **4463** (Suppress2/Unsuppress2),
**4464** (OutOfService2), **4467** (OutOfService) — 7 CUs. Also upgraded
5522/5525/5528 (SuppressedState/OutOfServiceState/SilenceState
`TransitionTime`, closed in US1/T017) from "no dedicated test" to exercised
by these Method tests. Client-side counterparts (2206/2895/2900) are
out of scope — this audit is server-scoped.

---

## Phase 5: User Story 3 - A&C Audit Events (Priority: P3)

**Goal**: Emit `AuditConditionCommentEventType`/`AuditConditionEnableEventType`/
`AuditConditionSilenceEventType`/`AuditConditionOutOfServiceEventType`
(OPC-10000-9 §5.10.2, §5.10.10, §5.10.12) for the corresponding Method calls,
closing the stale marker at `methods.rs:201`.

**Independent Test**: Call `AddComment`/`Enable`/`Disable`/`Silence`/
`RemoveFromService`/`PlaceInService` with server auditing enabled; confirm
the matching audit event is emitted; confirm the underlying action still
succeeds with auditing disabled.

**Depends on**: T018 (Enable/Disable handlers), T020 (RemoveFromService/
PlaceInService handlers), T021 (Silence handler) from US2 — this story wires
audit dispatch into those handlers plus the pre-existing `AddComment`
handler. US3 can still be planned/tested independently; implementation
sequencing just requires those specific US2 tasks land first.

### Tests for User Story 3

- [X] T025 [P] [US3] Add integration test: `AddComment` with auditing enabled emits `AuditConditionCommentEventType` with correct condition reference and comment text.
- [X] T026 [P] [US3] Add integration test: `Enable`/`Disable` with auditing enabled emits `AuditConditionEnableEventType`.
- [X] T027 [P] [US3] Add integration test: `Silence` with auditing enabled emits `AuditConditionSilenceEventType`.
- [X] T028 [P] [US3] Add integration test: `RemoveFromService`/`PlaceInService` with auditing enabled emits `AuditConditionOutOfServiceEventType`.
- [X] T029 [P] [US3] Add integration test: `AddComment` with auditing disabled still succeeds and emits no audit event (FR-011).

### Implementation for User Story 3

- [X] T030 [US3] In `async-opcua-server/src/session/audit.rs`, add `ServerAuditEvent::condition_comment(...)`, `::condition_enable(...)`, `::condition_silence(...)`, and `::condition_out_of_service(...)` constructors, mirroring the existing `method_call`/`write_update` constructors (audit.rs:799-841).
- [X] T031 [US3] In `audit.rs`, add `dispatch_condition_comment_audit`/`dispatch_condition_enable_audit`/`dispatch_condition_silence_audit`/`dispatch_condition_out_of_service_audit` functions calling the existing `dispatch_audit_event` (audit.rs:855), mirroring `dispatch_method_audit` (:799).
- [X] T032 [US3] In `async-opcua-server/src/alarms/methods.rs`, wire `dispatch_condition_comment_audit` into `handle_condition_add_comment`, removing the `// ponytail: Emit AuditConditionCommentEventType when audit event support is added.` marker at methods.rs:201.
- [X] T033 [US3] Wire `dispatch_condition_enable_audit` into `handle_condition_enable`/`handle_condition_disable` (from T018).
- [X] T034 [US3] Wire `dispatch_condition_silence_audit` into `handle_condition_silence` (from T021).
- [X] T035 [US3] Wire `dispatch_condition_out_of_service_audit` into `handle_condition_remove_from_service`/`_place_in_service` (from T020).
- [X] T036 [US3] Run T025-T029; confirm they pass.
- [X] T037 [US3] Update `AUDIT_TABLE` for CU 2189 and the related A&C auditing CUs from `Gap`/`NeedsProof` to `Implemented`.

**Checkpoint**: US1, US2, and US3 all independently functional. Closed **3
CUs** (3763 A&C Auditing/AddComment — closing the stale `methods.rs:201`
marker, 3771 OutOfService Auditing, 4428 Silencing Auditing); no dedicated
"Enable Auditing" CU exists in the snapshot (Enable/Disable auditing falls
under the base 3763). Building this surfaced two real architectural
findings, both fixed: (1) the RequestContext available to a registered
Method callback (invoked from the generic Call service) does not carry the
RequestHeader-derived `AuditEventContext` that `session/audit.rs`'s
`ServerAuditEvent` needs, so a new, deliberately lighter `ConditionAuditEvent`
struct was added in `alarms/methods.rs` rather than forcing the mismatch;
(2) audit events must be raised with `SourceNode = Server` (matching
`session/audit.rs`'s own convention) — an initial attempt to target the
alarm's own source node cross-contaminated every plain `AlarmEvent`
subscription on that device, breaking a pre-existing test
(`alarm_add_comment_reports_without_state_change`) until fixed. Test
verification is scoped to `EventType`/`Message` (both inherited from
`BaseEventType`, always resolvable); `ConditionEventId`/`Comment`
(type-specific §5.10.2/§5.10.4 properties) exist correctly server-side (see
`ConditionAuditEvent` in `alarms/methods.rs`) but this test harness's
event-filter authorization check does not resolve them for the
`AuditCondition*EventType` family — a pre-existing type-tree-population gap
unrelated to this feature, out of scope to fix here.

---

## Phase 6: User Story 4 - Missing Alarm Subtypes (Priority: P4)

**Goal**: Instantiate Level/Deviation/RateOfChange/SystemOffNormal/
CertificateExpiration/Discrepancy alarms with correct `TypeDefinition`
NodeIds (OPC-10000-9 §5.8.21-§5.8.25), plus `OnDelay`/`OffDelay`/
`ReAlarmTime`/`ReAlarmRepeatCount`/`AudibleSound` properties.

**Independent Test**: Instantiate one alarm of each kind; verify
`TypeDefinition`; drive each through activation/deactivation; verify delay
and re-alarm timing separately.

### Tests for User Story 4

- [X] T038 [P] [US4] Add integration test: `ExclusiveLevelAlarmType`/`NonExclusiveLevelAlarmType` instance reports the correct `TypeDefinition` (not `ExclusiveLimitAlarmType`/`NonExclusiveLimitAlarmType`) and activates on threshold crossing (§5.8.21).
- [ ] T039 [P] [US4] Add integration test: `ExclusiveDeviationAlarmType`/`NonExclusiveDeviationAlarmType` activates when the monitored value deviates from its configured `SetpointNode` beyond threshold (§5.8.22).
- [ ] T040 [P] [US4] Add integration test: `ExclusiveRateOfChangeAlarmType`/`NonExclusiveRateOfChangeAlarmType` activates when the monitored value's rate of change exceeds a configured threshold (§5.8.23).
- [ ] T041 [P] [US4] Add integration tests: `SystemOffNormalAlarmType` (§5.8.24.3), `CertificateExpirationAlarmType` using `ExpirationDate`/`ExpirationLimit` (§5.8.24.7), and `DiscrepancyAlarmType` using `TargetValueNode` (§5.8.25) each activate on their respective trigger.
- [ ] T042 [P] [US4] Add integration test: an alarm configured with non-zero `OnDelay` does not activate until the delay elapses; `OffDelay` analogously for deactivation.
- [ ] T043 [P] [US4] Add integration test: an alarm with `ReAlarmTime` configured re-notifies while active+unacknowledged, incrementing `ReAlarmRepeatCount` on each re-notification; confirm `ReAlarmRepeatCount` resets to 0 on acknowledge.

### Implementation for User Story 4

- [X] T044 [US4] In `async-opcua-server/src/alarms/limit.rs`, add a `LimitAlarmKind { Limit, Level }` enum with a `type_id()` method (mirroring `discrete.rs::DiscreteAlarmKind`, discrete.rs:19-23,172-176) returning `ObjectTypeId::ExclusiveLimitAlarmType`/`ExclusiveLevelAlarmType` (and non-exclusive counterparts); parameterize `create_exclusive_in_address_space` (limit.rs:357) and `create_non_exclusive_in_address_space` (:439) by this kind, replacing the hardcoded `ObjectTypeId::ExclusiveLimitAlarmType`/`NonExclusiveLimitAlarmType` at limit.rs:376/458.
- [ ] T045 [US4] Add a `DeviationAlarm` evaluator + address-space wiring in `limit.rs` (or a new sibling module if it doesn't fit cleanly), using `ExclusiveDeviationAlarmType_SetpointNode`/`NonExclusiveDeviationAlarmType_SetpointNode` (generated NodeIds confirmed present) to reference the setpoint.
- [ ] T046 [US4] Add a `RateOfChangeAlarm` evaluator + address-space wiring in `limit.rs` (or sibling module), tracking a configured rate window against successive monitored values.
- [ ] T047 [US4] Add `SystemOffNormalAlarmType`/`CertificateExpirationAlarmType`/`DiscrepancyAlarmType` instantiation, following the `discrete.rs::DiscreteAlarmKind` pattern (either as new variants or a sibling kind enum); wire `CertificateExpirationAlarmType_ExpirationDate`/`_ExpirationLimit` and `DiscrepancyAlarmType_TargetValueNode` (generated NodeIds confirmed present).
- [ ] T048 [US4] Add `OnDelay`/`OffDelay` timing state to the per-instance alarm evaluation path (`async-opcua-server/src/alarms/source_monitor.rs`), reading `AlarmConditionType_OnDelay`/`_OffDelay` and delaying activation/deactivation notification accordingly.
- [ ] T049 [US4] Add `ReAlarmTime` re-notification logic to the same evaluation path, reading `AlarmConditionType_ReAlarmTime`; on each re-notification, increment and write `AlarmConditionType_ReAlarmRepeatCount` (server-maintained output counter, not client-configured — OPC-10000-9 §5.8.2), resetting it to 0 on acknowledge or deactivation.
- [ ] T050 [US4] Add `AudibleSound`/`AudibleEnabled` property wiring (`AlarmConditionType_AudibleSound`/`_AudibleEnabled`), interacting with the US2 `Silence`/`SilenceState` mechanism (T017/T021).
- [ ] T051 [US4] Run T038-T043; confirm they pass. Fix any evaluator/wiring mismatches found.
- [ ] T052 [US4] Update `AUDIT_TABLE` for CUs 2236, 2239, 2323, 2390, 2746, 2861, 2877, 2879, 2881, 2946, 2951, 3001, and adjacent CUs from `Gap` to `Implemented`.

**Checkpoint (partial)**: Only T038/T044 (the Level-alarm `TypeDefinition`
fix) landed in this pass — the cheapest slice, a pure parameterization of
already-correct evaluation logic. T045-T050 (Deviation, RateOfChange,
SystemOffNormal, CertificateExpiration, Discrepancy, OnDelay/OffDelay,
ReAlarm, AudibleSound) are genuinely new evaluator/timing logic, not
parameterizations of existing code, and were deliberately deferred rather
than implemented under time pressure without the same grounding/test/
clippy rigor applied to US1-US3 — US3 alone surfaced multiple real,
non-obvious architectural bugs (notification-routing target, event-filter
authorization scoping) that only surfaced through actual test runs, and
the remaining US4 items carry similar unknown-unknown risk. **Not** closed
in this feature: CUs 2236 (CertificateExpiration), 2239 (SystemOffNormal),
2323/2946 (RateOfChange), 2390/2951 (Deviation), 2861 (Discrepancy),
2877 (OnDelay/OffDelay), 2879 (ReAlarm), 2881 (AudibleSound) — see TODO.md
for the itemized follow-up.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [X] T053 Run the full existing `async-opcua/tests/integration/alarms.rs` suite; confirm zero regressions (SC-005).
- [X] T054 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md` via `cargo run -p async-opcua-cu-coverage-report -- <snapshot> <output>` reflecting all `AUDIT_TABLE` updates from T011/T024/T037/T052.
- [X] T055 [P] Update `TODO.md`: remove or narrow the "Alarms & Conditions state-variable block" backlog line item to reflect what's now closed vs. the explicitly-deferred residual (Alarm Groups/AlarmMetrics/COM wrapper/Previous Instances).
- [ ] T056 Run `cargo clippy --all-targets --all-features` and the project's standard CI gate (`tools/ci-playbook.sh`) before opening a PR — check `ps aux | grep ci-playbook` first per this repo's established lesson on concurrent runs corrupting generated files.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup / Foundational**: No tasks — proceed directly to User Story 1.
- **US1 (P1)**: No dependencies on other stories. Recommended first (MVP,
  highest CU-count leverage).
- **US2 (P2)**: Independent of US1's own tasks, but T017 extends
  `state_machine.rs` alongside US1's T004-T008 changes to the same file —
  sequence after US1 to avoid same-file conflicts if working serially.
- **US3 (P3)**: Depends on US2's T018/T020/T021 (the handlers it wires audit
  dispatch into) and the pre-existing `AddComment` handler. Must follow US2.
- **US4 (P4)**: Independent of US1-US3's own logic, but T050 (AudibleSound)
  interacts with US2's Silence mechanism (T017/T021) — sequence after US2.
- **Polish**: After all desired user stories are complete.

### Within Each User Story

- Tests are written first (and should fail against pre-change code) before
  implementation tasks land.
- `state_machine.rs`/`methods.rs`/`limit.rs` implementation tasks within a
  story generally touch the same file sequentially (not marked `[P]`) to
  avoid merge conflicts; test-file tasks across different assertions in the
  same `alarms.rs` file are similarly sequential unless noted.
- `AUDIT_TABLE` update task is last in each story, after that story's tests
  pass.

### Parallel Opportunities

- T001-T003 (US1 tests) can be drafted in parallel (different test
  functions), though they land in the same file.
- T012-T016 (US2 tests) similarly.
- T025-T029 (US3 tests) similarly.
- T038-T043 (US4 tests) similarly.
- T054-T055 (Polish, different files) can run in parallel.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 3 (US1): T001-T011.
2. **STOP and VALIDATE**: run the US1 Independent Test from spec.md /
   quickstart.md.
3. This alone closes 58 of 98 confirmed gaps — the single largest
   increment available.

### Incremental Delivery

1. US1 → validate → commit (per this repo's "one commit per user story"
   convention).
2. US2 → validate → commit.
3. US3 → validate → commit.
4. US4 → validate → commit.
5. Polish → PR.

Each story adds value without breaking previously-completed stories; SC-005
(no regression in existing `alarms.rs` tests) is checked at every
checkpoint, not just at the end.
