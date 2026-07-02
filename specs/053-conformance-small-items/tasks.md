# Tasks: Conformance Small-Items Sprint

**Feature**: `053-conformance-small-items` | **Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)
**Input**: research.md (grounded decisions + file:line anchors), data-model.md, contracts/service-behavior.md

Division of labor per practice: Claude authors tests red-first (independent, spec-grounded via the
opc-ua-reference MCP); codex implements (one task per dispatch, no-git guardrail, no test edits).
Every task cites its OPC UA Part/§. One commit per user story (SC-007); each story's commit updates
its `specs/conformance-audit/FINDINGS.md` row (FR-011).

## Phase 1: Setup

- [ ] T001 Verify baseline on branch `053-conformance-small-items-sprint`: workspace builds; run
  `cargo test -p async-opcua-server` and the integration suites touched by this sprint
  (`read.rs`, `write.rs`, `browse.rs`, `subscriptions.rs`) green, so red-first diffs are attributable.

*No Phase 2 foundational tasks — the seven stories are mutually independent (plan.md).*

---

## Phase 2: US1 — ServerDiagnostics mandatory children, P5-04 (Priority: P1) — OPC UA Part 5 §6.3.3 (Table 11)

**Goal**: `EnabledFlag` (i=2294), `SubscriptionDiagnosticsArray` (i=2290),
`SessionsDiagnosticsSummary` (i=3706) + `SessionDiagnosticsArray` (i=3707) +
`SessionSecurityDiagnosticsArray` (i=3708) present, live, and permission-gated.

**Independent test**: browse `Server → ServerDiagnostics`, read the five nodes with a live
session+subscription, toggle `EnabledFlag`, verify gating (contracts/service-behavior.md §US1).

- [ ] T002 [P] [US1] codex: add read-only session enumeration to `SessionManager`
  (`async-opcua-server/src/session/manager.rs:506` private map — new accessor with snapshot
  semantics) and read-only diagnostics getters on `Subscription`
  (`async-opcua-server/src/subscriptions/subscription.rs:194-204` private fields). Pure accessors,
  no behavior change. Data sources for Part 5 §6.3.3 arrays.
- [ ] T003 [P] [US1] Claude: red-first tests — extend `async-opcua/tests/integration/read.rs`
  (`test_diagnostics` :1285 area) + `browse.rs`: children present with standard NodeIds/type
  definitions (assert ALL Table-11 mandatory members incl. the already-served
  `ServerDiagnosticsSummary`, for SC-002); arrays return one entry per live subscription/session with plausible counters;
  `EnabledFlag` reads config state; unprivileged `EnabledFlag` write → `Bad_UserAccessDenied`;
  disabled → empty arrays; `SessionSecurityDiagnosticsArray` denied to non-admin. Part 5 §6.3.3.
- [ ] T004 [US1] codex: serve the five NodeIds — extend `is_mapped`/`get` in
  `async-opcua-server/src/diagnostics/server.rs` (:137/:156) and the core read dispatch
  `node_manager/memory/core.rs:586-595`: `EnabledFlag` from the runtime `enabled` flag;
  `SubscriptionDiagnosticsArray` built on read from `SubscriptionCache`
  (`subscriptions/mod.rs:388/:401` + T002 getters); session arrays from the T002 iterator
  (`session/instance.rs` getters + `session_locale_ids`); disabled → empty arrays. Part 5 §6.3.3.
- [ ] T005 [US1] codex: `EnabledFlag` write path (privileged sessions only, toggles the runtime
  flag) and `SessionSecurityDiagnosticsArray` admin gating layered on the existing
  `read_diagnostics` permission gate (`core.rs:587-588`). Fail closed (constitution §IV).
  Part 5 §6.3.3.
- [ ] T006 [US1] Update `specs/conformance-audit/FINDINGS.md` P5-04 row (+ reconciliation banner)
  → FIXED with file/test evidence; commit story 1.

---

## Phase 3: US2 — Write range/enum validation, P4-ATTR-04 (Priority: P2) — Part 4 §5.11.4; Part 8 §5.3.2.2, §5.3.3.3/.4

**Goal**: writes violating modeled EURange or enumeration value sets → `Bad_OutOfRange`, stored
value unchanged; unconstrained Variables unaffected.

**Independent test**: write matrix against an AnalogItem with EURange and an enum-typed Variable
(contracts §US2).

- [ ] T007 [P] [US2] Claude: red-first tests — `async-opcua/tests/integration/write.rs` + unit
  tests in `async-opcua-server/src/address_space/utils.rs` (mod tests :708): out-of-range scalar
  rejected + value unchanged; in-range accepted; undefined enum value rejected, defined accepted
  (scalar AND array-element/index-ranged enum writes); index-ranged element out of range
  rejected; unconstrained Variable regression guard.
  Part 4 §5.11.4; Part 8 §5.3.3.3/.4.
