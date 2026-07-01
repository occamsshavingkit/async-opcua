# Tasks: Per-Operation diagnosticInfos Completion (P4-GEN-01)

**Feature**: `050-per-op-diagnostic-infos` | **Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

Tests are IN SCOPE (spec defines explicit per-service test scenarios). Per project practice, implementation
tasks (I) are codex-dispatchable one at a time; independent test tasks (T) are authored separately by Claude
and anchored to the observable contract (aligned array present when requested, absent when not), NOT to
implementation internals.

**Reference precedent** (do not modify, read for the pattern): `ReadNode`/`WriteValue` in
`async-opcua-server/src/node_manager/attributes.rs`; shared emit in
`async-opcua-server/src/node_manager/utils/result.rs` (`IntoResult`, `consume_results`).

**Shared contract for every implementation task**: add `diagnostic_bits: DiagnosticBits` +
`diagnostic_info: Option<DiagnosticInfo>` to the work item, thread `request.request_header.return_diagnostics`
into its `new(...)` (update every call site), add `diagnostic_bits()` getter + `set_diagnostic_info()` setter,
`impl IntoResult`, and switch the service handler from `diagnostic_infos: None` to
`consume_results(items, return_diagnostics)`. Do NOT populate `DiagnosticInfo` content (node-manager
extension point — matches the done services).

---

## Phase 1: Setup

- [X] T001 Confirm the shared mechanism is reusable and unchanged: read `async-opcua-server/src/node_manager/utils/result.rs` (`IntoResult`, `consume_results`, `filter_diagnostic_info`) and the `ReadNode` precedent in `async-opcua-server/src/node_manager/attributes.rs`; record the exact result-type name each affected work item must use for `IntoResult::Result` (Browse→`BrowseResult`, HistoryRead→`HistoryReadResult`, HistoryUpdate→`HistoryUpdateResult`, MonitoredItems create/modify/delete/set-mode/set-triggering, Query per-op) in `specs/050-per-op-diagnostic-infos/contracts/diagnostics-contract.md` (fill the "to be confirmed" note).

## Phase 2: Foundational

*None.* The shared `IntoResult`/`consume_results` path already exists and is used by Read/Call/Write/NodeMgmt;
no blocking prerequisite. Each user story is independent and can be implemented/tested on its own.

---

## Phase 3: User Story 1 — Browse & BrowseNext honor returnDiagnostics (P1) 🎯 MVP

**Goal**: Browse and BrowseNext return an aligned per-op `diagnosticInfos` array when requested, `None` when not.
**Independent test**: US1 test task below — passes only after the US1 impl tasks.

- [X] T002 [US1] Add the diagnostic slot to `BrowseNode` in `async-opcua-server/src/node_manager/view.rs`: `diagnostic_bits` + `diagnostic_info` fields, thread `diagnostic_bits` through `BrowseNode::new(...)`, add `diagnostic_bits()` getter + `set_diagnostic_info()` setter, and `impl IntoResult for BrowseNode { type Result = BrowseResult; .. }` returning `(self.<result>, self.diagnostic_info)`.
- [X] T003 [US1] Route Browse and BrowseNext through the shared emit in `async-opcua-server/src/session/services/view.rs`: pass `request.request_header.return_diagnostics` into each `BrowseNode::new(...)` call site, and replace the `diagnostic_infos: None` in the `BrowseResponse`/`BrowseNextResponse` construction with the array from `consume_results(nodes, return_diagnostics)`. Continuation-point logic unchanged.
- [X] T004 [P] [US1] Independent test in `async-opcua-server/tests/per_op_diagnostics.rs` (new file): for Browse and BrowseNext, assert that with an operational `return_diagnostics` bit set the response `diagnostic_infos` is `Some` with `len() == results.len()`, and with `DiagnosticBits::empty()` it is `None`. Use raw `UARequest` builders. Red-first (Browse case fails before T002/T003).

**Checkpoint**: US1 independently deliverable — Browse/BrowseNext honor returnDiagnostics; suite green.

---

## Phase 4: User Story 2 — MonitoredItems services honor returnDiagnostics (P2)

**Goal**: Create/Modify/Delete MonitoredItems, SetMonitoringMode, SetTriggering return aligned per-op arrays when requested.
**Independent test**: US2 test task; independent of US1.

