# Requirements Checklist: 052-numeric-range-dims

- [X] Spec citation for the removal (Part 4 Annex A.3 recursive BNF, §7.27) — verified via MCP
- [X] Actual current behavior verified in code (Invalid fallback, not decode failure) and register wording corrected
- [X] Consumer-safety question answered before speccing (R3: per-dimension post-017; backlog note stale)
- [X] Allocation/DoS boundedness argued (R4) — Constitution IV
- [X] Behavior-change inventory: flipped lock-in test; BadIndexRangeInvalid→BadIndexRangeNoData for >10-dim mismatches
- [X] Test strategy red-first incl. end-to-end range application on an 11-dim array
- [X] No new arbitrary limit introduced
