# Session handoff — Alarms & Conditions completion (feature 095, 2026-07-16)

**State:** `master` at `6f1b3cb56` (PR #297 merged, fork). Branch
`095-ac-completion` deleted (local + remote). Full local CI gate green
(21/21) and GitHub Actions green (20/20, 2 expected skips) before merge.

## Delivered this session (feature 095)

Closes the largest confirmed-gap cluster from the 2026-07-15 conformance
audit (`docs/conformance-audit-2026-07-15.md`): 98 gaps + 7 partials out of
126 audited Part 9 (Alarms & Conditions) CUs. US1-US3 done; US4 partial.

| US | Status | What | CUs closed |
|----|--------|------|------------|
| US1 | Done (partial scope) | `TransitionTime`/`EffectiveTransitionTime`/`EffectiveDisplayName` on core sub-states (OPC-10000-9 §5.2) — `EnabledState`/`ActiveState`/`AckedState`/`ConfirmedState`/`SuppressedState`/`OutOfServiceState`/`ShelvingState.CurrentState`, both limit-alarm modes | 14 of 58 in range 5510-5567. Ground-truth CU lookup against the real OPC Foundation snapshot (not just range boundaries) found the range is far more granular than "one shared mechanism" assumed: per-specific-transition-edge timestamps (e.g. distinct `UnshelvedToTimedShelved`/`TimedShelvedToUnshelved`/... properties) and per-substate `Effective*` variants beyond `ActiveState`. Remaining 44 documented precisely in `AUDIT_TABLE`, not silently dropped. |
| US2 | Done | All 10 lifecycle Methods: `Enable`/`Disable` (§5.5.4/.5), `Suppress`/`Suppress2`/`Unsuppress`/`Unsuppress2` (§5.8.8-.11), `RemoveFromService`/`RemoveFromService2`/`PlaceInService`/`PlaceInService2` (§5.8.12-.15), `Silence` (§5.8.7). First test coverage for previously-untested `SuppressedState`/`OutOfServiceState`. | 7 (2202, 2893, 2896, 2897, 4463, 4464, 4467) |
| US3 | Done | A&C audit events (§5.10.2-.12): `AuditConditionCommentEventType`/`EnableEventType`/`SilenceEventType`/`OutOfServiceEventType`, closing the stale `alarms/methods.rs:201` `// ponytail` marker | 3 (3763, 3771, 4428) |
| US4 | Partial | Level-alarm `TypeDefinition` fix (§5.8.21.2/.3): `LimitAlarmKind` parameterizes the existing `LimitAlarm` evaluator instead of duplicating it — `register_level_alarm` new entry point, `register_limit_alarm`'s own signature/behavior unchanged for all 11+ existing call sites | 2 (2746, 3001) |

**26 CUs total** moved Gap/Partial → Implemented, each with file:line + test
citation in `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` (no
blanket flips). `specs/conformance-tester/CU-COVERAGE.md` regenerated.
48/48 `async-opcua/tests/integration/alarms.rs` tests passing, confirmed
across repeated runs (SC-005, zero regressions).

### Real architectural bugs found and fixed mid-implementation

1. **Audit-event `SourceNode` targeting**: the first attempt targeted the
   alarm's own source device (matching `AlarmEvent`'s convention). This
   cross-contaminated every plain alarm-event subscription on that device
   via standard `HasNotifier` propagation (subscribing to a node receives
   events from its whole descendant hierarchy) and broke a pre-existing
   test (`alarm_add_comment_reports_without_state_change`) until fixed by
   targeting `ObjectId::Server`, matching `session/audit.rs`'s own
   convention.
2. **Session-audit-context mismatch**: `session/audit.rs`'s
   `ServerAuditEvent` needs an `AuditEventContext` built from
   `RequestHeader` (client audit entry id, secure channel id, ...) — not
   available to a Method callback registered via
   `add_method_callback_with_context` and invoked from the generic Call
   service. Added a deliberately lighter `ConditionAuditEvent` struct in
   `alarms/methods.rs` rather than forcing the architectural mismatch.
