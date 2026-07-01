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
| SetTriggering (§5.12.5) | `.return_diagnostics` | (triggering result item) | `StatusCode` (add/remove) | `SetTriggeringResponse.{add,remove}_diagnostic_infos` |
| QueryFirst (Annex B) | `QueryFirstRequest.return_diagnostics` | `QueryRequest` (`query.rs`) | per-op result | `QueryFirstResponse.diagnostic_infos` + nested `data_diagnostic_infos` |
| QueryNext (Annex B) | `QueryNextRequest.return_diagnostics` | `QueryRequest` | per-op result | `QueryNextResponse.diagnostic_infos` |

> Exact result-type names to be confirmed against `opcua_types` during implementation; the shape (each work
> item yields `(Result, Option<DiagnosticInfo>)`) is fixed by `IntoResult`.

## Verification

- Grep gate: after the change, `rg "diagnostic_infos: None|n: None" async-opcua-server/src/session/services
  async-opcua-server/src/node_manager` returns no per-op response construction site among the services above
  (SetTriggering has two arrays; both handled).
- Per-service test: requested → `Some` aligned; not requested → `None` (see quickstart.md).
