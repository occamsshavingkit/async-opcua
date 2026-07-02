# Phase 0 Research: NumericRange Dimension Cap (P4-ATTR-06)

Verified 2026-07-02 against code + spec (opc-ua-reference MCP).

- **R1 — Spec**: Part 4 Annex A.3 BNF is recursive with no count limit; §7.27 "a range for each
  dimension separated by ','". The cap has no basis (matches the in-code `really????` doubt).
- **R2 — Actual failure mode**: parse failure → `NumericRange::Invalid(UAString)` via
  `from_ua_string` (numeric_range.rs:97-102) → consumers return `Bad_IndexRangeInvalid`. NOT a
  message decode failure (finding wording imprecise; register corrected).
- **R3 — Consumer safety (the load-bearing question)**: post-017, `range_of`/`set_range_of`/
  `array_range_selection` are per-dimension: `ranges.len() != stored_dims.len()` (± the string
  element-substring +1 case) exits in O(dims); `row_major_indices` output ≤ stored array size;
  `range_bounds` rejects nested `MultipleRanges`. The complexity-cuts-backlog O(n·m) note
  (variant/mod.rs:1609) describes the PRE-017 implementation — stale; the cap is not load-bearing.
- **R4 — Allocation bound**: parse allocates ≤ (len/2 + 1) `NumericRange` entries (~40 B each) from
  an input string already bounded by decoding limits — same amplification class as existing bounded
  containers (e.g. `Vec<UAString>` of empties). No new pre-allocation-DoS surface; `Vec::with_capacity`
  is derived from actual split count, not a length prefix.
- **R5 — Test strategy**: types-crate unit tests (the change is entirely in async-opcua-types):
  flip the 11-dim lock-in test in `invalid_numeric_ranges`; add valid 11-dim parse/display; add
  `range_of`/`set_range_of` on an 11-dim array (variant tests); add a large-dim (1000+) parse +
  mismatch fail-fast test. `fuzz_numeric_range` already exercises hostile constructed ranges.
