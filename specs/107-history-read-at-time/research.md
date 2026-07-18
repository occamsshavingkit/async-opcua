# Phase 0 Research: Historical ReadAtTimeDetails

## R1: Exact request/response shape

**Decision**: `ReadAtTimeDetails { req_times: Option<Vec<UtcTime>>, use_simple_bounds: bool }`
(`async-opcua-types/src/generated/types/read_at_time_details.rs:27-30`) is the request. The
response is one `HistoryData`/`DataValue` per requested timestamp, same envelope shape
`history_read_processed` already produces. `HistoryReadDetails::AtTime(ReadAtTimeDetails)`
already exists and is already dispatched to at `async-opcua-server/src/node_manager/history.rs:28`
(match arm line 38) -- only the actual read implementation is missing.

**Rationale**: Confirmed directly from the generated type and existing dispatch code; no
ambiguity here.

## R2: Real spec semantics (re-verified against the local PDF)

**Decision**: Confirmed via `~/opcua-specs/OPC 10000-11 - UA Specification Part 11 - Historical
Access 1.05.04.pdf` section 6.5.5.2 ("Read at time functionality"):
- Exact-timestamp match -> return the raw value, StatusCode value-type `Raw`.
- No exact match -> interpolate "following the same rules as the standard Interpolated Aggregate
  as outlined in OPC 10000-13", StatusCode value-type `Interpolated`.
- `useSimpleBounds = true` -> use Simple Bounding Values (Part 13 §3.1.9): the raw points
  immediately adjacent to the requested timestamp, used as-is regardless of their own quality;
  `Bad_NoData` if an adjacent point is itself Bad or doesn't exist.
- `useSimpleBounds = false` -> use Interpolated Bounding Values (Part 13 §3.1.8): search outward
  from the requested timestamp for the nearest non-Bad raw point on each side; `Bad_NoData` only
  if no usable point is found on a required side.
- Unsupported `TimestampsToReturn` for a node -> `Bad_TimestampNotSupported`
  (`opcua_types::StatusCode::BadTimestampNotSupported`, 0x80A10000, already a generated variant).

**Rationale**: This is a direct quote/paraphrase from the real spec text, not an assumption --
re-verified this session (the original feature-103 mistake this project has since had to correct
twice was trusting a paraphrase without re-grepping the actual PDF).

## R3: What's actually reusable from the aggregate engine (materially more nuanced than initially assumed)

