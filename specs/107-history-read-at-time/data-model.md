# Data Model: Historical ReadAtTimeDetails

No new persisted entities. This feature is a read-only query built on existing history storage
(`HistoryStorageBackend::read_raw_modified`, already returning
`(Vec<DataValue>, Vec<ModificationInfo>, Option<Vec<u8>>)` --
`async-opcua-server/src/history/backend.rs:11`) and existing generated request/response types.

## Request

- **`ReadAtTimeDetails`** (generated, `async-opcua-types/src/generated/types/read_at_time_details.rs:27-30`)
  - `req_times: Option<Vec<UtcTime>>` -- the batch of requested timestamps, in client-supplied
    order (order is preserved in the response; duplicates are each evaluated independently).
  - `use_simple_bounds: bool` -- selects Simple Bounding Values (`true`) vs. Interpolated Bounding
    Values (`false`), per Part 13 §3.1.8/3.1.9 (see research.md R2/R3).

## Per-timestamp result (in-memory only, not persisted)

- **Bounding pair**: `(before: Option<DataValue>, after: Option<DataValue>)`, the raw values
  immediately adjacent to (simple bounds) or found by outward quality-search around (interpolated
  bounds) one requested timestamp. `after` (and an exact match) comes from a small, bounded
  forward `read_raw_modified` query; `before` comes from the new small, bounded backward
  `read_raw_reverse` query (research.md R7) -- both O(log n + k) on both shipped backends, never
  a full scan. Never persisted.
- **Resolved value**: a `DataValue` whose `.value` is either:
  - the exact raw value, when a raw sample exists at the requested timestamp, or
  - the `before` value verbatim, when the node is `Stepped` (research.md R4) and no exact match
    exists, or
  - a linearly-interpolated numeric value (via `interpolated_bound_at`'s existing ratio
    computation) when the node is *not* `Stepped` and both bounds resolve, or
  - `Bad_NoData`, when no exact match exists and a required bound is missing/Bad, or
  - `Bad_TimestampNotSupported`, when the node doesn't support the requested `TimestampsToReturn`.
  `.status` carries `StatusCodeValueType::Raw` or `::Interpolated` (research.md R5) on success.

## Per-node resolution input

- **`stepped: bool`** -- resolved once per node via the existing
  `crate::aggregates::resolve_stepped(address_space, node_id)`
  (`async-opcua-server/src/aggregates/middleware.rs:19`), the same call
  `SimpleNodeManagerImpl::history_read_processed` already makes (simple.rs:569-583).

No schema, migration, or storage-format changes are introduced by this feature.