- [ ] T008 [US2] codex: implement the check in `validate_node_write`'s Value arm
  (`address_space/utils.rs:381-398`, after RBAC/type checks, before `set_value_range`): resolve
  `EURange` via the `alarms/limit.rs:199 read_eurange` pattern; enum value sets via the DataType
  node's `DataTypeDefinition::Enum` fields (`async-opcua-nodes/src/data_type.rs:47`); reject with
  `BadOutOfRange`; no constraint → no check. Part 4 §5.11.4; Part 8 §5.3.2.2.
- [ ] T009 [US2] Update FINDINGS.md P4-ATTR-04 row → FIXED with evidence; commit story 2.

---

## Phase 4: US3 — LocalizedText write locale rules, P4-ATTR-03 (Priority: P2) — Part 4 §5.11.4.1

**Goal**: complete the feature-049 locale machinery: null-text deletes a locale entry, null-locale
updates the default text, unsupported locale already → `Bad_LocaleNotSupported` (verify);
Value-attribute behavior documented (server-specific per spec).

**Independent test**: locale write matrix on DisplayName/Description (contracts §US3).

- [ ] T010 [P] [US3] Claude: red-first tests — `read.rs`/`write.rs` integration + `utils.rs` unit:
  add locale keeps others; overwrite same locale; null text + locale deletes that entry only;
  null locale updates default text; unsupported locale → `Bad_LocaleNotSupported` with store
  unchanged; Value-attribute single-locale behavior lock-in. Part 4 §5.11.4.1.
- [ ] T011 [US3] codex: gap-close the side-table rules in `address_space/utils.rs`
  (`remember_localized_text_attribute_value` :453, `validate_localized_text_attribute_write_locale`
  :487-514, `localized_text_for_session` :520): null-text deletion, null-locale default-text
  update; doc-comment the Value-attribute server-specific choice. Part 4 §5.11.4.1.
- [ ] T012 [US3] Update FINDINGS.md P4-ATTR-03 row → FIXED with evidence; commit story 3.

---

## Phase 5: US4 — Read maxAge, P4-ATTR-02 (Priority: P3) — Part 4 §5.11.2.2 (Table 47)

**Goal**: maxAge honored where a refreshable source exists (callback/sampler); in-memory plain
values documented always-current; parameter never silently dropped for callback sources.

**Independent test**: callback-backed node with controllable source timestamps, maxAge 0/mid/max
(contracts §US4).

- [ ] T013 [P] [US4] Claude: red-first tests — `SimpleNodeManager` callback-source tests (+
  `read.rs` integration): maxAge=0 forces fresh sample; ≥ max Int32 permits cached; mid-range
  refreshes iff older; plain in-memory node self-consistent for any valid maxAge; negative maxAge
  still `Bad_MaxAgeInvalid` (regression); NaN/±infinity maxAge must not panic (NaN passes the
  `< 0.0` guard — document + test the chosen interpretation). Part 4 §5.11.2.2.
- [ ] T014 [US4] codex: freshness decision in `node_manager/memory/simple.rs` (callback invocation
  :240-245 and the `SyncSampler`-backed internal sampled values :330/:338): compare
  `DataValue.source_timestamp` age vs maxAge, trigger fresh sample/callback when required; keep
  passing maxAge through to user callbacks. Document the in-memory always-current contract where
  the sinks discard it (`async-opcua-nodes/src/variable.rs:613`). Part 4 §5.11.2.2.
- [ ] T015 [US4] Update FINDINGS.md P4-ATTR-02 row → FIXED (scoped per register note) with
  evidence; commit story 4.

---

## Phase 6: US5 — EURange refresh + SemanticsChanged, P8-02 (Priority: P3) — Part 8 §5.2, §5.3.2.2

**Goal**: EURange property writes re-parameterize percent-deadband filters and set the
`SemanticsChanged` StatusCode bit exactly once on the next notification per affected item.

**Independent test**: percent-deadband item; write EURange; drive values across old/new deadband
thresholds (contracts §US5).

- [ ] T016 [P] [US5] Claude: red-first tests — unit in
  `async-opcua-server/src/subscriptions/monitored_item.rs` (mod tests :1036, overflow-bit test
  pattern :1418-1480) + integration `subscriptions.rs` (templates: `test_data_change_filters`
  :780, `modify_…_with_eurange_succeeds` :1051): filter follows the new range; exactly one
  notification carries `SemanticsChanged`; unrelated items unaffected; EURange-removed fail-safe.
  Part 8 §5.2, §5.3.2.2.
