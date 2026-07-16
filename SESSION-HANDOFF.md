# SESSION HANDOFF — 2026-07-16

## Done — feature 095 (Alarms & Conditions completion)

Merged to master via PR #297 (`6f1b3cb56`). Branch `095-ac-completion` deleted
(local + remote). Full local CI gate green (21/21) and GitHub Actions green
(20/20, 2 expected skips) before merge.

Closes the largest confirmed-gap cluster from the 2026-07-15 conformance
audit (`docs/conformance-audit-2026-07-15.md`): 98 gaps + 7 partials out of
126 audited Part 9 (Alarms & Conditions) CUs.

| US | Status | What | CUs closed |
|----|--------|------|------------|
| US1 | Done (partial scope) | `TransitionTime`/`EffectiveTransitionTime`/`EffectiveDisplayName` on core sub-states (OPC-10000-9 §5.2) | 14 of 58 in range 5510-5567 — real snapshot lookup found the range is more granular (per-transition-edge timestamps, per-substate `Effective*` beyond `ActiveState`) than "one mechanism" implied |
| US2 | Done | All 10 lifecycle Methods: Enable/Disable, Suppress(2)/Unsuppress(2), RemoveFromService(2)/PlaceInService(2), Silence | 7 (2202, 2893, 2896, 2897, 4463, 4464, 4467) |
| US3 | Done | A&C audit events: `AuditConditionCommentEventType`/`EnableEventType`/`SilenceEventType`/`OutOfServiceEventType`, closing a stale `// ponytail` marker | 3 (3763, 3771, 4428) |
| US4 | Partial | Level-alarm `TypeDefinition` fix only (`LimitAlarmKind` parameterizes the existing evaluator) | 2 (2746, 3001) |

**26 CUs total** moved Gap/Partial → Implemented, each with file:line + test
citation in `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` — no
blanket flips. `specs/conformance-tester/CU-COVERAGE.md` regenerated.

### Real bugs found and fixed mid-implementation (not just written around)

1. **Audit-event `SourceNode` targeting**: first attempt targeted the alarm's
   own source device (matching `AlarmEvent`'s convention); this cross-
   contaminated every plain alarm-event subscription on that device via
   standard `HasNotifier` propagation and broke a pre-existing test
   (`alarm_add_comment_reports_without_state_change`). Fixed by targeting
   `ObjectId::Server`, matching `session/audit.rs`'s own convention.
2. **Session-audit-context mismatch**: `session/audit.rs`'s `ServerAuditEvent`
   needs an `AuditEventContext` built from `RequestHeader` (client audit
   entry id, etc.) that a registered Method callback (invoked from the
   generic Call service) does not have access to. Added a deliberately
   lighter `ConditionAuditEvent` in `alarms/methods.rs` instead of forcing
   the mismatch.
3. **Address-space model correction** (design-doc level, caught before
   coding): `ConditionStateMachine` dynamically mints namespace-2 string
   NodeIds per instance; it does NOT instantiate the full generated 1.05
   nodeset per condition. Initial design assumed writable *generated*
   per-instance NodeIds existed — they don't; only `ObjectTypeId`/
   `VariableTypeId` constants are used for `has_type_definition`.

### Not done — deferred to a follow-up feature

US4's remaining pieces need genuinely new evaluator/timing logic, not
parameterization of existing code — deferred given the amount of unplanned
complexity US3 alone turned up (2 real architectural bugs from ~10 rounds of
test-driven debugging). Itemized in `TODO.md` with CU citations and in
`specs/095-ac-completion/tasks.md` (T045-T052):

| Item | CUs | Needs |
|------|-----|-------|
| Deviation alarms (`Exclusive`/`NonExclusiveDeviationAlarmType`) | 2390, 2951 | Setpoint-tracking evaluator |
| RateOfChange alarms | 2323, 2946 | Rate-window evaluator |
| `SystemOffNormalAlarmType` | 2239 | New instantiation (likely cheap — same evaluation as existing `OffNormalAlarmType`) |
| `CertificateExpirationAlarmType` | 2236 | `ExpirationDate`/`ExpirationLimit` wiring |
| `DiscrepancyAlarmType` | 2861 | `TargetValueNode` comparison |
| `OnDelay`/`OffDelay` | 2877 | Delay timing on the per-instance evaluation path |
| `ReAlarmTime`/`ReAlarmRepeatCount` | 2879 | Re-notification logic; `ReAlarmRepeatCount` is a **server-maintained output counter**, not client-configured (confirmed via OPC UA reference tool grounding, corrected a wrong assumption mid-spec) |
| `AudibleSound`/`AudibleEnabled` | 2881 | Property wiring, interacts with US2's Silence mechanism |

Also still open in the wider AUDIT_TABLE (unrelated to A&C, from the
2026-07-15 audit, not touched this session): GDS Directory/Auth/
KeyCredential, File Access, DataAccess variable types, RBAC RolePermissions
Write path, Historical `ReadAtTimeDetails`, Historical structured-data
update, Subscription Durability, Scheduler/Redundancy/Sessionless — see
`TODO.md`.

## Current CI status (master)

Clean at `6f1b3cb56`. GitHub Actions was fully green on the PR before merge.

## Gotcha for next session

**The Bash tool's `timeout` parameter caps at 600000ms (10 min) even with
`run_in_background: true`** — `tools/ci-playbook.sh --ci` on this repo's full
workspace (build matrix + clippy variants + codegen verify + 3-stack interop
+ footprint checks across 4 foundation profiles) takes ~15-20 min end to end
and will get silently killed mid-run if launched via the Bash tool's own
backgrounding. Launch it detached instead:

```bash
nohup tools/ci-playbook.sh --ci > /path/to/scratchpad/ci-playbook.log 2>&1 &
disown
```

Then poll via `ps aux | grep ci-playbook` + `tail`/`grep FAIL` on the log in
separate Bash calls (not tied to any single call's timeout). Confirmed this
run took ~15 minutes end to end. Also: **always run `cargo fmt --all` before
the CI gate** — this session skipped it during development and the gate's
first `cargo fmt` check step failed on ~5 files' import ordering / line-
wrap, needing a full re-run to catch.

## Next — pick up US4's remainder, or a different backlog item

If continuing feature 095's US4: start from `specs/095-ac-completion/`
(spec.md/plan.md/tasks.md all still valid, just re-open a new branch off the
now-merged master and continue at T045). Otherwise see `TODO.md`'s Remaining
section for the next-highest-priority conformance backlog item.

## Commands

```bash
tools/ci-playbook.sh --ci    # pre-PR gate (~15-20 min; launch detached, see Gotcha above)
```

---

*Note: this handoff file lapsed between features 062-094 (2026-07-06 through
2026-07-15) — those features were tracked via commit history, PR descriptions,
and the auto-memory system instead. See `TODO.md`'s Done section and
`git log --oneline master` for that period.*
