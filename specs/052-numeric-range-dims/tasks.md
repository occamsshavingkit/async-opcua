# Tasks: NumericRange Dimension Cap Removal (P4-ATTR-06)

**Feature**: `052-numeric-range-dims` | **Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

Division of labor per practice: Claude authors tests red-first; codex implements (no-git guardrail,
no test edits).

## Phase 1: Setup

- [X] T201 Verify current shape unchanged (numeric_range.rs:227 cap; from_ua_string Invalid
  fallback; per-dimension consumers) — done during research 2026-07-02; re-confirm on branch.

## Phase 2: US1 — cap removal (P1)

- [X] T202 [US1] Claude: red-first tests. In `async-opcua-types/src/numeric_range.rs` tests: move
  `"0,1,2,3,4,5,6,7,8,9,10"` from `invalid_numeric_ranges` to a valid 11-dim case with parse +
  display round-trip; add a large-dim (1024) parse success asserting len. In the variant tests: an
  11-dimension array `range_of` block selection + `set_range_of` write-back; a 1024-dim range vs a
  2-dim array → `Bad_IndexRangeNoData` (fail fast, Part 4 §7.27/Annex A.3).
- [X] T203 [US1] codex: in `async-opcua-types/src/numeric_range.rs`, remove `MAX_INDICES` and the
  `2..=MAX_INDICES` arm bound: `1 => parse single`, `_ => MultipleRanges` (split never yields 0
  parts); replace the `really????` comment with the Annex A.3 citation + boundedness note
  (allocation ≤ split-part count of an already-bounded string). No other behavior change.

## Phase 3: Polish

- [X] T204 Update `specs/complexity-cuts-backlog.md` `Variant::range_of` entry: stale — resolved by
  feature 017's per-dimension rework; cap removed by 052 without reintroducing O(n·m).
- [X] T205 FINDINGS.md P4-ATTR-06 → FIXED (with corrected behavior description: Invalid-fallback →
  per-op BadIndexRangeInvalid, not decode failure).
- [X] T206 Full gate: `cargo test -p async-opcua-types`, full workspace suite via
  `cargo test -p async-opcua-server` + workspace check --all-targets, clippy legs, fmt.
