# Implementation Plan: Historical ReadAtTimeDetails

**Branch**: `107-history-read-at-time` | **Date**: 2026-07-18 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/107-history-read-at-time/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Implement OPC UA HistoryRead's `ReadAtTimeDetails` (Part 11 §6.5.5.2), closing CU 3020: given a
batch of arbitrary timestamps, return one value per timestamp -- the exact raw sample if one
exists at that timestamp (marked `Raw`), otherwise a value computed per Part 13's Interpolated/
Simple Bounding Value rules (marked `Interpolated`), or `Bad_NoData`/`Bad_TimestampNotSupported`
where no usable value can be determined. Built as a new default method on the existing
`HistoryStorageBackend` trait (mirroring `read_processed`'s established shape exactly), reusing
the aggregate engine's `interpolated_bound_at` ratio-interpolation math and `resolve_stepped`
per-node configuration lookup. One small, symmetrical addition to the same trait is genuinely
required alongside it: `read_raw_reverse` (nearest sample at-or-before a timestamp, closest
first) -- investigation found `read_raw_modified` alone cannot correctly/efficiently answer "the
nearest prior sample, however far back," and the in-memory backend silently ignores its own
`return_bounds` flag, so relying on that (as `read_processed` does) would violate this feature's
own cross-backend-parity requirement (research.md R7). Closes CU 2991 (structured-data
time-instance reads) as a byproduct, since the `Stepped` branch is inherently `Variant`-type-
agnostic (see research.md R4/R8).

## Technical Context

**Language/Version**: Rust (workspace MSRV, matches rest of `async-opcua-server`)
**Primary Dependencies**: `async-opcua-types` (generated `ReadAtTimeDetails`, `StatusCodeValueType`), existing `async-opcua-server::aggregates` module (`interpolated_bound_at`, `resolve_stepped`), existing `HistoryStorageBackend` trait
**Storage**: Reuses existing `HistoryStorageBackend::read_raw_modified`, plus a new default `read_raw_reverse` method overridden on both shipped backends (in-memory `async-opcua-server/src/history/data_history.rs`, SQLite `async-opcua-history-sqlite/src/backend.rs`) -- no schema change, no new tables/indexes needed (both backends' existing storage is already ordered by timestamp)
**Testing**: `cargo test` (unit tests in the backend/simple.rs modules + a real client/server end-to-end HistoryRead test), `cargo clippy --all-targets --all-features`, `cargo fmt --all -- --check`
**Target Platform**: Same as rest of workspace (server-side library, any OS Rust supports)
**Project Type**: Library (Rust workspace crate feature addition)
**Performance Goals**: No new performance requirement beyond existing HistoryRead paths; per-timestamp bounding lookups reuse the same bounded-window `read_raw_modified` query pattern the aggregate engine already relies on for interval-boundary values
**Constraints**: Must not modify `aggregates/engine.rs`'s existing aggregate computation pipeline or behavior (reuse its public bounding/step helpers only); any new `HistoryStorageBackend` method must be a *default* method (non-breaking for third-party implementors), matching `read_processed`'s precedent -- confirmed during research (R7) that one such addition (`read_raw_reverse`) is genuinely needed, not merely convenient
**Scale/Scope**: One new backend default method + one new `SimpleNodeManagerImpl` override + reused engine helpers; no new crate, no new storage format

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Spec grounded against the real local Part 11/Part 13 PDF
  text (research.md R2), not assumption; the R3 finding (existing aggregate helpers don't fully
  match what was initially assumed) was surfaced and designed around explicitly rather than
  papered over. PASS.
- **II. Do It Right Once**: Reuses `interpolated_bound_at` and `resolve_stepped` rather than
  duplicating interpolation math; does not force an ill-fitting existing function (`simple_bound_at`)
  into a job it wasn't built for -- writes the small amount of genuinely-new bound-selection logic
  instead (research.md R3). PASS.
- **III. Individual Task Discipline**: Tasks (next phase) will be one-per-line, matching the
  established pattern from every prior history feature (032/034/035). PASS (verified at /speckit-tasks).
- **IV. Security Is Paramount**: Read-only historical-data path; per-timestamp bounding lookups
  use small, fixed `num_values_per_node` limits on both `read_raw_modified` (forward) and the new
  `read_raw_reverse` (backward), each an O(log n + k) indexed/tree-range query on both shipped
  backends -- explicitly chosen (research.md R7) over a naive fixed-window heuristic or an
  unbounded/retry backward scan precisely to avoid a client-controlled (`req_times`)
  performance/DoS surface. PASS.
- **V. Leave It Better Than You Found It**: Notes (not fixes) the pre-existing aggregate-engine
  gap found in R3 (no outward quality-search in `agg_interpolative` today) as a one-line TODO.md
  entry rather than silently ignoring it or scope-creeping into fixing it here. PASS.

No violations requiring justification in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/107-history-read-at-time/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

No `contracts/` directory: this feature adds no new external service contract (the OPC UA wire
contract for `ReadAtTimeDetails` already exists and is unchanged; only the server-side
implementation behind it is new).

### Source Code (repository root)

```text
async-opcua-server/src/
├── history/
│   ├── backend.rs                  # + `read_raw_reverse` (new default method) + `read_at_time` (new default method) on HistoryStorageBackend
│   └── data_history.rs             # + InMemoryDataHistory::read_raw_reverse override
├── aggregates/
│   ├── engine.rs                   # unchanged; `interpolated_bound_at` becomes `pub(crate)` if not already reachable
│   └── middleware.rs                # unchanged; `resolve_stepped` reused as-is
└── node_manager/memory/
    └── simple.rs                   # + `history_read_at_time` override (alongside the other 4 history_read_* overrides)

async-opcua-history-sqlite/src/
└── backend.rs                      # + SqliteHistoryBackend::read_raw_reverse override

async-opcua-server/tests/
└── history_read_at_time.rs         # new end-to-end test (real client HistoryRead round trip)
```

**Structure Decision**: Single-crate addition within the existing `async-opcua-server` history
subsystem, following the exact file layout `read_processed` already established. No new crate, no
new top-level module.

## Complexity Tracking

> No Constitution Check violations -- section intentionally left without entries.