3. **Address-space model correction** (caught at design-review time, before
   coding): `ConditionStateMachine::create_in_address_space` dynamically
   mints namespace-2 string NodeIds per condition instance — it does NOT
   instantiate the full generated 1.05 nodeset per condition. The initial
   plan/data-model assumed writable generated per-instance NodeIds existed
   (e.g. `AlarmConditionType_EnabledState_TransitionTime = 9016`); those are
   abstract-type ModellingRule template declarations, not concrete
   addresses. Only `ObjectTypeId`/`VariableTypeId` constants are used, for
   `has_type_definition` references.
4. **`ReAlarmRepeatCount` semantics** (caught during grounding, before
   implementation): the spec description assumed it was a client-configured
   "stop after N re-alarms" limit. OPC UA reference tool grounding
   (OPC-10000-9 §5.8.2: `ReAlarmRepeatCount`, Int16, `BaseDataVariableType`,
   not a Property) confirmed it's actually a **server-maintained output
   counter** — re-alarming continues indefinitely at `ReAlarmTime` intervals
   while active+unacknowledged; there's no base-spec "stop after N" concept.
   Corrected across spec.md/data-model.md/tasks.md before any code was
   written against the wrong assumption.
5. **CU 2189 mislabeling** (caught during US3 grounding): the original task
   description cited CU 2189 as "A&C Auditing." The real snapshot shows
   CU 2189 is "ConditionClasses" (multiple condition classes for alarm
   grouping/filtering) — an unrelated concept. The real Auditing CUs are
   3763-3771 + 4428, which is what got closed. `AUDIT_TABLE`'s own evidence
   text for 2189 was already correct; only the task description's citation
   was wrong.

### Not done — deferred to a follow-up feature

US4's remaining pieces need genuinely new evaluator/timing logic, not
parameterization of already-correct code like the Level-alarm fix was —
deferred given the amount of unplanned complexity US3 alone turned up (2
real architectural bugs surfaced only through actual test-run failures, not
foreseeable from grounding/planning alone). Itemized in `TODO.md` with CU
citations and in `specs/095-ac-completion/tasks.md` (T045-T052, each already
grounded with file:line targets and generated-NodeId-constant confirmations
from this session):

| Item | CUs | Needs |
|------|-----|-------|
| Deviation alarms (`Exclusive`/`NonExclusiveDeviationAlarmType`) | 2390, 2951 | Setpoint-tracking evaluator (`ExclusiveDeviationAlarmType_SetpointNode` etc. already confirmed to exist as generated NodeIds) |
| RateOfChange alarms | 2323, 2946 | Rate-window evaluator (new — no existing analog in this codebase) |
| `SystemOffNormalAlarmType` | 2239 | New instantiation — likely cheap, probably reuses `OffNormalAlarmType`'s existing evaluation logic exactly (same pattern as the Level-alarm fix), just needs confirming |
| `CertificateExpirationAlarmType` | 2236 | `ExpirationDate`/`ExpirationLimit` wiring (generated NodeIds confirmed present) |
| `DiscrepancyAlarmType` | 2861 | `TargetValueNode` comparison (generated NodeId confirmed present) |
| `OnDelay`/`OffDelay` | 2877 | Delay timing added to the per-instance evaluation path (`source_monitor.rs`) |
| `ReAlarmTime`/`ReAlarmRepeatCount` | 2879 | Re-notification logic; `ReAlarmRepeatCount` is a server-maintained output counter (see bug #4 above), not client-configured |
| `AudibleSound`/`AudibleEnabled` | 2881 | Property wiring, interacts with US2's Silence mechanism (`state_machine.rs` `get_silenced`/`set_silenced`) |

Also still open in the wider `AUDIT_TABLE` (unrelated to A&C, from the
2026-07-15 audit, not touched this session): GDS Directory/Auth/
KeyCredential, File Access, DataAccess variable types, RBAC RolePermissions
Write path, Historical `ReadAtTimeDetails`, Historical structured-data
update, Subscription Durability, Scheduler/Redundancy/Sessionless — see
`TODO.md`'s Remaining section.

### Gotcha for next session: CI playbook backgrounding

**The Bash tool's `timeout` parameter caps at 600000ms (10 min) even with
`run_in_background: true`.** `tools/ci-playbook.sh --ci` on this repo's full
workspace (build matrix + all clippy variants + codegen verify + 3-stack
interop + footprint checks across 4 foundation profiles) takes ~15-20
minutes end to end and gets silently killed mid-run if launched via the Bash
tool's own backgrounding — confirmed twice this session (both runs killed
around the 10-minute mark with no real failure, just still compiling).
Launch it detached instead:

