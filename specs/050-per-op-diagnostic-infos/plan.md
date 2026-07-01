# Implementation Plan: Per-Operation diagnosticInfos Completion (P4-GEN-01)

**Branch**: `050-per-op-diagnostic-infos` | **Date**: 2026-07-01 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/050-per-op-diagnostic-infos/spec.md`

## Summary

Complete conformance finding **P4-GEN-01**: the server honors `returnDiagnostics` per-operation
`diagnosticInfos` for Read/Call/Write/NodeManagement but still hardcodes an absent (`None`) per-op array for
**Browse/BrowseNext**, **HistoryRead/HistoryUpdate**, the **MonitoredItems** service group, and
**Query**. Apply the *existing* `IntoResult` + `consume_results(items, return_diagnostics)` mechanism
(`node_manager/utils/result.rs`) to each remaining service so the per-op array is returned **positionally
aligned with results when the client requests it, and `None` when not** — matching the precedent exactly. This
is structural (array presence gated on bits); `DiagnosticInfo` per-entry *content* stays the node-manager
extension point and is unchanged. Purely additive: results/status/order/sizes unchanged.

## Technical Context

**Language/Version**: Rust 1.75+ workspace
**Primary Dependencies**: existing — `opcua_types` (`DiagnosticBits`, `DiagnosticInfo`); the in-crate
`IntoResult`/`consume_results`/`filter_diagnostic_info` helpers. No new dependency.
**Storage**: N/A (in-memory request handling)
**Testing**: `cargo test -p async-opcua-server` (lib + integration binaries); new per-service integration
tests using raw `UARequest` builders (return_diagnostics set vs zero).
**Target Platform**: Linux CI + dev
**Project Type**: Rust workspace OPC UA server library
**Performance Goals**: neutral — one extra `unzip`/alloc only when diagnostics are requested; the default
(`bits.is_empty()`) path is unchanged.
**Constraints**: single-server behavior byte-for-byte unchanged when `returnDiagnostics = 0`; no change to
results, status codes, ordering, or array sizes; no new hot-path lock; network-reachable code stays
panic-free and allocation-bounded.
**Scale/Scope**: 4 service groups / ~6 work-item types (`BrowseNode`, `HistoryNode`, `HistoryUpdateNode`,
`MonitoredItemRef`, `MonitoredItemUpdateRef`, `QueryRequest`) + their service handlers; ~5 files under
`node_manager/` and `session/services/` + tests.

## OPC UA Standard Grounding

- **Part 4 §5.2** (`RequestHeader.returnDiagnostics`) and **§5.3** (common response params): the per-operation
  `diagnosticInfos` list matches the size and order of the operation results, and is empty when the client did
  not request per-operation diagnostics. Per-entry content is returned "if available".
- Per-service sections: **§5.8** (Browse/BrowseNext), **§5.10** (HistoryRead/HistoryUpdate), **§5.12/§5.13**
  (MonitoredItems: Create/Modify/Delete, SetMonitoringMode, SetTriggering), **Annex B / §5.10** (Query).
- This feature only changes the presence/alignment of the per-op array (gated on `returnDiagnostics`); no wire
  format, decode, crypto, or status-code semantics change.

## Constitution Check

*GATE: pass before Phase 0; re-check after Phase 1.*

- **I. Correctness Over Completion**: PASS. "Done" requires red-first tests per service (array absent before,
  present+aligned after) AND the `returnDiagnostics = 0` no-regression assertion; the full existing suite must
  stay green. The scope was corrected during research (verify-before-fix): the precedent is structural array
  presence, not content — so the spec asserts alignment/gating, not fabricated content.
- **II. Do It Right Once**: PASS. Reuses the single shared `IntoResult`/`consume_results` path rather than a
  second diagnostics mechanism; every remaining service ends on the same emit path as Read/Call/Write.
- **III. Individual Task Discipline**: PASS. One work-item type / service per task; the four user stories map
  to independent per-service slices, each with its own test.
- **IV. Security Is Paramount**: PASS. No decode/crypto/auth change. The extra array is built only when the
  client sets the bits, is exactly `results.len()` long (bounded by the already-bounded operation count), and
  the entries are default `DiagnosticInfo` (no attacker-influenced allocation, no new recursion). No panic
  path added.
- **V. Leave It Better Than You Found It**: PASS. Removes the last `n: None` / `diagnostic_infos: None`
  hardcodes so `returnDiagnostics` is honored uniformly; adds the missing per-service coverage; documents the
  node-manager content extension point.

**Result: PASS.** Complexity Tracking empty.

## Project Structure

### Documentation (this feature)

```text
specs/050-per-op-diagnostic-infos/
├── spec.md · plan.md · research.md · data-model.md · quickstart.md
├── contracts/diagnostics-contract.md   # per-service: work item → slot/IntoResult → consume_results wiring
├── checklists/requirements.md
└── tasks.md                            # (/speckit-tasks output)
```

### Source Code (repository root)

```text
async-opcua-server/src/
├── node_manager/
│   ├── view.rs             # BrowseNode: add diagnostic_bits + diagnostic_info + IntoResult
│   ├── history.rs          # HistoryNode, HistoryUpdateNode: same
│   ├── monitored_items.rs  # MonitoredItemRef, MonitoredItemUpdateRef: same
│   ├── query.rs            # QueryRequest (+ nested data_diagnostic_infos): same, dual-level
│   └── utils/result.rs     # UNCHANGED — reused (IntoResult, consume_results, filter_diagnostic_info)
└── session/services/
    ├── view.rs             # browse/browse_next → consume_results(nodes, return_diagnostics)
    ├── (history handler)   # history_read/history_update → consume_results
    ├── monitored_items.rs  # create/modify/delete/set-mode/set-triggering → consume_results
    └── query.rs            # query_first/query_next → consume_results (op-level + data-level)

