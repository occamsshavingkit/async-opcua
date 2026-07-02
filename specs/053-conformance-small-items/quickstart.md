# Quickstart: Conformance Small-Items Sprint (053)

## Build & test

```bash
cargo build --workspace --all-features
cargo test -p async-opcua-server                 # ALL server-crate test binaries (not just --lib)
cargo test -p async-opcua --test integration_tests
cargo clippy --all-targets --all-features -- -D warnings
# before push, also the CI legs local --all-features misses:
cargo clippy -p async-opcua --no-default-features --features server -- -D warnings
```

Integration tests use the in-process test harness (`async-opcua/tests/utils/`); no external
server needed. The interop harness (`samples/demo-server/interop/`) is a smoke check only for US1.

## Where each story lands

| Story | Impl area | Test home |
|---|---|---|
| US1 P5-04 | `async-opcua-server/src/diagnostics/server.rs`, `node_manager/memory/core.rs`, `session/manager.rs`, `subscriptions/` | `tests/integration/read.rs` (`test_diagnostics`), `browse.rs` |
| US2 P4-ATTR-04 | `address_space/utils.rs` (`validate_node_write` Value arm) | `tests/integration/write.rs`, `utils.rs mod tests` |
| US3 P4-ATTR-03 | `address_space/utils.rs` (locale side-table rules) | `tests/integration/read.rs`/`write.rs`, `utils.rs mod tests` |
| US4 P4-ATTR-02 | `node_manager/memory/simple.rs` (callback/sampler freshness) | new callback-source tests + `read.rs` |
| US5 P8-02 | `subscriptions/monitored_item.rs`, write path → `SubscriptionCache` | `monitored_item.rs mod tests`, `tests/integration/subscriptions.rs` |
| US6 P3-09 | `async-opcua-nodes/src/variable.rs`, `address_space/utils.rs` | `read.rs`/`write.rs` |
| US7 P5-03 | none expected (verify-and-close) | `tests/integration/browse.rs` lock-in |

## Definition of done (per story)

1. Implementation + independent spec-grounded tests green (red-first where fixing behavior).
2. `specs/conformance-audit/FINDINGS.md` row updated (status, Part/§, named tests) — including the
   reconciliation banner table.
3. One commit per story on `053-conformance-small-items-sprint`.
4. Full workspace test suite + clippy legs green before push.

## Spec grounding

Use the opc-ua-reference MCP (`search_text`/`search_nodes` with docNumber OPC-10000-3/-4/-5/-8).
Key sections: Part 5 §6.3.3 (Table 11), §6.3.13/§6.3.14; Part 4 §5.11.2.2 (Table 47), §5.11.4.1;
Part 3 §5.6.2, §8.60; Part 8 §5.2, §5.3.2.2, §5.3.3.3/.4.
