# Phase 1 Data Model: Per-Operation diagnosticInfos Completion

No new types. Each remaining per-op **work item** gains the same slot `ReadNode`/`WriteValue` already have, and
each remaining **service handler** switches its response's per-op `diagnosticInfos` from a hardcoded `None` to
the output of the shared `consume_results`.

## Per-work-item additions (mirror `ReadNode`)

For each of `BrowseNode` (`node_manager/view.rs`), `HistoryNode` + `HistoryUpdateNode` (`history.rs`),
`MonitoredItemRef` + `MonitoredItemUpdateRef` (`monitored_items.rs`), and `QueryRequest` (`query.rs`):

| Element | Shape | Notes |
|---|---|---|
| field `diagnostic_bits` | `DiagnosticBits` | set from `request.request_header.return_diagnostics` in `new(...)` |
| field `diagnostic_info` | `Option<DiagnosticInfo>` | default `None`; node-manager extension point (not populated here) |
| ctor `new(item, .., diagnostic_bits)` | add the param | update **every** call site in the matching service handler |
| getter `diagnostic_bits(&self) -> DiagnosticBits` | | parity with `ReadNode` |
| setter `set_diagnostic_info(&mut self, DiagnosticInfo)` | | parity; unused by built-in NMs, kept for extension |
| `impl IntoResult` | `type Result = <the service result type>`; `into_result(self) -> (Self::Result, Option<DiagnosticInfo>)` | returns `(existing result, self.diagnostic_info)` |

`Self::Result` per work item:
- `BrowseNode` → `BrowseResult`
- `HistoryNode` → `HistoryReadResult`
- `HistoryUpdateNode` → `HistoryUpdateResult`
- `MonitoredItemRef` → `MonitoredItemCreateResult` (create) / `StatusCode` (delete/set-mode) as the handler
  currently produces
- `MonitoredItemUpdateRef` → `MonitoredItemModifyResult`
- `QueryRequest` → the QueryFirst/QueryNext per-op result element

## Service-handler change (mirror the Read handler)

Each handler currently builds `results` and sets `diagnostic_infos: None`. Replace with:

```
let (results, diagnostic_infos) = consume_results(items, request.request_header.return_diagnostics);
```

and place `diagnostic_infos` into the response's `diagnostic_infos` field. `results` is `Option<Vec<..>>`
(already how the done services thread it).

Affected handlers: `session/services/view.rs` (`browse`, `browse_next`), the history read/update handler,
`session/services/monitored_items.rs` (create / modify / delete / set-monitoring-mode / set-triggering),
`session/services/query.rs` (`query_first`, `query_next`).

## Query dual level

`QueryFirst`/`QueryNext` carry both an operation-level per-node `diagnosticInfos` and a nested per-QueryDataSet
`data_diagnostic_infos`. Both are gated on the same `return_diagnostics` bits: aligned array when requested,
`None` when not. Operation-level goes through `consume_results`; the nested `data_diagnostic_infos` follows the
same bits-gated rule (aligned-empty when requested, `None` otherwise).

## Invariants

- **INV-1**: when the response's per-op `diagnosticInfos` is `Some`, its `len()` equals the results `len()`
  and its order matches (guaranteed by `consume_results`' `unzip` over the same item vector).
- **INV-2**: when `return_diagnostics.is_empty()`, the per-op `diagnosticInfos` is `None` (guaranteed by the
  `bits.is_empty()` branch of `consume_results`) — byte-for-byte identical to today.
- **INV-3**: results, status codes, ordering, and array sizes are unchanged relative to the pre-change handler
  (the `Self::Result` returned by `into_result` is exactly the value the handler produced before).
