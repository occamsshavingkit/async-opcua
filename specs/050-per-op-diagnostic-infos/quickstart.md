# Quickstart: Per-Operation diagnosticInfos Tests

Per-service integration test pattern (raw `UARequest` builders, per the error-mode campaign lesson — drive the
service directly rather than through Session helpers so `return_diagnostics` is controllable).

## Red-first pattern (one per affected service)

```rust
// 1) REQUESTED: per-op diagnostics asked for -> aligned array present.
let mut req = BrowseRequest { /* nodes_to_browse: N ops */ ..default };
req.request_header.return_diagnostics =
    DiagnosticBits::OPERATIONAL_LEVEL_SYMBOLIC_ID; // any operational bit
let resp = send(req).await;
let diags = resp.diagnostic_infos.expect("per-op diagnosticInfos present when requested"); // FAILS before change
assert_eq!(diags.len(), resp.results.as_ref().unwrap().len());       // aligned length + order

// 2) NOT REQUESTED: default -> no array (no regression).
let mut req0 = BrowseRequest { /* same nodes */ ..default };
req0.request_header.return_diagnostics = DiagnosticBits::empty();
let resp0 = send(req0).await;
assert!(resp0.diagnostic_infos.is_none());
```

Repeat for: `browse_next`, `history_read`, `history_update`, `create_monitored_items`,
`modify_monitored_items`, `delete_monitored_items`, `set_monitoring_mode`, `set_triggering`, `query_first`,
`query_next`. Use each service's own response type + `diagnostic_infos` field (SetTriggering has add/remove
arrays; QueryFirst also has nested `data_diagnostic_infos`).

## What NOT to assert

- Do **not** assert `DiagnosticInfo` *content* (symbolic_id, localized_text, ...). The built-in node managers
  leave entries default/empty, exactly like Read/Call/Write today. Asserting content would test behavior this
  feature deliberately does not add (it stays a node-manager extension point).

## No-regression gate

```
cargo test -p async-opcua-server                 # lib + all integration binaries
cargo clippy -p async-opcua-server --all-features # and default features
cargo fmt --all
```

The full existing suite must pass unchanged; the only new observable behavior is the aligned per-op array when
`returnDiagnostics` requests it.
