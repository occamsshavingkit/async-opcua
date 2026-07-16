# Implementation Plan: Alarms & Conditions Completion

**Branch**: `095-ac-completion` | **Date**: 2026-07-16 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/095-ac-completion/spec.md`

## Summary

Close the largest confirmed gap cluster from the 2026-07-15 conformance
audit: 98 of 126 audited Part 9 (Alarms & Conditions) CUs. Four
independently-testable increments, in priority order: (1) populate
`TransitionTime`/`EffectiveTransitionTime`/`EffectiveDisplayName` on every
`TwoStateVariableType` sub-state — the generated nodeset already has these
as addressable Variable NodeIds (confirmed via
`async-opcua-types/src/generated/node_ids.rs`, e.g.
`AlarmConditionType_EnabledState_TransitionTime = 9016`), so this is a
write-path addition, not new address-space structure; (2) register 9 new
Method callbacks (`Enable`/`Disable`/`Suppress`/`Suppress2`/`Unsuppress`/
`Unsuppress2`/`RemoveFromService`/`RemoveFromService2`/`PlaceInService`/
`PlaceInService2`/`Silence`) in `alarms/methods.rs` following the exact
pattern already used for `Acknowledge`/`Confirm`/`AddComment`; (3) emit 4
audit event types via the existing `ServerAuditEvent`/`dispatch_*_audit`
pattern in `session/audit.rs`; (4) add Level/Deviation/RateOfChange/
SystemOffNormal/CertificateExpiration/Discrepancy alarm kinds plus
OnDelay/OffDelay/ReAlarm/AudibleSound properties, extending the existing
`LimitAlarm`/`DiscreteAlarmKind` patterns in `limit.rs`/`discrete.rs`.

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`)
**Primary Dependencies**: `async-opcua-server` (alarms module), `async-opcua-types` (generated NodeIds/ObjectTypeIds), `async-opcua-core` (`AlarmEvent`, `sync::RwLock`)
**Storage**: N/A (in-memory `AddressSpace`, no persistence layer touched)
**Testing**: `cargo test`, integration tests in `async-opcua/tests/integration/alarms.rs`
**Target Platform**: Cross-platform Rust library (server crate), `alarms` + `method-call` + `generated-address-space` cargo features
**Project Type**: Rust workspace library — single crate area (`async-opcua-server/src/alarms/`)
**Performance Goals**: No new performance targets; state-variable writes and Method dispatch reuse existing hot paths (address-space write, `notify_events`) with no additional per-tick polling
**Constraints**: MUST NOT panic on malformed/adversarial Method-call input (Constitution Principle IV — alarms Methods are reachable from any authenticated session); MUST NOT regress existing `alarms.rs` integration tests (SC-005)
**Scale/Scope**: 58 CUs (US1) + ~11 Methods (US2) + 4 audit event types (US3) + 6 alarm kinds + 4 properties (US4), all within `async-opcua-server/src/alarms/` (currently 4286 LOC across 10 files) plus `async-opcua-core/src/events.rs` and `session/audit.rs`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Each user story ships with its own
  passing tests before being considered done (SC-001 through SC-004 each
  name a test requirement); no user story is marked complete on a
  happy-path-only implementation. PASS.
- **II. Do It Right Once**: US1's design reuses generated NodeIds rather
  than improvising new address-space structure; US4's Level-alarm fix
  corrects the root cause (wrong `ObjectTypeId` constant) rather than adding
  a workaround. The stale `ponytail` marker in `methods.rs:201` is resolved,
  not left in place. PASS.
- **III. Individual Task Discipline**: Enforced at `/speckit-tasks` — each
  task is one file-scoped, independently verifiable change; the four user
  stories are themselves already independently testable, and tasks.md will
  decompose each further into single-purpose tasks. PASS (verified at task
  generation).
