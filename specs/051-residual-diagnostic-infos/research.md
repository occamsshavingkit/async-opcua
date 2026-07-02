# Phase 0 Research: Residual diagnosticInfos (P4-GEN-04)

All decisions verified against code + spec (opc-ua-reference MCP) on 2026-07-02, continuing feature
050's mechanism (see `specs/050-per-op-diagnostic-infos/contracts/diagnostics-contract.md`).

## R1 — Site inventory verified (shapes confirmed by reading each handler)

| Site | Shape | Bits reachable? | Emit strategy |
|---|---|---|---|
| TranslateBrowsePaths (`session/services/view.rs:375`) | one root `BrowsePathItem::new_root(p, i)` per path, in order; node managers get `&mut chunk`; results built from `items[..paths.len()]` + merged `final_results` | header at handler | slot on `BrowsePathItem` (+ bits param on `new_root`/`new`); zip root items' diags with final `results` → `consume_results` |
| SetPublishingMode (`session_subscriptions.rs:321`) | response built inside `set_publishing_mode(&request)`; `results: Vec<StatusCode>` | `request.request_header` in scope | `(status, None)` pairs → `consume_results` |
| DeleteSubscriptions (`session/services/subscriptions.rs:13`) | `delete_subscriptions_inner -> Vec<StatusCode>` | header at handler | pairs → `consume_results` |
| TransferSubscriptions (`subscriptions/mod.rs:1450`) | response built in `transfer(...)`; `results: Vec<(u32, TransferResult)>` → `.1` | `req.request_header` in scope | pairs → `consume_results` |
| Publish (`session_subscriptions.rs:799` + `:1032`) | `PublishResponseShared { results: publish_request.ack_results (Option<Vec<StatusCode>>), diagnostic_infos: None }` | `publish_request.request.request_header` | when `ack_results` `Some` → pairs → `consume_results`; when `None` → both `None` |
| RegisterServer2 (`session/controller.rs:770`) | `configuration_results: Option<Vec<StatusCode>>`; service-level `apply_return_diagnostics` already called | `request.request_header` | when `Some` → pairs → `consume_results` |
| ActivateSession (`session/manager.rs:1234`) | `results: None` always | — | **N/A**: §5.7.3.2 `clientSoftwareCertificates` "Reserved for future use"; nothing to align. Documented closure |
| Call nested (`node_manager/method.rs:88`) | `MethodCall` already has the 050 slot; `into_result` hardcodes `input_argument_diagnostic_infos: None`; `argument_results: Vec<StatusCode>` emitted `Some` when non-empty | `self.diagnostic_bits` | in `into_result`: bits non-empty && non-empty args → `Some(vec![DiagnosticInfo::default(); len])` |
| HistoryUpdate nested (`node_manager/history.rs:290`) | `HistoryUpdateNode` has the 050 slot; `into_result` hardcodes `diagnostic_infos: None`; `operation_results: Option<Vec<StatusCode>>` | `self.diagnostic_bits` | bits non-empty && `Some(v)` → `Some(vec![default; v.len()])` |
| EventFilter nested (`services/subscription/filter.rs:34`) | `EventFilterResult` built in public `ParsedEventFilter::parse`; both consumers go through `FilterType::from_filter` (`subscriptions/monitored_item.rs:108`), called from `CreateMonitoredItem::new` (:227, HAS bits since 050) and `MonitoredItem::modify` (:475, needs bits threaded) | via `from_filter` bits param | add `DiagnosticBits` param to `from_filter`; post-process the returned `EventFilterResult` there (avoids changing public `ParsedEventFilter::parse`); thread bits into the modify path (`modify_monitored_items` chain) |

## R2 — Gating rule

Identical to `consume_results`: emit when `!bits.is_empty()` (any bit, incl. service-level-only —
matches the Read/Write precedent), `None` when empty. Nested entries are `DiagnosticInfo::default()`
(filtering default entries is a no-op).

## R3 — Publish "no acks" case

§5.14.5.2: results "size and order matches the acknowledgements". No acks → `results: None` →
`diagnostic_infos: None` even when requested. The `#[cfg(test)]` allocation-bench construction in
`subscriptions/subscription.rs` is not a wire path — out of scope.

## R4 — Test harnesses

- TranslateBrowsePaths/SetPublishingMode/DeleteSubscriptions/TransferSubscriptions: extend
  `tests/per_op_diagnostics.rs` (raw builders; TestServer harness from 050).
- Publish: create subscription → send `PublishRequest` with one bogus acknowledgement → response
  arrives on the first keep-alive tick (publishing interval 100ms); `results: Some(len 1)`.
- Call nested: `CallRequest` on a standard server method (e.g. GetMonitoredItems) with a bad-typed
  argument → `input_argument_results` populated → nested gating observable.
- HistoryUpdate nested: requires a history backend to populate `operation_results` — reuse the
  in-memory history harness pattern (`tests/history_data_inmemory.rs`) or assert the `None`-axis rule
  plus a backend-driven positive case if cheap.
- EventFilter nested: CreateMonitoredItems with an EventFilter containing an invalid select clause →
  `filter_result` decodes to `EventFilterResult` with `select_clause_results` populated.
- RegisterServer2: secured-channel discovery harness (registration requires cert-bound URI per
  P4-DISC-03 fix) — reuse the existing discovery test setup.

## R5 — What was REJECTED

- Slot on `QueryRequest`-style whole-request items: N/A here; every slot added (BrowsePathItem) is
  `&mut`-visible to node managers. No slot for pure status vectors (SetPublishingMode etc.) — nothing
  can populate them (same dead-API rationale as 050's MonitoredItemRef decision).
- Changing public `ParsedEventFilter::parse` signature: post-process in crate-internal
  `FilterType::from_filter` instead.