```bash
nohup tools/ci-playbook.sh --ci > /path/to/scratchpad/ci-playbook.log 2>&1 &
disown
```

Then poll via `ps aux | grep ci-playbook` + `tail`/`grep FAIL` on the log
file in separate Bash calls, not tied to any single call's timeout. This
run completed clean (21/21 PASS) in ~15 minutes once launched this way.

Also: **run `cargo fmt --all` before the CI gate**, not after — this session
skipped it during development and the gate's `cargo fmt` check step failed
on ~5 files' import ordering / line-wrapping on the first attempt, forcing a
second full ~15-minute re-run to reconfirm green after `cargo fmt --all`
fixed it.

### CI gates (all green at merge, PR #297)

- `cargo fmt --all -- --check` — PASS (after `cargo fmt --all` fix)
- Full local `tools/ci-playbook.sh --ci` — 21/21 PASS, 0 FAIL
- GitHub Actions — 20/20 PASS (2 expected skips: `external-interop`,
  `release`)
- `async-opcua/tests/integration/alarms.rs` — 48/48 PASS, confirmed stable
  across 3+ repeated runs (no flakiness from the new timing-sensitive audit-
  event tests)

### Files changed: 8 source + 1 test file + 7 spec artifacts + docs = ~17 files, ~+3130/-136 lines (6 commits, one per user story + polish + fmt)

---

## Next — pick up US4's remainder, or a different backlog item

If continuing feature 095's US4: `specs/095-ac-completion/spec.md`/
`plan.md`/`tasks.md` are all still valid and merged to master — open a new
branch and continue at T045 (Deviation alarm evaluator). Otherwise see
`TODO.md`'s Remaining section for the next-highest-priority conformance
backlog item.

---

# Session handoff — hot path audit fixes (feature 061, 2026-07-05)

**State:** branch `061-hot-path-audit-fixes` on `master` (fork). Features 060 + 061
stacked in one branch. All CI green.

## Delivered this session (feature 061)

### Fixes applied (5 user stories + 1 bug fix)

| Story | Severity | Fix | Impact |
|-------|----------|-----|--------|
| US1 | CRITICAL | `DecodingOptions` → `Arc<DecodingOptions>` | Eliminates 104-byte struct clone on every encode/decode |
| US2 | CRITICAL | Type tree `publish_type_tree_snapshot` once | Snapshot published once, not N times per init |
| US3 | HIGH | `RequestContext` cached on `SessionActor` | Per-Read/Write: `Arc::clone` when token unchanged |
| US4 | MEDIUM | `SecurityPolicy` validated once | 7-arm match per encrypt/decrypt → bool flag |
| US5 | MEDIUM | Parallel cert+key file I/O | `std::thread::scope` across endpoints |
| Bug | HIGH | Restore `SecurityPolicy::None` guard on clientNonce | Open62541 interoperability (zero-length nonce for None) |

### One task deferred per research decision