- **IV. Security Is Paramount**: New Method callbacks (US2) are reachable
  from any session with Call-service access; each MUST reject malformed
  arguments and missing-state-variable preconditions with a `Bad*` status
  code rather than panicking, mirroring the existing `parse_u32_arg`/
  `parse_f64_arg` guarded-parse pattern in `methods.rs`. No cryptographic or
  authentication code is touched. PASS.
- **V. Leave It Better Than You Found It**: US2 adds the first test coverage
  for `SuppressedState`/`OutOfServiceState` (pre-existing, untested code);
  US3 removes the stale `ponytail` marker instead of leaving it dangling.
  PASS.

No violations requiring Complexity Tracking justification.

## Project Structure

### Documentation (this feature)

```text
specs/095-ac-completion/
├── plan.md              # This file
├── spec.md              # Feature specification
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks — not created by /speckit-plan)
```

### Source Code (repository root)

```text
async-opcua-server/src/alarms/
├── state_machine.rs     # US1: add TransitionTime/EffectiveTransitionTime/EffectiveDisplayName
│                         #      writers on EnabledState/ActiveState/AckedState/ConfirmedState/
│                         #      SuppressedState/OutOfServiceState/SilenceState transitions;
│                         #      US2: add get/set is already present for enabled/out_of_service,
│                         #      extend with Silence/Suppress transition-time hooks
├── limit.rs              # US1: TransitionTime on Low/LowLow/High/HighHigh limit states;
│                         #      US4: parameterize create_exclusive_in_address_space /
│                         #      create_non_exclusive_in_address_space by alarm kind
│                         #      (Limit vs. Level, mirroring discrete.rs's DiscreteAlarmKind),
│                         #      add Deviation/RateOfChange evaluator + address-space wiring
├── discrete.rs            # US4: extend DiscreteAlarmKind or add sibling kind enum for
│                         #      SystemOffNormal/CertificateExpiration/Discrepancy
├── methods.rs             # US2: register_condition_methods gains Enable/Disable/Suppress/
│                         #      Suppress2/Unsuppress/Unsuppress2/RemoveFromService/
│                         #      RemoveFromService2/PlaceInService/PlaceInService2/Silence
│                         #      handlers; US3: resolve the methods.rs:201 ponytail marker,
│                         #      wire audit dispatch into each new + existing handler
├── dispatch.rs, refresh_events.rs, registry.rs, source_monitor.rs, transitions.rs, dialog.rs
│                         # touched only if a shared helper needs extending; no new files
│                         #      expected in this set
└── mod.rs                # re-exports for any new public kind enums/structs

async-opcua-core/src/
└── events.rs              # US3: AlarmEvent gains fields needed for audit-event dispatch
                          #      (condition class / event-type distinction) if not already
                          #      derivable from existing fields

async-opcua-server/src/session/
└── audit.rs                # US3: new ServerAuditEvent constructors + dispatch_*_audit
                          #      functions for AuditConditionCommentEventType/
                          #      AuditConditionEnableEventType/AuditConditionSilenceEventType/
                          #      AuditConditionOutOfServiceEventType, following the existing
                          #      dispatch_method_audit/dispatch_write_audit pattern

async-opcua/tests/integration/
└── alarms.rs               # tests for all 4 user stories; existing tests must not regress (SC-005)

tools/cu-coverage-report/src/lib.rs
└── AUDIT_TABLE              # updated post-implementation: CUs 5510-5567 (US1) and the
                          #      Method/audit/subtype CUs named in spec.md move from
                          #      Gap to Implemented, each with a file:line citation
```

**Structure Decision**: Single Rust workspace crate area — all production
code changes are confined to `async-opcua-server/src/alarms/` (extending the
existing 10-file module) plus two small, targeted touches in
`async-opcua-core/src/events.rs` and `async-opcua-server/src/session/audit.rs`
to reuse (not duplicate) the existing audit-event infrastructure. No new
crates, no new top-level directories, no new cargo features beyond the
already-existing `alarms`/`method-call`/`generated-address-space` gates this
module is already built behind.

## Complexity Tracking

*No Constitution Check violations — this section is not applicable.*
