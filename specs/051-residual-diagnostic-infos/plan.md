# Implementation Plan: Residual diagnosticInfos Completion (P4-GEN-04)

**Branch**: `051-residual-diagnostic-infos` | **Date**: 2026-07-02 | **Spec**: [spec.md](./spec.md)

## Summary

Close finding **P4-GEN-04** (the per-op/nested diagnostics sites feature 050's grep gate surfaced
beyond the original audit): TranslateBrowsePaths, the subscription service set (SetPublishingMode,
DeleteSubscriptions, TransferSubscriptions, Publish acks), RegisterServer2, and the three nested arrays
(Call input arguments, HistoryUpdateResult, EventFilterResult select clauses). Identical mechanism to
050: aligned array gated on `returnDiagnostics` via the shared `consume_results` path (or the same
`bits.is_empty()` rule for nested arrays); content stays the node-manager extension point;
`returnDiagnostics = 0` byte-for-byte unchanged. ActivateSession verified N/A (reserved field, no
results ever produced) — documentation-only closure.

## Technical Context

**Language/Version**: Rust 1.75+ workspace. **Dependencies**: existing only.
**Testing**: extend `async-opcua-server/tests/per_op_diagnostics.rs` (050 harness); secured-channel
discovery harness for RegisterServer2; history-backend harness for HistoryUpdate nested positive case.
**Performance**: neutral — extra alloc only when diagnostics requested.
**Constraints**: no result/status/order/size change; no new lock; panic-free network paths.
**Scale**: ~8 emit sites + 1 work-item slot (`BrowsePathItem`) + 1 bits-threading chain (modify-path
filters) across ~8 files + tests.

## OPC UA Standard Grounding

Part 4 §5.2/§5.3 (general per-op rule); §5.9.4 TranslateBrowsePaths; §5.14.4/.5/.7/.8 subscription
services (Publish diag aligned to ack results, §5.14.5.2); §5.5.6 RegisterServer2; §5.7.3.2
ActivateSession (clientSoftwareCertificates reserved); §5.12.2.2 CallMethodResult
inputArgumentDiagnosticInfos; §5.10.5 HistoryUpdateResult; §7.22.3 EventFilterResult.

## Constitution Check

- **I. Correctness Over Completion**: PASS — red-first per-site tests + no-regression assertion;
  ActivateSession closed by verification, not code.
- **II. Do It Right Once**: PASS — same single shared emit path as 050; no second mechanism.
- **III. Individual Task Discipline**: PASS — one site/service per task.
- **IV. Security Is Paramount**: PASS — arrays bounded by already-bounded result counts; default
  entries; no decode/crypto change; no new panic path.
- **V. Leave It Better**: PASS — finishes the returnDiagnostics surface; grep gate then holds globally.

**Result: PASS.** Complexity Tracking empty.

## Project Structure

```text
async-opcua-server/src/
├── node_manager/
│   ├── view.rs            # BrowsePathItem: slot (new_root/new gain bits param)
│   ├── method.rs          # MethodCall::into_result: nested input-arg array
│   └── history.rs         # HistoryUpdateNode::into_result: nested operation_results array
├── session/
│   ├── services/view.rs           # translate_browse_paths emit
│   ├── services/subscriptions.rs  # delete_subscriptions emit
│   ├── controller.rs              # RegisterServer2 emit
│   └── manager.rs                 # ActivateSession — NO CHANGE (documented N/A)
├── subscriptions/
│   ├── session_subscriptions.rs   # set_publishing_mode + 2× Publish emits; modify-path bits threading
│   ├── mod.rs                     # transfer emit
│   └── monitored_item.rs          # FilterType::from_filter gains bits; EventFilterResult post-process
└── tests/per_op_diagnostics.rs    # extended per-site tests (+ discovery/history harness reuse)
```

## Phase 0/1 Summary

See [research.md](./research.md) (R1 site inventory with verified shapes, R2 gating rule, R3 Publish
no-acks, R4 harnesses, R5 rejections) and
[contracts/residual-diagnostics-contract.md](./contracts/residual-diagnostics-contract.md).

**Post-Design Constitution Re-check: PASS.**