- [ ] T017 [US5] codex: event-driven refresh — signal at the address-space value-set layer (so
  BOTH client Writes and server-side value updates fire it) when the changed node is an `EURange`
  property of a monitored Variable → `SubscriptionCache` → affected
  `MonitoredItem`s re-resolve the range (reuse `modify()` seam :476-497 / `get_eu_range`
  machinery in `session/services/monitored_items.rs:35-98`) and arm a one-shot
  `semantics_changed` flag ORed into the next queued notification via
  `StatusCode::set_semantics_changed` (injection pattern: overflow bit,
  `monitored_item.rs:861-879`); remove the ponytail deferral comment (:398-400). Never re-read
  per-sample (O(changes), not O(samples)). Part 8 §5.2, §5.3.2.2.
- [ ] T018 [US5] Update FINDINGS.md P8-02 row → FIXED with evidence; commit story 5.

---

## Phase 7: US6 — AccessLevelEx attribute, P3-09 (Priority: P4) — Part 3 §5.6.2, §8.60

**Goal**: Variables serve attribute 27 (`AccessLevelExType` UInt32, low byte ≡ AccessLevel,
configurable extended bits); non-Variables unchanged.

**Independent test**: read attr 27 on plain + extended-bit Variables and on an Object
(contracts §US6).

- [ ] T019 [P] [US6] Claude: red-first tests — `read.rs`/`write.rs` integration + nodes-crate
  unit: low byte mirrors AccessLevel on any Variable; configured extended bit returned; Object →
  `Bad_AttributeIdInvalid`; set-attribute honors WriteMask bit 25. Part 3 §5.6.2, §8.60.
- [ ] T020 [US6] codex: `async-opcua-nodes/src/variable.rs` — store extended bits only (derive
  full value `(extended << 8) | access_level` so the low byte cannot diverge, research R6); read
  arm in `get_attribute_max_age` (:177-212), set arm in `set_attribute` (:215+), builder setter;
  write-mask mapping already exists (`utils.rs:107/:313`). Part 3 §5.6.2.
- [ ] T021 [US6] Update FINDINGS.md P3-09 row → FIXED with evidence; commit story 6.

---

## Phase 8: US7 — P5-03 verify-and-close (Priority: P4) — Part 5 §6.3.13 (Table 22), §6.3.14

**Goal**: register row closed **not-a-bug** (Phase-0 verification: `diagnostics/node_manager.rs`
:178/:418 serve Object NodeClass; properties are Variables :197/:463 — matches Part 5) with a
lock-in regression test.

**Independent test**: browse `Server → Namespaces → <ns>` asserting NodeClasses (contracts §US7).

- [ ] T022 [P] [US7] Claude: lock-in test in `async-opcua/tests/integration/browse.rs` (near
  `browse_multiple` :115): namespace child is an Object of `NamespaceMetadataType`; its property
  children are Variables with PropertyType. Part 5 §6.3.13/§6.3.14.
- [ ] T023 [US7] Update FINDINGS.md P5-03 row (+ banner) → not-a-bug with spec citation + test
  name; commit story 7.

---

## Phase 9: Polish & cross-cutting

- [ ] T024 Sweep `specs/conformance-audit/FINDINGS.md` reconciliation banner to final statuses;
  verify zero rows in sprint scope remain OPEN/PARTIAL/deferred (SC-001); note the §6.3.2→§6.3.3
  citation correction.
- [ ] T025 Full gate: `cargo test` workspace (ALL server-crate test binaries — feature-030 lesson),
  `cargo clippy --all-targets --all-features -- -D warnings` plus the no-default/json-off legs,
  `cargo fmt --check`; fix fallout; verify branch still `053-conformance-small-items-sprint`
  (codex worktree hazard).

## Dependencies

- T001 → everything.
- Within each story: Claude test task ∥ any pure-accessor codex task ([P]); impl tasks follow
  their story's test task (red-first); FINDINGS/commit task last.
- US1 internal order: T002 ∥ T003 → T004 → T005 → T006.
- Stories are mutually independent; executed sequentially in priority order (one commit each), any
  story is individually revertable. US2 and US5 both touch the write path (`utils.rs`) — land US2
  before US5 to avoid same-file churn.
- T024/T025 after all stories.

## Parallel example (US1)

    # In parallel after T001:
    codex → T002 (accessors: session/manager.rs, subscriptions/subscription.rs)
    Claude → T003 (tests: tests/integration/read.rs, browse.rs)

## Implementation strategy

MVP = US1 alone (the P1 story closes the largest gap and is independently shippable). Then US2 →
US7 in phase order, committing per story; stop-anywhere is safe. Estimated effort skews heavily to
US1; US7 is minutes.
