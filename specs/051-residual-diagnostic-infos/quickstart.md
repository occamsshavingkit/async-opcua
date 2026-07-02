# Quickstart: Residual diagnosticInfos Tests

Same red-first pattern as feature 050 (`specs/050-per-op-diagnostic-infos/quickstart.md`), extending
`async-opcua-server/tests/per_op_diagnostics.rs` and its `TestServer` harness.

Per-service: (1) requested (any operational bit) → `diagnostic_infos.expect(...)` with
`len == results.len()`; (2) `DiagnosticBits::empty()` → `is_none()`. Do NOT assert entry content.

Service-specific setup:
- **TranslateBrowsePaths**: 2 paths (valid + no-match) → results len 2.
- **SetPublishingMode / DeleteSubscriptions**: 1 valid + 1 bogus subscription id → len 2.
- **TransferSubscriptions**: transfer own subscription → len 1 (status irrelevant, alignment only).
- **Publish**: subscription w/ 100ms publishing interval; PublishRequest with 1 bogus ack; response on
  first keep-alive → results Some(len 1). Also assert the no-acks case yields no diag array.
- **Call nested**: GetMonitoredItems with a bad-typed input arg → input_argument_results populated →
  nested array gated/aligned.
- **HistoryUpdate nested**: history-backend server (in-memory history harness) so UpdateData produces
  operation_results; assert nested gating. If backend setup proves disproportionate, assert the
  axis-absent rule (operation_results None → nested None even when requested) and document.
- **EventFilter nested**: CreateMonitoredItems (EventNotifier) with an EventFilter containing one bad
  select clause → decode filter_result → EventFilterResult.select_clause_diagnostic_infos gated,
  aligned to select_clause_results. Cover create AND modify.
- **RegisterServer2**: secured-channel discovery harness (cert-bound serverUri, P4-DISC-03);
  RegisterServer2 with 1 discovery configuration → configuration_results len 1.

## No-regression gate

`cargo test -p async-opcua-server` (all binaries), workspace `check --all-targets`, clippy
all-features/default/no-default, `cargo fmt --all` — clean; existing tests unchanged.