- **T013**: Per-chunk `SecurityPolicy::from_uri()` in `security_header.rs` — left for future
  optimization. Requires threading the cached policy through the chunk decoding pipeline.

### Changes: 29 files, +709/-118 lines (source across 7 crates + tests)

### Benchmark (combined 060 + 061, CPU 11, taskset -c 11)

| Metric | Pre-060 baseline | 060 alone | 060+061 combined |
|--------|-----------------|-----------|-----------------|
| Read (req/s) | 98,605 | 110,081 | 108,785 |

061 adds no throughput regression; the allocation/caching/validation fixes complement the
compilation-level optimizations from 060.

### CI gates (all green at PR #266)

- `cargo fmt --check` — PASS
- `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` — PASS
- `cargo test --locked --all-features` — PASS (0 failures)
- `tools/ci-playbook.sh --ci` — 23 PASS, 0 FAIL
- `--no-default-features` builds — PASS

---

# Session handoff — perf regression fix (feature 060, 2026-07-05)

**State:** branch `060-perf-regression-fix` on `master` (fork), all CI green. Feature 060
delivered: the 27% throughput regression was investigated and resolved with three fixes
yielding an **+11% throughput improvement** over HEAD baseline.

## Delivered this session (feature 060)

### Root cause re-evaluation

The ~90k→66k regression reported in the session handoff for feature 059 was re-evaluated with
CPU isolation (`taskset -c 11`). The pinned HEAD baseline was **~98.6k read / ~93.7k write** —
well above the claimed 66k. This indicates the original "regression" was measurement noise from
CPU scheduling, not a code-induced problem. However, the three compilation optimization fixes
still yielded a net **+11.6% read / +11.0% write** improvement.

### Fixes applied

| Fix | File(s) | Impact |
|-----|---------|--------|
| VIEW-03 revert | `async-opcua-server/src/node_manager/view.rs` | Inlined `strip_result_mask_fields()` back into `add()` and `add_unchecked()` |
| `#[inline]` annotations | `controller.rs` (process_request, validate_request), `instance.rs` (validate_timed_out, validate_activated) | Counteracts LLVM de-inlining from code-size heuristics |
| Release profile tuning | `Cargo.toml` (`[profile.release]` codegen-units=1, lto=true) | Full LLVM visibility for inlining across crate |

### Benchmark results (CPU 11, taskset -c 11, median of 3 runs)

| Metric | Pre-fix (HEAD) | Post-fix (all fixes) | Delta |
|--------|---------------|---------------------|-------|
| Read (req/s) | 98,605 | 110,081 | **+11.6%** |
| Write (req/s) | 93,726 | 104,032 | **+11.0%** |

### CI gates (all green)

- `cargo fmt --all -- --check` — PASS
- `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` — PASS
- `cargo test --locked --all-features` — PASS (0 failures)
- `tools/ci-playbook.sh --ci` — ALL PASS
- `RUSTFLAGS="-D warnings" cargo check --no-default-features -p async-opcua -p async-opcua-types -p async-opcua-nodes -p async-opcua-server` — PASS

### Files changed: 3 source + 1 config + 7 spec artifacts = 11 files, ~+60/-40 lines

### Compliance verification (T021)

Code inspection confirmed all 23 feature 059 compliance findings remain intact. The VIEW-03
revert preserves the same result-mask-field-clearing logic at both `add()` and `add_unchecked()`
call sites — identical 5 if-blocks, identical fields, identical conditions.

---

## Conventions / gotchas (carried forward)

- **Benchmarking requires CPU isolation**: Use `taskset -c 11` (or any single isolated core)
  when running `async-opcua-localhost-bench`. Without pinning, OS scheduling noise can cause
  ±50% variance in reported throughput. This explains the original "66k" measurement.
- **Pre-push gate:** `cargo fmt --check`; clippy `--workspace --all-targets --all-features`;
  `RUSTFLAGS="-D warnings" cargo check --no-default-features -p async-opcua -p async-opcua-types
  -p async-opcua-nodes -p async-opcua-server`; foundation-profile builds; `cargo deny check
  advisories`; full workspace tests.