**Decision**: `async-opcua-server/src/aggregates/engine.rs`'s `interpolated_bound_at(boundary,
before, after, use_sloped) -> Option<(f64, bool)>` (line ~1366) IS directly reusable for the
interpolation *math* (ratio-based linear interpolation between two numeric bounding points) in
both the simple-bounds and interpolated-bounds cases. However, two things the original feature
description assumed turned out not to hold, and must be built fresh in this feature rather than
reused wholesale:

1. **Bound *selection* differs from both existing call sites.** `compute_processed_intervals`
   (engine.rs:1529) derives its own `prior`/`next` by scanning for the nearest raw value on each
   side with **no quality filtering at all** -- that selection rule matches `useSimpleBounds=true`
   exactly (nearest neighbor, quality-agnostic), but **nothing in the existing aggregate engine
   currently implements "search outward past Bad-quality points"** for `useSimpleBounds=false`.
   `agg_interpolative` (the existing Interpolated Aggregate) feeds `input.prior`/`input.next`
   straight into `interpolated_bound_at` unfiltered too -- i.e. today's Interpolated Aggregate
   itself doesn't do the Part 13 §3.1.8 outward-search either. This is a pre-existing
   simplification in the aggregate engine, out of scope to fix here (per the explicit "don't
   redesign the aggregate pipeline" constraint) -- but it means this feature must implement its
   own outward-quality-search for the `useSimpleBounds=false` path; it cannot just call an
   existing "find interpolated bounds" entry point that already does this, because none exists.
2. **`simple_bound_at` (engine.rs:1411) is not what it sounds like.** It only accepts a `before`
   value and returns it held constant (a single-sided helper used by `agg_delta_bounds`/
   `agg_end_bound`, not a general "simple bounding value pair" function). It is not reused here;
   this feature calls `interpolated_bound_at` directly with the immediate-neighbor `before`/
   `after` pair for the `useSimpleBounds=true` case instead.

**Rationale**: Verified by reading the actual function bodies and their only existing call site
(`compute_processed_intervals`), not by trusting the surface-level function names. Reusing the
interpolation *math* while writing fresh (small) bound-*selection* logic is the correct scope --
matches Constitution Principle II ("do it right once") better than forcing an ill-fitting existing
function to serve a job it was never built for.

**Alternatives considered**: Extending `interpolated_bound_at`/`agg_interpolative` themselves to
do outward quality-search, fixing the aggregate engine's own gap as a byproduct. Rejected: out of
this feature's stated scope, and would require its own spec-grounding + regression pass against
every aggregate that depends on that function; tracked as a one-line TODO.md note instead, not
built here.

## R4: Stepped vs. sloped, and how this closes CU 2991 (structured data) as a byproduct

**Decision**: `crate::aggregates::resolve_stepped(address_space, node_id) -> bool`
(`async-opcua-server/src/aggregates/middleware.rs:19`) already resolves each node's
`HistoricalDataConfigurationType.Stepped` property (defaulting to `true` -- stepped -- when no
configuration exists), and `SimpleNodeManagerImpl::history_read_processed` already resolves it
per-node before dispatch (simple.rs:569-583). `history_read_at_time` reuses the exact same
resolution call. When a node is `stepped`, the "interpolated" value for a non-exact timestamp is
simply the prior raw value held constant (no numeric interpolation involved at all) -- which is
well-defined for *any* `Variant` type, not just numeric ones. When a node is *not* stepped
(sloped), numeric interpolation via `interpolated_bound_at` applies, which is only meaningful for
numeric types.

This resolves CU 2991 cleanly: structured/non-numeric historized values have no defined sloped
interpolation, but Part 11 doesn't need one for them either -- a structured-data historized item
is inherently `stepped` in practice (there's no other legitimate configuration for a type slope
interpolation can't apply to), so the existing Stepped branch -- which works for any `Variant` by
construction (it's just "return the last raw value verbatim") -- already covers the exact-match
and step-hold cases CU 2991 needs. **Byproduct closure confirmed, not just hypothesized.** The
only case genuinely unreachable for structured data is a *sloped*-configured structured-data node
requesting an interpolated (non-exact, non-stepped) result, which is not a real configuration
Part 11 requires supporting (sloped interpolation is inherently numeric) -- documented as an
explicit non-requirement rather than a silent gap.

**Rationale**: Traced `resolve_stepped`'s real default and call site rather than assuming a
config always exists; confirmed the Stepped path is genuinely type-agnostic by reading
`interpolated_bound_at`'s `(None, Some) | (Some, None)` step-hold arms, which just return the
already-known value without any `variant_to_f64` conversion in the step-hold case -- for
ReadAtTime's step-hold path, this feature reads the raw `Variant` straight off the bounding
`DataValue`, never touching `variant_to_f64` at all, so no numeric conversion ever happens for a
`stepped` node.

## R5: InfoBits (Raw / Interpolated marking) -- a mechanism already exists, unused so far

**Decision**: `opcua_types::StatusCode` already has `value_type()`/`set_value_type()`
(`async-opcua-types/src/status_code.rs:254-263`) operating on a `StatusCodeValueType` enum with
`Raw = 0b00`, `Calculated = 0b01`, `Interpolated = 0b10` (lines ~517-521) -- this is the exact
Part 8 HistoryInfoBits mechanism Part 11 §6.5.5.2 refers to. **Nothing in the aggregate engine
currently calls `set_value_type` at all** -- this feature is the first caller. Use
`status_code.set_value_type(StatusCodeValueType::Raw)` for exact-timestamp results and
`StatusCodeValueType::Interpolated` for computed results, on the `DataValue.status` returned for
each requested timestamp.

**Rationale**: Confirmed the mechanism exists at the types layer via direct code read; confirmed
by grep that it's unused anywhere in `async-opcua-server/src/aggregates/`, so this feature does
not need to touch or coordinate with any existing InfoBits-setting code.

## R6: Architectural placement and feature gating

**Decision**: Implement `read_at_time` as a new default method on `HistoryStorageBackend`
(`async-opcua-server/src/history/backend.rs`), gated behind the `history-aggregates` feature
(since it needs `interpolated_bound_at`/`resolve_stepped`/`StatusCodeValueType`, all currently
gated the same way `read_processed`'s default is) -- exactly mirroring `read_processed`'s existing
shape (backend.rs:85-140+), built on `read_raw_modified` plus the new `read_raw_reverse` (R7). This
gets both `InMemoryDataHistory` and `SqliteHistoryBackend`
(`async-opcua-history-sqlite/src/backend.rs:583`) the capability once both provide real
`read_raw_reverse` overrides -- no other backend-specific `read_at_time` code needed.
`SimpleNodeManagerImpl::history_read_at_time` (new override in
`async-opcua-server/src/node_manager/memory/simple.rs`, alongside the other four `history_read_*`
overrides) resolves `stepped` per node exactly like `history_read_processed` does, then delegates.

**Rationale**: Directly matches the established, working `read_processed` precedent; avoids
duplicating backend-specific logic; keeps the feature-flag boundary consistent with the other
history-aggregates-dependent capability already shipped.

**Alternatives considered**: Implementing directly in `simple.rs` only (no shared backend
default). Rejected: would mean the SQLite backend gets nothing without a second, separately-
written implementation, unlike every other aggregate-dependent history capability in this
codebase.

## R7: `read_raw_modified` is NOT sufficient -- one small, well-scoped new storage-layer method is genuinely needed

**Decision (revised after checking both backend implementations, not assumed):** the original
plan hypothesized reusing `read_raw_modified` alone, mirroring `read_processed`'s full-range-drain
pattern. Checking both shipped backends surfaced two real problems with that:

1. **`return_bounds` is silently a no-op on the in-memory backend.** `InMemoryDataHistory::
   read_raw_modified` (`async-opcua-server/src/history/data_history.rs:129-137,571`) takes
   `_return_bounds: bool` -- underscore-prefixed, completely ignored. Only the SQLite backend
   (`async-opcua-history-sqlite/src/backend.rs:368-369`) honors it. Relying on `return_bounds` to
   get genuine outside-window bounding values (as `read_processed`'s own default impl does) would
   make the in-memory backend silently return `Bad_NoData` at range edges where the SQLite
   backend would correctly interpolate -- a direct violation of FR-007 ("behaves identically
   regardless of backend"), not an acceptable inherited limitation.
2. **`read_raw_modified` has no way to find "the nearest sample before T," however far back it
   is, without a full backward scan.** Its only truncation behavior is a forward limit from the
   start of the queried range (`node_values.range(effective_start..end_tick)`, capped by
   `num_values_per_node` taken from the *earliest* end) -- there is no primitive for "give me the
   closest point at or before T, scanning backward." Faking this with a fixed-size lookback
   window is semantically wrong (Part 13's outward search has no distance cap -- a legitimately
   sparse history could have the nearest usable sample arbitrarily far back), and faking it with
   an ever-widening series of forward-scan retries risks a full-history linear scan per requested
   timestamp -- a real, attacker-reachable performance/DoS concern per Constitution Principle IV
   given `req_times` is client-controlled.

Both storages happen to make "nearest sample at-or-before T, closest-first, bounded count" trivial
and efficient to add directly: the in-memory backend's `node_values` is a `BTreeMap`, where
`.range(..=at_or_before).rev().take(n)` is O(log n + n); the SQLite backend already indexes on
timestamp, where `ORDER BY timestamp DESC LIMIT n` is equally natural. **Add
`read_raw_reverse(node_id, at_or_before, num_values_per_node) -> Result<Vec<DataValue>,
StatusCode>`** as a new *default* method (not required) on `HistoryStorageBackend`, defaulting to
`Err(BadHistoryOperationUnsupported)` so it is non-breaking for any third-party implementor of
this public trait, and give both shipped backends (`InMemoryDataHistory`,
`SqliteHistoryBackend`) real overrides. `read_at_time`'s default impl then uses
`read_raw_modified` (forward, small limit) to find the exact match / nearest "next" bound, and
the new `read_raw_reverse` (backward, small limit) to find the nearest "before" bound -- both
genuinely bounded, backend-efficient, and correctness-equivalent across both storages.

**Rationale**: The original out-of-scope note explicitly anticipated this exact contingency ("if
investigation during planning reveals it's genuinely insufficient... that's a real finding to
document, not a blocker to work around with an overly clever storage-layer redesign"). This is
that documented finding: a fixed-window heuristic would be semantically wrong per Part 13, and an
unbounded/retry scan is a real performance and DoS concern (Principle IV) -- a small, symmetrical,
default (non-breaking) addition to the same trait `read_raw_modified` already lives on is the
correct-once (Principle II) resolution, not scope creep.

**Alternatives considered**:
- *Fixed lookback window per timestamp*: rejected -- silently wrong for legitimately sparse
  history (returns `Bad_NoData` where a real bound exists further back), and the window size
  would be an arbitrary, unspecified magic number with no basis in the OPC UA spec.
- *Widening/retry forward scans to fake backward search*: rejected -- can degrade to a full
  linear scan per requested timestamp, an attacker-reachable resource-exhaustion vector given
  `req_times` is client-controlled (Principle IV).
- *Require every custom `HistoryStorageBackend` implementor to add this method*: rejected --
  breaking a public trait's required-method set is avoided by making it a defaulted method with a
  safe `Unsupported` fallback, exactly like `read_processed` itself already is.

## R8: CU 2991 status

**Decision**: Closed as a byproduct (see R4) -- a type-agnostic `read_at_time` implementation,
built on `Variant`-generic step-hold plus numeric-only sloped interpolation gated by the node's
own `Stepped` configuration, naturally covers structured/non-numeric historized values without
any additional code. Verify with a dedicated unit test using a structured (non-numeric) historized
value at both an exact-match and a between-samples timestamp.

**Rationale**: See R4; this is the planning-phase confirmation the original feature description
asked for rather than assumed.

## R9: ContinuationPoint handling -- a real correction made during implementation (T001)

**Decision**: The real Part 11 v1.05.04 §6.5.5.2 text, re-verified via `pdftotext -layout` against
the local PDF as T001 requires (not trusted from paraphrase), is explicit: *"The standard
ContinuationPoint rules (see 6.3) apply"* to `ReadAtTimeDetails`. An earlier draft of this
feature's planning (during `/speckit-analyze`) incorrectly inferred, by analogy with
`read_processed`'s unconditional continuation-point rejection, that `ReadAtTimeDetails` needed no
continuation-point support since its result count is bounded by `req_times.len()`. **That
inference was wrong** and has been reverted in spec.md/tasks.md.

The apparent tension this correction resolves: unlike `ReadRawModifiedDetails`/
`ReadProcessedDetails`, the generated `ReadAtTimeDetails` type (R1) has **no**
`num_values_per_node`-style field, so there is no client-specified "max results per operation" to
honor. §6.3's general rule ("A Server shall not return more than this number of results but it
can return fewer") is written for operations that *do* have such a field; for `ReadAtTimeDetails`,
§6.3's own text still supports server-*initiated* partial results independent of a client limit
("Server can return fewer results due to buffer issues or other internal constraints... If a
request is taking a long time to calculate... the Server can return partial results with a
ContinuationPoint"). So `read_at_time` implements a server-chosen internal batch size (a fixed
constant bounding how many of `req_times` are resolved per call, purely a
defensive/latency-bounding measure per Constitution Principle IV, not a client-requested feature)
-- if more of `req_times` remains after one batch, return a continuation point encoding the resume
index; a supplied continuation point decodes back to that resume index. This is index-into-
`req_times` pagination, unrelated to the backend's own time-range-based continuation tokens used
by `read_raw_modified`/`read_raw_reverse` (those remain purely internal implementation details of
resolving each individual timestamp's bounding values, never exposed to the client).

**Rationale**: This is exactly the kind of "verify the real spec text before hardcoding behavior"
discipline this project has had to re-learn multiple times (features 103/104's directory-
singleton correction is the canonical prior example) -- T001 existing specifically to catch this
class of mistake before it reaches code, which it did here.
