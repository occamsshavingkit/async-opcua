# Quickstart: HistoryRead ReadAtTimeDetails

## Before

```text
HistoryRead(ReadAtTimeDetails, node=Temperature, reqTimes=[T1, T2, T3])
→ Bad_HistoryOperationUnsupported     // history_read_at_time was never overridden anywhere
```

## After

A client has recorded raw samples for `Temperature` at 12:00:00 (20.0), 12:00:10 (22.0), and
12:00:20 (21.0) via the existing history-write path, and now wants values at three arbitrary
timestamps in one call:

```text
HistoryRead(ReadAtTimeDetails { reqTimes: [12:00:10, 12:00:05, 12:00:30], useSimpleBounds: false })
→ [
    DataValue(22.0, status=Good|Raw),          // exact match at 12:00:10
    DataValue(21.0, status=Good|Interpolated), // sloped interpolation between 12:00:00/12:00:10
    DataValue(_, status=Bad_NoData),           // 12:00:30 is after the last recorded sample
  ]
```

```text
// same request with useSimpleBounds: true and the value at 12:00:10 marked Bad quality upstream
HistoryRead(ReadAtTimeDetails { reqTimes: [12:00:05], useSimpleBounds: true })
→ [DataValue(_, status=Bad_NoData)]  // immediately-adjacent sample at 12:00:10 is itself Bad;
                                      // simple bounds never searches further out for a better one
```

A structured/non-numeric historized value (e.g. a recorded `Annotation` or other complex type)
behaves the same way for exact matches, and step-holds (not numeric-interpolates) for non-exact
timestamps, since such nodes are inherently `Stepped`:

```text
HistoryRead(ReadAtTimeDetails, node=StructuredLogEntry, reqTimes=[T_between_two_samples])
→ [DataValue(<value recorded just before T>, status=Good|Interpolated)]  // step-hold, any type
```

## Unchanged

- `ReadRawModified`, `ReadProcessed`, `ReadEvents`, `ReadAnnotations` HistoryRead requests behave
  identically to before.
- The Interpolated Aggregate (`ReadProcessed` with `AggregateType=Interpolative`) is untouched --
  this feature does not modify `aggregates/engine.rs`'s existing computation pipeline, only reuses
  its `interpolated_bound_at` ratio-interpolation function and `resolve_stepped` helper.
- Both the in-memory and SQLite history backends gain this capability identically, with no new
  configuration.
