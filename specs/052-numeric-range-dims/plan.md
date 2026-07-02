# Implementation Plan: NumericRange Dimension Cap Removal (P4-ATTR-06)

**Branch**: `052-numeric-range-dims` | **Date**: 2026-07-02 | **Spec**: [spec.md](./spec.md)

## Summary

Remove `MAX_INDICES=10` from `NumericRange::from_str` (Part 4 Annex A.3 permits any dimension
count), keeping parse cost linear and all other failure modes unchanged. Consumers verified safe
without the cap (research R3); the stale complexity-backlog note is corrected. Types-crate only.

## Technical Context

**Files**: `async-opcua-types/src/numeric_range.rs` (parse + its in-file tests),
`async-opcua-types/src/variant/mod.rs` tests (or `tests.rs`) for range application,
`specs/complexity-cuts-backlog.md`, `specs/conformance-audit/FINDINGS.md`.
**Testing**: `cargo test -p async-opcua-types` + full workspace suite. **Deps**: none.

## Constitution Check

- **I. Correctness Over Completion**: PASS — red-first tests incl. the flipped lock-in test;
  spec-grounded in Annex A.3.
- **II. Do It Right Once**: PASS — removes a wrong limit rather than raising it to another
  arbitrary number; boundedness argument documented at the parse site.
- **III. Individual Task Discipline**: PASS — one parse change, one test task, one docs task.
- **IV. Security Is Paramount**: PASS — R3/R4: no unbounded work or allocation; mismatch exits
  O(dims); allocation bounded by already-bounded input length; fuzz target unaffected.
- **V. Leave It Better**: PASS — kills the `really????` comment with a spec citation; fixes the
  stale complexity note.

**Result: PASS.** Complexity Tracking empty.

## Phase 0/1

See [research.md](./research.md) R1–R5. Design is the FromStr match collapse:
`1 => single`, `_ (≥2) => MultipleRanges` (split() never yields 0 parts), with a comment citing
Annex A.3 and the boundedness argument.