async-opcua-server/tests/
└── per_op_diagnostics.rs   # new: per-service requested-vs-not integration tests (raw UARequest builders)
```

**Structure Decision**: the per-op diagnostic slot belongs on the node-manager work items (where
`ReadNode`/`WriteValue` already carry it), and the emit happens in the session service handlers (where
`consume_results` is already called for the done services). No new module; `utils/result.rs` is reused
verbatim.

## Phase 0 Research Summary

See [research.md](./research.md). Key decisions:

- **R1 — structural scope**: return the aligned per-op array gated on `returnDiagnostics`; do NOT populate
  `DiagnosticInfo` content (`set_diagnostic_info` has zero callers; the done services emit empty entries —
  content is a node-manager extension point). This corrected the spec's initial "populated content" wording.
- **R2 — add the slot to each work item** (`BrowseNode`, `HistoryNode`, `HistoryUpdateNode`,
  `MonitoredItemRef`, `MonitoredItemUpdateRef`, `QueryRequest`): `diagnostic_bits` + `diagnostic_info` fields,
  thread `return_diagnostics` into `new(...)`, add getter/setter, impl `IntoResult`.
- **R3 — Query has two levels** (operation-level + nested `data_diagnostic_infos`): apply the same bits-gated
  structural rule at both.
- **R4 — Browse assembly**: collect `BrowseNode`s through `consume_results`; continuation-point logic
  unchanged (only affects `BrowseResult`, returned as `IntoResult::Result`).
- **R5 — testing**: per-service raw-builder integration test; assert `diagnostic_infos.is_some()` and
  `len == results.len()` when requested, `is_none()` when not; content not asserted (matches precedent).

## Phase 1 Design Summary

- [data-model.md](./data-model.md): the per-work-item field additions, the `IntoResult` impls, and the
  service-handler emit changes; the invariant `diagnostic_infos.len() == results.len()` when present.
- [contracts/diagnostics-contract.md](./contracts/diagnostics-contract.md): the authoritative per-service
  table (service → request field → work item → `IntoResult::Result` → response field) and the requested/not
  behavior, plus the grep verification that no `n: None` / `diagnostic_infos: None` remains.
- [quickstart.md](./quickstart.md): the red-first per-service test pattern.

**Post-Design Constitution Re-check: PASS** — additive, reuses the shared path, no new global/lock, no new
panic or unbounded allocation, behavior-preserving when diagnostics not requested.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| None | N/A | N/A |
