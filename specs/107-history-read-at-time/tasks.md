---

description: "Task list for feature 107: Historical ReadAtTimeDetails (HistoryRead at-time queries)"
---

# Tasks: Historical ReadAtTimeDetails

**Input**: Design documents from `/specs/107-history-read-at-time/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Single user story (P1).

## Path Conventions

New backend method `read_raw_reverse` on `async-opcua-server/src/history/backend.rs`'s
`HistoryStorageBackend` trait, overridden in `async-opcua-server/src/history/data_history.rs`
(`InMemoryDataHistory`) and `async-opcua-history-sqlite/src/backend.rs` (`SqliteHistoryBackend`).
New `read_at_time` default method on the same trait. New override in
`async-opcua-server/src/node_manager/memory/simple.rs`. New end-to-end test file
`async-opcua-server/tests/history_read_at_time.rs`.

---

## Phase 1: Setup

- [X] T001 Re-verify (do not trust the summary in research.md alone) the exact Part 11 §6.5.5.2 and Part 13 §3.1.8/§3.1.9 wording against `~/opcua-specs/OPC 10000-11 - UA Specification Part 11 - Historical Access 1.05.04.pdf` and `~/opcua-specs/OPC 10000-13 - UA Specification Part 13 - Aggregates 1.05.07.pdf` via `pdftotext -layout`, before writing any code. Confirm `StatusCode::BadTimestampNotSupported` (0x80A10000) is the correct code for FR-006, and confirm `StatusCodeValueType::{Raw, Interpolated}` (`async-opcua-types/src/status_code.rs:517-521`, accessed via `value_type()`/`set_value_type()` at lines 254-263) is the correct InfoBits mechanism (research.md R2, R5).

---

## Phase 2: Foundational

- [X] T002 Make `interpolated_bound_at` (`async-opcua-server/src/aggregates/engine.rs:~1366`) `pub(crate)` (it is currently private `fn`) so `history/backend.rs` can call it. No behavior change; add a one-line doc comment noting the new caller if none exists.
- [X] T003 [P] Add `read_raw_reverse` to `HistoryStorageBackend` (`async-opcua-server/src/history/backend.rs`) as a new **default** method (not required, non-breaking per research.md R7): `async fn read_raw_reverse(&self, node_id: &NodeId, at_or_before: DateTime, num_values_per_node: u32) -> Result<Vec<DataValue>, StatusCode>`, defaulting to `Err(StatusCode::BadHistoryOperationUnsupported)`.
- [X] T004 [P] Override `read_raw_reverse` on `InMemoryDataHistory` (`async-opcua-server/src/history/data_history.rs`): `self.raw_values.read().get(node_id)` then `.range(..=at_or_before.ticks()).rev().take(num_values_per_node as usize)`, cloning matched `DataValue`s in descending order. Handle `num_values_per_node == 0` the same way `read_raw_modified` already does (no limit / treat as unbounded -- match existing convention exactly).
- [X] T005 [P] Override `read_raw_reverse` on `SqliteHistoryBackend` (`async-opcua-history-sqlite/src/backend.rs`): reuse `query::fetch_interval(conn, node_id, at_or_before.ticks(), i64::MIN, /*chronological=*/false, None, limit)` (`async-opcua-history-sqlite/src/query.rs:43`, already exactly the "descending, at-or-before" query shape), wrapped in the same `tokio::task::spawn_blocking` + connection-pool pattern `fetch_raw_modified_page` (backend.rs:314) already uses. No new SQL query needed -- `fetch_interval`'s non-chronological branch already implements this.
- [X] T006 Add unit tests for both new `read_raw_reverse` overrides (in-memory: `async-opcua-server/src/history/data_history.rs` test module; SQLite: `async-opcua-history-sqlite/src/backend.rs` or its existing test module) proving: exact-match-at-boundary inclusion, correct descending order, correct truncation at `num_values_per_node`, empty result when nothing exists at or before the timestamp, and identical results between both backends for the same seeded data (a shared test fixture/table asserting both backends agree, if such a cross-backend harness already exists for `read_raw_modified` -- reuse it; else two parallel tests are fine).

**Checkpoint**: `read_raw_reverse` compiles, is unit-tested, and behaves identically on both backends before any `read_at_time` code is written against it.

---

## Phase 3: User Story 1 - Client reads historical values at arbitrary timestamps (Priority: P1) 🎯 MVP

**Goal**: Real, spec-correct `ReadAtTimeDetails` (OPC-10000-11 §6.5.5.2) against both history
backends, closing CU 3020 (and CU 2991 as a byproduct).

### Implementation for User Story 1

- [X] T007 [US1] Implement `HistoryStorageBackend::read_at_time` default method (`async-opcua-server/src/history/backend.rs`), gated `#[cfg(feature = "history-aggregates")]` matching `read_processed`'s existing gating shape (lines 85-172): signature `async fn read_at_time(&self, node_id: &NodeId, req_times: &[DateTime], use_simple_bounds: bool, stepped: bool, continuation_point: Option<Vec<u8>>) -> Result<(Vec<DataValue>, Option<Vec<u8>>), StatusCode>`. Per the real Part 11 §6.5.5.2 text ("The standard ContinuationPoint rules (see 6.3) apply") -- re-verified in T001, correcting an earlier wrong assumption made during analysis (research.md R9) -- genuinely support pagination: decode a supplied `continuation_point` into a resume index into `req_times` (`Err(BadContinuationPointInvalid)` if it doesn't decode), process up to a fixed internal batch-size constant's worth of timestamps starting from that index (bounding per-call work regardless of `req_times.len()`, a defensive measure since `req_times` is client-controlled and has no client-specified limit field of its own to honor), and if timestamps remain after the batch, encode+return the next resume index as the continuation token. For each requested timestamp in the current batch: (a) query a small forward window via `read_raw_modified` to check for an exact match and find the nearest `after` point; (b) query via the new `read_raw_reverse` to find the nearest `before` point; (c) if `use_simple_bounds`, use `before`/`after` as-is (`Bad_NoData` if either required side is missing or itself `Bad`-quality, per FR-004); if not, search outward (bounded, paginating `read_raw_modified`/`read_raw_reverse` with an increasing limit only as far as needed, not an unbounded scan) for the nearest *non-Bad* point on each side (FR-005); (d) if `stepped`, resolve to `before`'s value verbatim (any `Variant` type, closing CU 2991 per research.md R4/R8); if not `stepped`, resolve numerically via `interpolated_bound_at` (only meaningful for numeric `before`/`after`); (e) set `StatusCodeValueType::Raw` for exact matches, `::Interpolated` for computed values, `Bad_NoData` where undetermined.
- [X] T008 [US1] In the `#[cfg(not(feature = "history-aggregates"))]` branch of `read_at_time`, return `Err(StatusCode::BadHistoryOperationUnsupported)` for every requested timestamp (and `Err(BadContinuationPointInvalid)` if a continuation point was supplied, matching `read_processed`'s existing precedence of that check), matching `read_processed`'s off-feature behavior exactly (backend.rs:101-116).
- [X] T009 [US1] Implement `SimpleNodeManagerImpl::history_read_at_time` (`async-opcua-server/src/node_manager/memory/simple.rs`, new override alongside the existing 4 `history_read_*` overrides at lines 477/548/601/661): resolve `stepped` per node via `crate::aggregates::resolve_stepped` exactly as `history_read_processed` does (simple.rs:569-583), then delegate to the backend's `read_at_time` per node, passing that node's `HistoryNode::input_continuation_point` (`async-opcua-server/src/node_manager/history.rs:18`) through and setting `next_continuation_point` (line 19) from the returned token per T007's new `(Vec<DataValue>, Option<Vec<u8>>)` return shape. Reject (FR-006) any node/`TimestampsToReturn` combination the node doesn't support with `Bad_TimestampNotSupported`, independent of the other requested timestamps in the same call (per-node, not per-timestamp, since `TimestampsToReturn` is a whole-request setting per HistoryReadValueId -- verify this against `HistoryNode`'s existing per-node status field, `async-opcua-server/src/node_manager/history.rs:21`, before assuming per-timestamp granularity).

### Tests for User Story 1

- [X] T010 [P] [US1] Unit tests for `read_at_time`'s core cases (in `async-opcua-server/src/history/backend.rs`'s own test module, or a new `#[cfg(test)] mod tests` if none exists yet): exact-timestamp match (`Raw`), interpolated match under `use_simple_bounds=false` (search-outward-past-Bad), interpolated match under `use_simple_bounds=true` (immediate-neighbor-only, `Bad_NoData` when the immediate neighbor is itself `Bad`), `Bad_NoData` outside the recorded range on either side, multiple requested timestamps in one call each resolving independently (FR-001/FR-005 edge case: duplicate/out-of-order timestamps), a structured/non-numeric historized value resolving correctly via the `Stepped` step-hold path (CU 2991, research.md R8), an invalid/garbage continuation point rejected with `Bad_ContinuationPointInvalid`, and (if the internal batch-size constant from T007 can be exercised without an unreasonably large test fixture) a `req_times` array spanning more than one batch resolving correctly across two calls chained by the returned continuation token (research.md R9).
- [X] T011 [US1] New `async-opcua-server/tests/history_read_at_time.rs`: real client + real server, record raw values via the existing HistoryUpdate write path, then issue a real `HistoryRead(ReadAtTimeDetails)` Call through the wire and assert the returned values/StatusCode InfoBits match what was recorded, for at least one exact-match and one interpolated-match timestamp. Mirror the connection/session setup pattern of the nearest existing history integration test in this crate's `tests/` directory (check `async-opcua-server/tests/` for an existing HistoryRead end-to-end test to copy the scaffold from, e.g. one used by feature 032/034/035, rather than writing connection setup from scratch).
- [X] T012 [US1] Run T006/T010/T011; all pass.

**Checkpoint**: A real client can HistoryRead `ReadAtTimeDetails` end-to-end against both shipped
backends and get spec-correct Raw/Interpolated/Bad_NoData results.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [X] T013 `cargo test -p async-opcua-server --all-features` and `cargo test -p async-opcua-history-sqlite --all-features`: 0 failures. `cargo build -p async-opcua-server --no-default-features --features history` (history-aggregates disabled): confirm `history_read_at_time` correctly returns `Bad_HistoryOperationUnsupported` rather than failing to compile or panicking.
- [X] T014 Update `TODO.md`: remove/narrow the "Historical `ReadAtTimeDetails`" entry; if CU 2991 is confirmed closed by T010's structured-data test, remove its mention from that entry too (it's currently listed only as a related-but-separate item, not under this exact bullet -- check TODO.md's actual current wording before editing, don't assume the phrasing from this feature's own input prompt is verbatim what's in the file).
- [X] T015 Add a one-line TODO.md note (per plan.md's Constitution Check, Principle V) documenting the R3 finding: `agg_interpolative`/`compute_processed_intervals` do not currently search outward past Bad-quality points when resolving interval-boundary values, unlike what Part 13 §3.1.8 describes for genuine Interpolated Bounding Values -- a pre-existing aggregate-engine gap, out of this feature's scope to fix.
- [X] T016 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CU `3020` (`Gap` -> `Implemented`) and CU `2991` (update per T010's confirmed outcome).
- [X] T017 [P] Mirror T016 into `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T018 `cargo clippy --all-targets --all-features` and `cargo fmt --all` (workspace-wide) -- clean.
- [X] T019 Run the full local CI gate; resolve or triage any failures before opening the PR.

---

## Dependencies & Execution Order

Phase 2 (the new `read_raw_reverse` primitive on both backends) blocks Phase 3, since `read_at_time`
cannot search backward without it. T003 blocks T004/T005 (both override the same new trait method).
T007 depends on T002-T006 all being green. Polish (T013-T019) depends on Phase 3 being complete.

## Implementation Strategy

1. T001 (re-verify spec grounding) -> confirms the exact rules before any code is written.
2. T002-T006 (the new `read_raw_reverse` primitive, both backends, unit-tested in isolation) ->
   validated compiles and behaves identically cross-backend before building on top of it.
3. T007-T009 (`read_at_time` default + `SimpleNodeManagerImpl` wiring) -> validated compiles.
4. T010-T012 (tests, incl. real end-to-end HistoryRead round trip) -> validated green.
5. T013-T019 (regression, docs, CI gate) -> PR.