- **OPC UA spec citations:** Not applicable to this feature (performance optimization).
- **Merge strategy:** rebase-and-merge on the fork (`occamsshavingkit/async-opcua`), never push
  upstream (`FreeOpcUa/async-opcua`) without explicit request.

---

# Session handoff — spec compliance audit closeout (feature 059, 2026-07-05)

**State:** `master` clean at merge commit `765434c3b` (PR #264, rebased), all CI green. Feature
059 delivered and merged. A ~27% throughput regression (90k → 66k req/sec) was detected in the
localhost read/write benchmark post-merge; root cause is pending bisection (see §Performance
regression below).

**Driving principle:** async-opcua is a *complete reference implementation* — build the spec
surface; do not defer spec-defined behavior on YAGNI/ponytail grounds (memory
`completeness-over-yagni`).

## Headline

**23 OPC UA spec compliance findings closed across 7 OPC UA Parts (3, 4, 5, 6, 7, 12).**
Zero remaining HIGH/MEDIUM/MINOR findings. One known limitation deferred (DISC-05: ECC asymmetric
encryption).

## Delivered this session (feature 059, PR #264 — MERGED)

Artifacts: `specs/059-spec-compliance-audit-fixes/`. Audit source:
`docs/spec-compliance-audit-2026-07-05.md`.

### Changes by OPC UA Part

| Part | Findings | Key Changes |
|------|----------|-------------|
| Part 4 §5.7 Session Services | 8 | sessionName default, nonce range [32,128], unactivated session eviction, authenticationToken, X509 signature, localeIds preservation, revisedSessionTimeout > 0 |
| Part 4 §5.6/§6.1 SecureChannel | 4 | CloseSecureChannel audit event, token_created_at renewal, redundant set_role removal, async-cleanup docs |
| Part 4 §5.9 View Services | 3 | RESULT_MASK_IS_FORWARD, BrowseDirection::INVALID, external reference result mask |
| Part 4 §5.5/§5.13 Discovery & Subscriptions | 2 | startingRecordId `>` filter, publishingInterval precision |
| Part 7/12 Discovery & Security | 4 | ECC security levels, endpoint URL filtering, locale-aware server names, ECC encryption (deferred) |
| Part 3/6 Address Space & Encoding | 2 | set_browse_name pub(crate), set_token_created_at accessor |

### Files changed: 14 source + 2 test + 7 spec artifacts = 23 files, +1107/-54 lines.

### Process notes

- Three `/speckit.analyze` passes (general → atomicity → spec-citation) caught real issues:
  missing task for SC-02 (tokenCreatedAt verify), missing spec reference on T020 (SUB-01), and
  14 underspecified "verify" tasks without method — all fixed before implementation.
- `software-engineer-zai` ran out of tokens; switched to `general` for implementation and
  `qa-engineer` for verification per AGENTS.md role assignments.
- Verification tasks should not be assigned to software engineers; qa-engineer is the correct
  agent. Recorded in `specs/059-spec-compliance-audit-fixes/tasks.md` as a process correction.
- A `git stash -u` / `git stash pop` cycle silently lost working-tree changes (pre-existing
  branch changes not committed). Recovered by re-applying all edits from the task plan.
- Pre-existing working-tree changes on this branch (SESSION-01 through SESSION-08, SC-01,
  VIEW-01, SUB-01, etc.) were partial implementations that needed to be committed together
  with the audit-driven T001-T022 tasks.

### CI gates (all green at merge)

- `cargo fmt --all -- --check` — PASS
- `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` — PASS
- `cargo test --locked --all-features` — PASS (0 failures)
- `cargo build --workspace --all-features` — PASS

### Test adjustments

- `create_session_limit_lock_scope.rs`: Updated assertion for SESSION-02 eviction (unactivated
  session replaced at capacity instead of BadTooManySessions).
- `event_filter_tests.rs`: Extended poll window 4→6 iterations — the SC-01 CloseSecureChannel
  audit event shifts the AuditActivateSessionEventType beyond the old window.
- `event_filter_tests.rs` plus `create_session_limit_lock_scope.rs`: Removed unused `StatusCode`
  import after assertion changes.

---

## Performance regression: 90k → 66k req/sec (27% drop)

**Background:** The `tools/opcua-localhost-bench` Read/Write benchmark dropped from ~90k to
~66k req/sec after the feature 059 merge. Analysis confirmed **no code-level changes in the
Read/Write hot path** — the `process_request` message handler in controller.rs is byte-for-byte
identical between base and HEAD.

### Likely mechanism: indirect compilation effects

The diff adds ~1,100 lines across 23 files. This code growth can cause:

1. **Instruction cache pressure** — larger binary spills the hot loop out of L1i cache
2. **LLVM inlining threshold** — added code in the same crate pushes hot functions past the
   inlining cost limit, turning inline code into function calls
3. **`.text` section layout** — new cold-path functions shift hot-path functions in memory,
   disrupting branch predictor spatial locality

### Recommended next step: speckit workflow

Create a speckit feature to diagnose and fix the regression:

```
/speckit.specify investigate and fix the 27% throughput regression (90k → 66k req/sec)
in the localhost read/write benchmark, caused by indirect compilation effects from
the feature 059 spec compliance changes
```

### Fix candidates (ordered by expected impact)

1. **Profile first** — run `perf stat -e instructions,cycles,cache-misses,branch-misses` on both
   base and HEAD builds to confirm the mechanism.
2. **`#[inline]` on hot-path functions** — add `#[inline]` to `validate_timed_out`,
   `validate_activated`, and the `message =>` dispatch handler in controller.rs. This prevents
   LLVM from de-inlining due to code-size heuristics. Lowest risk, highest expected impact.
3. **Release profile tuning** — `codegen-units = 1` and `lto = true` in `Cargo.toml` release
   profile. Gives LLVM full visibility across the crate for inlining decisions.
4. **Revert VIEW-03 refactoring** — the `strip_result_mask_fields()` extraction in
   `node_manager/view.rs` is the only change that modifies struct method layout on a frequently
   instantiated type (`BrowseNode`), affecting compilation-unit layout. If profiling confirms
   layout disruption, revert to inline field-stripping in `add_unchecked()`.
5. **Function ordering** — if profiling confirms `.text` layout disruption, investigate
   `#[link_section]` or linker script approaches to colocate hot-path functions.

### What NOT to do

- Do NOT remove any compliance fix — all 23 findings are spec-mandated OPC UA behaviors
- Do NOT add `#[cold]` to cold-path functions as the primary fix — LLVM usually infers this;
  the real issue is de-inlining of hot functions, not speculation on cold ones

---

## Conventions / gotchas (carried forward)

- **Pre-push gate:** `cargo fmt --check`; clippy `--workspace --all-targets --all-features`;
  `RUSTFLAGS="-D warnings" cargo check --no-default-features -p async-opcua -p async-opcua-types
  -p async-opcua-nodes -p async-opcua-server`; foundation-profile builds; `cargo deny check
  advisories`; full workspace tests.
- **OPC UA spec citations:** All tasks touching behavior must cite their governing Part/§.
- **Agent roles:** software-engineer for implementation, qa-engineer for verification,
  architect for architecture review, explore for codebase research.
- **One task per assignment** — never batch multiple tasks into one agent dispatch.
- **Verification tasks:** "Verify by code inspection that [finding] is present at [file:line]."
- **Merge strategy:** rebase-and-merge on the fork (`occamsshavingkit/async-opcua`), never push
  upstream (`FreeOpcUa/async-opcua`) without explicit request.