- [X] T005 [US2] *(revised per T001)* Add the diagnostic slot (`diagnostic_bits` + `diagnostic_info` + getter/setter) to `CreateMonitoredItem` in `async-opcua-server/src/subscriptions/monitored_item.rs` (the mutable extension point for Create; `MonitoredItemRef` gets NO slot — node managers only see it behind `&`), threading `return_diagnostics` through its `new(...)`.
- [X] T006 [US2] ~~slot on `MonitoredItemUpdateRef`~~ **CLOSED BY T001 REASONING**: like `MonitoredItemRef`, node managers only receive `&[&MonitoredItemUpdateRef]` (immutable, `ServiceCbRef` in the modify handler), so a `set_diagnostic_info(&mut self)` extension point would be unreachable dead API. Modify emits `(into_result(), None)` pairs via the shared `consume_results` in T007b instead; observable contract identical.
- [X] T007a [US2] Route CreateMonitoredItems and DeleteMonitoredItems in `async-opcua-server/src/session/services/monitored_items.rs` through `consume_results(items, return_diagnostics)`, replacing their `n: None` / `diagnostic_infos: None` (Create→`MonitoredItemCreateResult`, Delete→`StatusCode`).
- [X] T007b [US2] Route ModifyMonitoredItems in `async-opcua-server/src/session/services/monitored_items.rs` through `consume_results(items, return_diagnostics)`, replacing its `diagnostic_infos: None` (result `MonitoredItemModifyResult`).
- [X] T007c [US2] Route SetMonitoringMode in `async-opcua-server/src/session/services/monitored_items.rs` through `consume_results(items, return_diagnostics)`, replacing its `diagnostic_infos: None` (result `StatusCode`).
- [X] T007d [US2] *(revised per T001)* Route SetTriggering in `async-opcua-server/src/session/message_handler.rs` (`fn set_triggering`, ~line 619 — NOT services/monitored_items.rs) through the shared emit for BOTH its add and remove arrays (`add_diagnostic_infos` + `remove_diagnostic_infos`), via `(StatusCode, None)` pairs into `consume_results`.
- [X] T008 [P] [US2] Independent test in `async-opcua-server/tests/per_op_diagnostics.rs`: for each of the five MonitoredItems services, assert aligned `diagnostic_infos` present when requested (`len() == results.len()`; SetTriggering: both add & remove arrays aligned to their respective request arrays) and `None` when not. Raw `UARequest` builders. Red-first.

**Checkpoint**: US2 independently deliverable.

---

## Phase 5: User Story 3 — HistoryRead & HistoryUpdate honor returnDiagnostics (P3)

**Goal**: HistoryRead and HistoryUpdate return aligned per-op arrays when requested.

- [ ] T009 [US3] Add the diagnostic slot + `IntoResult` to `HistoryNode` (result: `HistoryReadResult`) and `HistoryUpdateNode` (result: `HistoryUpdateResult`) in `async-opcua-server/src/node_manager/history.rs`, threading `diagnostic_bits` through each `new(...)`.
- [ ] T010 [US3] Route the HistoryRead and HistoryUpdate handlers in `async-opcua-server/src/session/services/attribute.rs` through `consume_results(items, return_diagnostics)`, replacing the `diagnostic_infos: None` in `HistoryReadResponse`/`HistoryUpdateResponse`.
- [ ] T011 [P] [US3] Independent test in `async-opcua-server/tests/per_op_diagnostics.rs`: for HistoryRead and HistoryUpdate, assert aligned `diagnostic_infos` present when requested and `None` when not. Raw `UARequest` builders. Red-first.

**Checkpoint**: US3 independently deliverable.

---

## Phase 6: User Story 4 — Query honors returnDiagnostics (P3)

**Goal**: QueryFirst and QueryNext return aligned per-op arrays (operation-level and nested `data_diagnostic_infos`) when requested.

