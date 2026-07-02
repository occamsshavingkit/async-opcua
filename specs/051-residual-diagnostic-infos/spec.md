# Feature Specification: Residual diagnosticInfos Completion (P4-GEN-04)

**Feature Branch**: `051-residual-diagnostic-infos`
**Created**: 2026-07-02
**Status**: Draft
**Input**: Conformance finding P4-GEN-04 — per-op/nested diagnostics sites the original audit never
enumerated, surfaced by feature 050's grep gate.

## Context

Feature 050 closed P4-GEN-01: every *audit-enumerated* service now returns per-operation
`diagnosticInfos` positionally aligned with its results when `RequestHeader.returnDiagnostics` requests
diagnostics, and `None` when not, all via the shared `consume_results` emit path. The closing grep gate
found the remaining hardcoded `None` sites, recorded as finding **P4-GEN-04**:

**Per-op response arrays** (Part 4 §5.2/§5.3 general rule — list matches size and order of the results,
empty when not requested):
- TranslateBrowsePathsToNodeIds (§5.9.4) — `session/services/view.rs`
- SetPublishingMode (§5.14.4) — `subscriptions/session_subscriptions.rs`
- Publish acknowledgement results (§5.14.5.2) — `subscriptions/session_subscriptions.rs` (2 sites)
- TransferSubscriptions (§5.14.7) — `subscriptions/mod.rs`
- DeleteSubscriptions (§5.14.8) — `session/services/subscriptions.rs`
- RegisterServer2 `configurationResults` (§5.5.6) — `session/controller.rs`
- ActivateSession (§5.7.3.2) — `session/manager.rs` — **verified N/A**: `clientSoftwareCertificates`
  are "reserved for future use"; the server never produces `results`, so there is nothing to align.
  Documented closure only.

**Nested arrays** (same bits-gating rule as feature 050's Query `data_diagnostic_infos` precedent):
- `CallMethodResult.input_argument_diagnostic_infos` (§5.12.2.2 — "corresponding to the inputArguments")
  — `node_manager/method.rs`
- `HistoryUpdateResult.diagnostic_infos` (§5.10.5 — aligned to `operationResults`) —
  `node_manager/history.rs`
- `EventFilterResult.select_clause_diagnostic_infos` (§7.22.3 — aligned to the select clauses) —
  `services/subscription/filter.rs`

As in 050 this is **structural**: array presence/alignment gated on the bits; per-entry
`DiagnosticInfo` *content* remains the node-manager extension point and is not populated. Behavior with
`returnDiagnostics = 0` stays byte-for-byte unchanged.

## User Scenarios & Testing

### User Story 1 — TranslateBrowsePaths honors returnDiagnostics (P1)

A client translating browse paths with `returnDiagnostics` set receives a `diagnosticInfos` array
aligned with `results` (one per requested browse path).

**Acceptance**:
1. Given N browse paths (mixed outcomes) and an operational bit set, the response `diagnostic_infos`
   is `Some` with `len == results.len() == N`.
2. Given the same request with `returnDiagnostics = 0`, `diagnostic_infos` is `None`.

### User Story 2 — Subscription service set honors returnDiagnostics (P1)

SetPublishingMode, DeleteSubscriptions, TransferSubscriptions, and Publish (ack results) return
aligned per-op arrays when requested.

**Acceptance** (per service):
1. Requested → `diagnostic_infos` `Some`, `len == results.len()` (Publish: aligned with the
   acknowledgement results; absent when the request carried no acknowledgements).
2. Not requested → `None`.

### User Story 3 — Nested diagnostic arrays honor returnDiagnostics (P2)

The three nested arrays follow the same rule at their own alignment axis:
- Call: `input_argument_diagnostic_infos` aligned with `input_argument_results` when that list is
  populated and diagnostics requested; `None` otherwise.
- HistoryUpdate: `HistoryUpdateResult.diagnostic_infos` aligned with `operation_results` when
  populated and requested; `None` otherwise.
- MonitoredItem filters: `EventFilterResult.select_clause_diagnostic_infos` aligned with
  `select_clause_results` when populated and requested; `None` otherwise.

### User Story 4 — RegisterServer2 honors returnDiagnostics; ActivateSession documented N/A (P3)

RegisterServer2's `diagnostic_infos` aligns with `configuration_results` when requested. ActivateSession
requires no change (results never produced — reserved field); the finding row documents this.

### Edge Cases

- Publish with no acknowledgements: `results` is `None` → `diagnostic_infos` stays `None` even when
  requested (nothing to align, matching §5.14.5.2 "size and order matches the acknowledgements").
- Nested arrays whose status-code list is absent (e.g. successful parse, no `operation_results`):
  nested diagnostics stay `None` — alignment axis is the status list, not the request.
- All emits gate on `!bits.is_empty()` exactly like `consume_results` (service-level-only bits also
  produce the array of empty entries — matching the Read/Write precedent from 050).

## Functional Requirements

- **FR-001**: TranslateBrowsePathsToNodeIds MUST return `diagnostic_infos` aligned with `results` when
  requested, `None` when not. `BrowsePathItem` gains the diagnostic slot (node managers receive it
  `&mut`); the array is emitted from the per-path root items via `consume_results`.
- **FR-002**: SetPublishingMode MUST return the aligned array when requested (`session_subscriptions.rs`).
- **FR-003**: DeleteSubscriptions MUST return the aligned array when requested.
- **FR-004**: TransferSubscriptions MUST return the aligned array when requested.
- **FR-005**: Publish MUST return `diagnostic_infos` aligned with the acknowledgement `results` when
  requested and acknowledgements were present, at every `PublishResponseShared` wire construction site.
- **FR-006**: RegisterServer2 MUST return `diagnostic_infos` aligned with `configuration_results` when
  requested.
- **FR-007**: `CallMethodResult.input_argument_diagnostic_infos` MUST be `Some` aligned with
  `input_argument_results` when that list is populated and diagnostics requested; `None` otherwise.
- **FR-008**: `HistoryUpdateResult.diagnostic_infos` MUST be `Some` aligned with `operation_results`
  when populated and requested; `None` otherwise.
- **FR-009**: `EventFilterResult.select_clause_diagnostic_infos` MUST be `Some` aligned with
  `select_clause_results` when populated and requested (create and modify paths); `None` otherwise.
- **FR-010**: All emits reuse the shared mechanism (`consume_results` / the same `bits.is_empty()`
  gate); per-entry content is NOT populated; `returnDiagnostics = 0` behavior is byte-for-byte
  unchanged; no result/status/order/size changes.
- **FR-011**: ActivateSession is closed as documented-N/A in FINDINGS.md (no code change).

## Success Criteria

- **SC-001**: Each in-scope service/nested array has a red-first integration test: requested → `Some`
  + aligned length; not requested → `None`.
- **SC-002**: The 050 grep gate re-run shows no remaining hardcoded per-op/nested
  `diagnostic_infos: None` among wire-response construction sites (test-module sites excluded),
  except documented N/A (ActivateSession).
- **SC-003**: Full server suite, workspace check, clippy (all-features/default/no-default), fmt all
  clean; no existing test changes behavior.
- **SC-004**: FINDINGS.md P4-GEN-04 → FIXED with the per-site list.

## Assumptions

- `PublishResponseShared` (opcua-core) already carries `diagnostic_infos`; only construction sites
  change — no type change.
- RegisterServer2 testing uses the existing secured-channel discovery test harness (P4-DISC-03
  requires cert-bound registration).
- The `subscription.rs` allocation-bench `PublishResponseShared` site is in a `#[cfg(test)]` module —
  out of scope.
