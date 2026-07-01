# Phase 0 Research: Per-Operation diagnosticInfos Completion (P4-GEN-01)

Current-code findings (verified 2026-07-01) and decisions.

## Existing mechanism (the precedent to reuse)

`async-opcua-server/src/node_manager/utils/result.rs`:
- `trait IntoResult { type Result; fn into_result(self) -> (Self::Result, Option<DiagnosticInfo>); }`
- `consume_results<T: IntoResult>(items, bits) -> (Option<Vec<T::Result>>, Option<Vec<DiagnosticInfo>>)`:
  - `bits.is_empty()` → `(Some(results), None)` — **no** per-op array (matches "empty/absent when not requested").
  - else → `(Some(results), Some(diagnostics))` where each `DiagnosticInfo` is run through
    `filter_diagnostic_info(bits, ..)` to strip fields the client did not request.

Per-op work items that already implement this (the "done" services): `ReadNode`, `WriteValue`
(`node_manager/attributes.rs`), `MethodCall` (`method.rs`), and the four NodeManagement items
(`node_management.rs`). Each carries `diagnostic_bits: DiagnosticBits` + `diagnostic_info: Option<DiagnosticInfo>`,
a `new(item, diagnostic_bits)` ctor, `diagnostic_bits()` getter, `set_diagnostic_info()` setter, and an
`IntoResult` impl. The service handler calls `consume_results(items, request.request_header.return_diagnostics)`
and puts the returned `diagnostic_infos` into the response.

## R1 — Scope is STRUCTURAL (aligned array gated on bits), not content population

**Decision**: match the precedent exactly — make each remaining service return the positionally-aligned per-op
`diagnosticInfos` array when the client requests it, and `None` when not. Do **not** populate `DiagnosticInfo`
content.

**Rationale**: `set_diagnostic_info()` has **zero callers** anywhere in the crate; the done services
(Read/Call/Write/NodeMgmt) emit an aligned array whose entries are `DiagnosticInfo::default()` (empty) unless a
node manager attaches detail. Content is a deliberate node-manager extension point. Part 4 §5.2/§5.3 require the
per-op list to match the size/order of the results and be empty when not requested; content is "if available".
So the conformance gap (P4-GEN-01) is the missing *array*, not missing content. Populating content uniformly
would (a) diverge from the done services and (b) be a much larger, separate effort.

**Alternatives rejected**: (a) synthesize `DiagnosticInfo` content from each op's `StatusCode` — inconsistent
with the done services, larger surface, and not required by the finding; defer to a future feature if ever
wanted. (b) return `Some(empty vec)` regardless of bits — wrong; violates "empty when not requested".

## R2 — Per-service work items need the slot added (none have it today)

Verified none of these carry the diagnostic slot yet:
- `node_manager/view.rs`: `BrowseNode` (Browse + BrowseNext share it).
- `node_manager/history.rs`: `HistoryNode` (HistoryRead), `HistoryUpdateNode` (HistoryUpdate).
- `node_manager/monitored_items.rs`: `MonitoredItemRef`, `MonitoredItemUpdateRef` (used by the 4 monitored-item
  services — Create/Modify/Delete + SetMonitoringMode/SetTriggering).
- `node_manager/query.rs`: `QueryRequest` (+ the nested `data_diagnostic_infos` for QueryDataSets).

**Decision**: for each work item, add `diagnostic_bits` + `diagnostic_info` fields, thread
`return_diagnostics` through its `new(...)` (updating every call site in the matching service handler), add a
`diagnostic_bits()` getter + `set_diagnostic_info()` setter, and implement `IntoResult`. Then switch the
service handler from `diagnostic_infos: None` to `consume_results(items, return_diagnostics)`.

**Rationale**: identical to the `ReadNode`/`WriteValue` precedent — least surprise, single shared emit path.

## R3 — Query has two diagnostic levels

`query.rs` exposes both an operation-level per-node `diagnosticInfos` **and** a nested per-QueryDataSet
`data_diagnostic_infos` (both currently `None`). §Annex B / §5.10.

**Decision**: honor the operation-level array via `consume_results` like the others. The nested
`data_diagnostic_infos` (per data set within a result) is gated on the same bits and set to an aligned array
when requested (empty entries), `None` when not — mirroring the operation-level rule. Keep it minimal and
consistent; no per-dataset content synthesis.

**Rationale**: same bits-gated, structural rule applied at both levels keeps Query consistent with the rest and
with §5.2/§5.3, without inventing content.

## R4 — Browse/BrowseNext result assembly

Browse builds `Vec<Option<BrowseResult>>` with continuation-point handling; BrowseNext reuses `BrowseNode`.
**Decision**: collect the `BrowseNode`s (which own their `BrowseResult`) and run them through `consume_results`
so results and the aligned `diagnostic_infos` come out together; the continuation-point logic is unchanged
(it only affects the `BrowseResult` content, which `IntoResult` returns as `Self::Result`).

## R5 — Testing approach

Per affected service, an integration test (raw `UARequest` builders, per the error-mode campaign lesson) that:
1. sends the request with `return_diagnostics` requesting per-op diagnostics → asserts
   `response.diagnostic_infos` is `Some` and `.len() == results.len()` (aligned);
2. sends the same request with `return_diagnostics = 0` → asserts `diagnostic_infos` is `None` (no regression).
Content is intentionally not asserted (matches precedent). Red-first: the "requested" case fails before the
change (array is `None`) and passes after.

**Independent-test isolation**: each service group is testable on its own; the tests need only a running
server with the default node managers.

## Open items

None — mechanism, scope (structural), and per-service work-item changes are confirmed against current code.