- [ ] T012 [US4] Add the diagnostic slot + `IntoResult` to `QueryRequest` in `async-opcua-server/src/node_manager/query.rs`, threading `diagnostic_bits` through its `new(...)`; note the dual level (operation-level per-node + nested per-QueryDataSet `data_diagnostic_infos`).
- [ ] T013 [US4] Route QueryFirst and QueryNext in `async-opcua-server/src/session/services/query.rs` through `consume_results(items, return_diagnostics)` for the operation-level array, and gate the nested `data_diagnostic_infos` on the same bits (aligned when requested, `None` when not), replacing the `data_diagnostic_infos: None` sites.
- [ ] T014 [P] [US4] Independent test in `async-opcua-server/tests/per_op_diagnostics.rs` for QueryFirst: assert (a) the **operation-level** `diagnostic_infos` is aligned+present when requested and `None` when not, and (b) the **nested per-QueryDataSet** `data_diagnostic_infos` follows the same bits-gated rule (present/aligned to its data sets when requested, `None` when not). Raw `UARequest` builders. Red-first.
- [X] T014b [US4] ~~QueryNext test~~ **CLOSED BY T001 (verify-before-fix)**: `QueryNextResponse` has no `diagnostic_infos` field — Part 4 Annex B.2.4 defines the response as only `responseHeader` + `queryDataSets` + `revisedContinuationPoint`, and the generated type matches. There is nothing to emit or test for QueryNext; coverage documented in contracts/diagnostics-contract.md.

**Checkpoint**: US4 independently deliverable.

---

## Phase 7: Polish & Cross-Cutting

- [ ] T015 Grep gate: confirm no per-op response construction among the affected services still hardcodes `diagnostic_infos: None` / `n: None` — `rg "diagnostic_infos: None|n: None" async-opcua-server/src/session/services async-opcua-server/src/node_manager` shows none for Browse/BrowseNext/History*/MonitoredItems*/Query (SetTriggering both arrays; Query both levels). Update `specs/conformance-audit/FINDINGS.md` P4-GEN-01 row from PARTIAL to FIXED with the per-service list.
- [ ] T016 Full gate: `cargo test -p async-opcua-server` (lib + all integration binaries), `cargo clippy -p async-opcua-server --all-features` and default features, `cargo fmt --all` — all clean; confirm no result/status/ordering/size change in existing tests.

---

## Dependencies & Execution Order

- **T001** (setup) before all implementation.
- Within each story: work-item task(s) → handler task(s) → test task. (e.g. US1: T002 → T003 → T004; US2:
  T005/T006 → T007a→T007b→T007c→T007d → T008.)
- **Stories are independent** and may be done in any order / parallel by different workers (US1 P1 first as MVP).
- Within US2, the four handler tasks (T007a–d) all touch `monitored_items.rs`, so run them **sequentially**
  (not `[P]`) to avoid same-file contention; each depends on the work-item tasks T005/T006.
- **T004/T008/T011/T014/T014b** are `[P]` — all live in the one new test file but target disjoint services; if
  one worker owns that file, run them after their story's impl; if parallelizing, split by service to avoid
  file contention.
- **T015/T016** (polish) after all stories.

## Implementation Strategy

- **MVP = US1** (Browse/BrowseNext, highest-use service). Ship, then US2 (MonitoredItems), then US3/US4.
- One codex dispatch per implementation task (T002/T003; T005/T006/T007a/T007b/T007c/T007d; T009/T010;
  T012/T013). Claude authors the test tasks (T004/T008/T011/T014/T014b) independently, anchored to the
  observable contract.
- Commit locally at the end of each user story (per the standing commit cadence); one PR at the end,
  unsquashed, so per-story commits remain.

## Task Summary

- **Total**: 20 tasks (1 setup, 0 foundational, 15 across 4 user stories, 2 polish; tests embedded per story).
- **US1**: T002–T004 (3) · **US2**: T005, T006, T007a–d, T008 (7) · **US3**: T009–T011 (3) ·
  **US4**: T012, T013, T014, T014b (4).
- **Parallel**: the 5 test tasks are `[P]` by service; stories are mutually independent; US2's handler tasks
  T007a–d are sequential (same file).
- **Independent test criteria**: each story's response `diagnostic_infos` is `Some` and length-aligned with
  results when `returnDiagnostics` requests per-op diagnostics, and `None` when it does not (Query also checks
  the nested `data_diagnostic_infos`; SetTriggering checks both add & remove arrays).
