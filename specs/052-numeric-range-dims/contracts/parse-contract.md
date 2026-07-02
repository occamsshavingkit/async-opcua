# Contract: NumericRange parse (Annex A.3)

| Input | Result |
|---|---|
| `""` | `None` (unchanged) |
| single well-formed dimension | `Index`/`Range` (unchanged) |
| N ≥ 2 well-formed dimensions, ANY N | `MultipleRanges(vec; len == N)` — cap removed |
| any malformed part / overflow / min≥max | `Err(NumericRangeError)` (unchanged) → `Invalid` on decode |

Application contract (unchanged, verified): `MultipleRanges` is per-dimension; count mismatch vs the
stored array → `Bad_IndexRangeNoData` in O(dims); selection output bounded by stored array size.
Display round-trips any parsed value.
