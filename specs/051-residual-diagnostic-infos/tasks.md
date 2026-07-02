# Tasks: Residual diagnosticInfos Completion (P4-GEN-04)

**Feature**: `051-residual-diagnostic-infos` | **Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

Same division of labor as 050: implementation tasks are codex-dispatchable one at a time (no-git
guardrail, no test authoring); Claude authors the red-first tests independently, anchored to the
observable contract. Shared mechanism: `consume_results` + blanket `IntoResult` for
`(T, Option<DiagnosticInfo>)` (both exist since 050). Gate rule everywhere: `!bits.is_empty()`.

## Phase 1: Setup

- [X] T101 Re-verify the site inventory against current master (research.md R1 table): each listed
  file/line still matches; confirm no NEW hardcoded sites appeared since the 050 merge. Update the
  contract if drift found.

## Phase 2: Foundational

*None* — mechanism exists.

---

## Phase 3: US1 — TranslateBrowsePaths (P1) 🎯 MVP

- [X] T102 [US1] Slot on `BrowsePathItem` in `async-opcua-server/src/node_manager/view.rs`
  (Part 4 §5.9.4, §5.2/§5.3): `diagnostic_bits` + `diagnostic_info` fields, bits param on `new_root(...)`
  AND `new(...)` (continuation items inherit the same request's bits), `diagnostic_bits()` getter +
  `set_diagnostic_info()` setter (ReadNode-style docs) + `pub(crate) take_diagnostic_info()`. NOTE:
  `#[derive(Clone)]`-compat; placeholder `DiagnosticBits::empty()` at call sites.
- [X] T103 [US1] Route `translate_browse_paths` in `async-opcua-server/src/session/services/view.rs`:
  real header bits into both constructors; zip the per-path root items' (`items[..paths.len()]`)
  `take_diagnostic_info()` with the final `results` (AFTER the BadNoMatch post-processing loop) →
  `consume_results` → both response fields.
- [X] T104 [P] [US1] Independent red-first test in `tests/per_op_diagnostics.rs`: 2 paths (one
  resolvable, one no-match) → requested: `Some`, len==2; empty bits: `None`.

**Checkpoint**: US1 deliverable.

---

## Phase 4: US2 — Subscription service set (P1)

- [X] T105 [US2] SetPublishingMode (§5.14.4): in
  `async-opcua-server/src/subscriptions/session_subscriptions.rs::set_publishing_mode`, emit
  `(StatusCode, None)` pairs via `consume_results(pairs, request.request_header.return_diagnostics)`.
- [X] T106 [US2] DeleteSubscriptions (§5.14.8): in
  `async-opcua-server/src/session/services/subscriptions.rs`, same pairs treatment.
- [X] T107 [US2] TransferSubscriptions (§5.14.7): in `async-opcua-server/src/subscriptions/mod.rs`
  `transfer(...)`, pairs from `results.into_iter().map(|r| (r.1, None))` → `consume_results` with
  `req.request_header.return_diagnostics`.
- [X] T108 [US2] Publish (§5.14.5.2): BOTH wire `PublishResponseShared` sites in
  `session_subscriptions.rs` (~:799 deliver_pending_status_changes, ~:1032 main delivery): when
  `publish_request.ack_results` is `Some(v)` → pairs → `consume_results` with
  `publish_request.request.request_header.return_diagnostics`; when `None` → both `None`. The
  `#[cfg(test)]` bench site in `subscriptions/subscription.rs` is out of scope.
- [X] T109 [P] [US2] Independent red-first tests in `tests/per_op_diagnostics.rs`: SetPublishingMode
  (1 valid + 1 bogus id, len 2), DeleteSubscriptions (same), TransferSubscriptions (own sub, len 1),
  Publish (bogus ack, results Some(len 1) on first keep-alive; plus requested-but-no-acks → `None`).

**Checkpoint**: US2 deliverable.

---

## Phase 5: US3 — Nested arrays (P2)

- [X] T110 [US3] Call nested (§5.12.2.2): in `async-opcua-server/src/node_manager/method.rs`
  `MethodCall::into_result`, when `!self.diagnostic_bits.is_empty()` and `argument_results` non-empty →
  `input_argument_diagnostic_infos: Some(vec![DiagnosticInfo::default(); len])`, else `None`.
- [X] T111 [US3] HistoryUpdate nested (§5.10.5): in `async-opcua-server/src/node_manager/history.rs`
  `HistoryUpdateNode::into_result`, same rule against `operation_results`.
- [X] T112 [US3] EventFilter nested (§7.22.3): add `DiagnosticBits` param to `FilterType::from_filter`
  (`async-opcua-server/src/subscriptions/monitored_item.rs`); post-process the returned
  `EventFilterResult` (bits non-empty && `select_clause_results` `Some(v)` →
  `select_clause_diagnostic_infos: Some(vec![default; v.len()])`). Thread bits from
  `CreateMonitoredItem::new` (has them) AND the modify path (`MonitoredItem::modify` ←
  `session_subscriptions::modify_monitored_items` ← handler — add a bits param through the chain;
  the handler has the header). Do NOT change public `ParsedEventFilter::parse`.
- [X] T113 [P] [US3] Independent red-first tests: Call (GetMonitoredItems w/ bad-typed arg →
  input_argument_results populated → nested gated/aligned); EventFilter (create + modify w/ one bad
  select clause → decode filter_result → nested gated/aligned); HistoryUpdate (history-backend server
  so UpdateData yields operation_results; if disproportionate, axis-absent assertion + doc note).

**Checkpoint**: US3 deliverable.

---

## Phase 6: US4 — RegisterServer2 + ActivateSession closure (P3)

- [ ] T114 [US4] RegisterServer2 (§5.5.6): in `async-opcua-server/src/session/controller.rs`, when
  `configuration_results` is `Some(v)` → pairs → `consume_results` with the request header bits
  (service-level `apply_return_diagnostics` already handled — keep it).
- [ ] T115 [P] [US4] Independent red-first test via the secured-channel discovery harness:
  RegisterServer2 w/ 1 discovery configuration → requested: `Some(len 1)`; empty bits: `None`.
- [ ] T116 [US4] ActivateSession documented closure: no code change; covered in the P4-GEN-04
  FINDINGS row update (T117).

**Checkpoint**: US4 deliverable.

---

## Phase 7: Polish

- [ ] T117 Grep gate re-run (050 pattern): no wire-response per-op/nested diagnostics `None` remains
  except documented N/A. Update FINDINGS.md P4-GEN-04 → FIXED (per-site list, ActivateSession N/A
  rationale).
- [ ] T118 Full gate: `cargo test -p async-opcua-server`, workspace `check --all-targets`, clippy
  all-features/default/no-default, `cargo fmt --all` — clean.

## Dependencies & Strategy

T101 first. Within stories: impl → test. Stories independent; US1 MVP first, then US2/US3/US4.
T105/T108 + T112's modify-threading all touch `session_subscriptions.rs` — run those sequentially.
One codex dispatch per impl task; Claude authors T104/T109/T113/T115. Commit per user story; one
unsquashed PR at the end.
