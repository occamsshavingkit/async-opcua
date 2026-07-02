# Contract: Residual diagnosticInfos (P4-GEN-04)

Behavior contract identical to feature 050 (`specs/050-per-op-diagnostic-infos/contracts/`): requested
(any bit) → `Some(vec)` aligned with the results axis, same order; not requested → `None`. Content not
populated.

## Per-site map

| Site (§) | Results axis | Emit point | Mechanism |
|---|---|---|---|
| TranslateBrowsePaths (§5.9.4) | `results[]` (one per browse path) | `session/services/view.rs` handler | slot on `BrowsePathItem` (roots carry it); zip root diags with results → `consume_results` |
| SetPublishingMode (§5.14.4) | `results[]` StatusCode | `session_subscriptions.rs::set_publishing_mode` | `(status, None)` pairs → `consume_results` |
| Publish (§5.14.5.2) | ack `results[]` (absent when no acks) | both `PublishResponseShared` wire sites in `session_subscriptions.rs` | `ack_results: Some(v)` → pairs → `consume_results`; `None` → `(None, None)` |
| TransferSubscriptions (§5.14.7) | `results[]` TransferResult | `subscriptions/mod.rs::transfer` | pairs → `consume_results` |
| DeleteSubscriptions (§5.14.8) | `results[]` StatusCode | `session/services/subscriptions.rs` handler | pairs → `consume_results` |
| RegisterServer2 (§5.5.6) | `configurationResults[]` | `session/controller.rs` | `Some(v)` → pairs → `consume_results` |
| ActivateSession (§5.7.3.2) | `results[]` for swCerts | — | **N/A** — reserved field, `results` never produced; FINDINGS documents |
| Call nested (§5.12.2.2) | `inputArgumentResults[]` | `MethodCall::into_result` (`node_manager/method.rs`) | bits non-empty && args non-empty → `Some(vec![default; len])` |
| HistoryUpdate nested (§5.10.5) | `operationResults[]` | `HistoryUpdateNode::into_result` (`node_manager/history.rs`) | bits non-empty && `Some(v)` → `Some(vec![default; v.len()])` |
| EventFilter nested (§7.22.3) | `selectClauseResults[]` | `FilterType::from_filter` (`subscriptions/monitored_item.rs`) — create AND modify paths | bits param; post-process returned `EventFilterResult` |

## Verification

- Re-run the 050 grep gate: no wire-response construction site among the above still hardcodes the
  diagnostics field to `None` (test modules + documented N/A excluded).
- Per-site test: requested → `Some` aligned; not requested → `None` (see quickstart.md).
