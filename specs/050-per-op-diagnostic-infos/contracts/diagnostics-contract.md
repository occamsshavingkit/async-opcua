# Contract: Per-Operation diagnosticInfos

Authoritative per-service wiring. Every row ends on the shared
`consume_results(items, request_header.return_diagnostics)` emit path (`node_manager/utils/result.rs`).

## Behavior contract (all services)

| Client `returnDiagnostics` | Response per-op `diagnosticInfos` |
|---|---|
| requests per-op diagnostics (any operational bit set) | `Some(vec)` with `vec.len() == results.len()`, same order |
| `0` / no operational bit | `None` (unchanged from today) |

Per-entry `DiagnosticInfo` content is NOT populated by this feature (node-manager extension point via
`set_diagnostic_info`, unused by built-in node managers — same as Read/Call/Write today).

## Per-service map

| Service (§) | Request field | Work item (file) | `IntoResult::Result` | Response field |
|---|---|---|---|---|
| Browse (§5.8.2) | `BrowseRequest.return_diagnostics` | `BrowseNode` (`node_manager/view.rs`) | `BrowseResult` | `BrowseResponse.diagnostic_infos` |
| BrowseNext (§5.8.3) | `BrowseNextRequest.return_diagnostics` | `BrowseNode` | `BrowseResult` | `BrowseNextResponse.diagnostic_infos` |
| HistoryRead (§5.10.3) | `HistoryReadRequest.return_diagnostics` | `HistoryNode` (`history.rs`) | `HistoryReadResult` | `HistoryReadResponse.diagnostic_infos` |
| HistoryUpdate (§5.10.5) | `HistoryUpdateRequest.return_diagnostics` | `HistoryUpdateNode` | `HistoryUpdateResult` | `HistoryUpdateResponse.diagnostic_infos` |
| CreateMonitoredItems (§5.12.2) | `.return_diagnostics` | `MonitoredItemRef` (`monitored_items.rs`) | `MonitoredItemCreateResult` | `CreateMonitoredItemsResponse.diagnostic_infos` |
| ModifyMonitoredItems (§5.12.3) | `.return_diagnostics` | `MonitoredItemUpdateRef` | `MonitoredItemModifyResult` | `ModifyMonitoredItemsResponse.diagnostic_infos` |
| DeleteMonitoredItems (§5.12.6) | `.return_diagnostics` | `MonitoredItemRef` | `StatusCode` | `DeleteMonitoredItemsResponse.diagnostic_infos` |
| SetMonitoringMode (§5.12.4) | `.return_diagnostics` | `MonitoredItemRef` | `StatusCode` | `SetMonitoringModeResponse.diagnostic_infos` |
| SetTriggering (§5.12.5) | `.return_diagnostics` | none — plain status pairs (`session/message_handler.rs`) | `StatusCode` (add/remove) | `SetTriggeringResponse.{add,remove}_diagnostic_infos` |
| QueryFirst (Annex B.2.3) | `QueryFirstRequest.return_diagnostics` | `QueryRequest` (`query.rs`) | per-nodeType `ParsingResult` | `QueryFirstResponse.diagnostic_infos` (aligned to `node_types`, NOT to data sets) + nested `ParsingResult.data_diagnostic_infos` |
| ~~QueryNext~~ | — | — | — | **OUT OF SCOPE**: `QueryNextResponse` has no `diagnostic_infos` field (Part 4 Annex B.2.4: response is only `responseHeader` + `queryDataSets` + `revisedContinuationPoint`). Verified against the generated type and the spec 2026-07-01. |

## T001 verification results (confirmed against the code, 2026-07-01)

Result-type names confirmed. Three corrections to the original assumptions, found by reading the actual
handlers:

1. **`impl IntoResult` does not fit every work item directly.** `BrowseNode::into_result(self, nm_index,
   nm_count, &mut Session)` and `HistoryNode::into_result(self, &mut Session)` are session-dependent
   (continuation points), and Browse writes `results[input_index]` out of request order; `browse_next`
   pre-fills invalid-continuation-point slots that never become `BrowseNode`s. **Sanctioned mechanism**: add
   ONE additive blanket impl in `utils/result.rs` — `impl<T> IntoResult for (T, Option<DiagnosticInfo>)` —
   and have handlers assemble `Vec<(Result, Option<DiagnosticInfo>)>` in final positional order, then emit
   via the unchanged `consume_results(pairs, return_diagnostics)`. Work items whose inherent `into_result`
   is already zero-arg (`HistoryUpdateNode` → `HistoryUpdateResult`, `MonitoredItemUpdateRef` →
   `MonitoredItemModifyResult`) get the diagnostic slot + a direct `impl IntoResult` per the original plan.
   Everything still ends on the single `consume_results` emit path (Constitution II).
2. **`MonitoredItemRef` gets no diagnostic slot.** Node managers only ever receive it behind `&`
   (`&[&MonitoredItemRef]`), so a `set_diagnostic_info(&mut self)` extension point would be unreachable dead
   API. SetMonitoringMode/Delete results are `Vec<(StatusCode, MonitoredItemRef)>`; CreateMonitoredItems
   results come from the subscription cache (`Vec<MonitoredItemCreateResult>`, confirmed built in request
   order). The mutable extension point for Create is `CreateMonitoredItem`
   (`subscriptions/monitored_item.rs`, node managers get `&mut`), which gets the slot; the handler zips its
   diagnostics with the cache results. SetMonitoringMode/Delete emit `(StatusCode, None)` pairs.
3. **SetTriggering lives in `session/message_handler.rs`** (`fn set_triggering`, ~line 619), not
   `session/services/monitored_items.rs`; it has no work items (plain `Vec<StatusCode>` pairs from
   `subscriptions.set_triggering`). Emit both arrays via `consume_results` on `(StatusCode, None)` pairs.

Confirmed `IntoResult::Result` types: Browse/BrowseNext→`BrowseResult`; HistoryRead→`HistoryReadResult`;
HistoryUpdate→`HistoryUpdateResult`; CreateMonitoredItems→`MonitoredItemCreateResult`;
ModifyMonitoredItems→`MonitoredItemModifyResult`; Delete/SetMonitoringMode/SetTriggering→`StatusCode`;
QueryFirst→`ParsingResult` (op level, aligned to `node_types`; spec B.2.3: "diagnostic information for the
requested NodeTypeDescription") + nested `data_diagnostic_infos` aligned to each `ParsingResult`'s
`data_status_codes`.

## Verification

- Grep gate: after the change, `rg "diagnostic_infos: None|n: None" async-opcua-server/src/session/services
  async-opcua-server/src/node_manager` returns no per-op response construction site among the services above
  (SetTriggering has two arrays; both handled).
- Per-service test: requested → `Some` aligned; not requested → `None` (see quickstart.md).
