#[cfg(feature = "history-aggregates")]
use crate::aggregates::engine::{compute_processed_intervals, get_value_timestamp};
use async_trait::async_trait;
use moka::future::Cache;
use opcua_types::{
    AggregateConfiguration, DataValue, DateTime, EventFilter, HistoryEventFieldList,
    ModificationInfo, NodeId, PerformUpdateType, StatusCode,
};

/// Raw/modified HistoryRead result: values, modification metadata, continuation token.
pub type HistoryRawModifiedResult = (Vec<DataValue>, Vec<ModificationInfo>, Option<Vec<u8>>);

/// A cache for historical data values to avoid database hits.
#[derive(Clone)]
pub struct HistoryCache {
    cache: Cache<(NodeId, i64, i64), Vec<DataValue>>,
}

impl HistoryCache {
    /// Create a new HistoryCache with the given maximum capacity.
    pub fn new(max_capacity: u64) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_idle(std::time::Duration::from_secs(300))
                .build(),
        }
    }

    /// Retrieve cached values.
    pub async fn get(&self, key: &(NodeId, DateTime, DateTime)) -> Option<Vec<DataValue>> {
        let cache_key = (key.0.clone(), key.1.ticks(), key.2.ticks());
        self.cache.get(&cache_key).await
    }

    /// Insert values into the cache.
    pub async fn insert(&self, key: (NodeId, DateTime, DateTime), values: Vec<DataValue>) {
        let cache_key = (key.0, key.1.ticks(), key.2.ticks());
        self.cache.insert(cache_key, values).await;
    }

    /// Get current size of cache.
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }

    /// Invalidate all entries in the cache.
    pub fn invalidate_all(&self) {
        self.cache.invalidate_all();
    }

    /// Force cache tasks to run (useful for testing eviction immediately).
    pub async fn run_pending_tasks(&self) {
        self.cache.run_pending_tasks().await;
    }
}

/// Trait representing a storage backend for OPC-UA Historical Data Access (HDA).
/// Custom backends (e.g. SQLite, In-Memory, etc.) implement this trait.
///
/// HistoryUpdate write implementations for [`HistoryStorageBackend::update_data`],
/// [`HistoryStorageBackend::update_structure_data`], and
/// [`HistoryStorageBackend::update_event`] must support all [`PerformUpdateType`]
/// modes: [`PerformUpdateType::Insert`], [`PerformUpdateType::Replace`],
/// [`PerformUpdateType::Update`], and [`PerformUpdateType::Remove`].
#[async_trait]
pub trait HistoryStorageBackend: Send + Sync {
    /// Reads raw data values from the history backend.
    #[allow(clippy::too_many_arguments)]
    async fn read_raw_modified(
        &self,
        node_id: &NodeId,
        start_time: DateTime,
        end_time: DateTime,
        num_values_per_node: u32,
        return_bounds: bool,
        is_read_modified: bool,
        continuation_point: Option<Vec<u8>>,
    ) -> Result<HistoryRawModifiedResult, StatusCode>;

    /// Reads up to `num_values_per_node` raw values at or before `at_or_before`, in descending
    /// time order (closest first). Used by [`HistoryStorageBackend::read_at_time`]'s bounding-
    /// value search: unlike [`HistoryStorageBackend::read_raw_modified`] (forward-only, truncated
    /// from the *start* of its range), there is no existing way to efficiently find "the nearest
    /// sample before T, however far back it is" -- see `specs/107-history-read-at-time/
    /// research.md` R7 for why a fixed-window heuristic or an unbounded backward scan were both
    /// rejected in favor of this small, symmetrical addition. Defaults to `Unsupported` so this
    /// is non-breaking for any existing external implementor of this trait; both backends this
    /// SDK ships override it.
    async fn read_raw_reverse(
        &self,
        _node_id: &NodeId,
        _at_or_before: DateTime,
        _num_values_per_node: u32,
    ) -> Result<Vec<DataValue>, StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Reads processed aggregate values from the history backend.
    // ponytail: 8 params (added AggregateConfiguration); a params struct isn't worth it — the
    // signature won't grow further (bounds flow through read_raw_modified, not here).
    #[allow(clippy::too_many_arguments)]
    async fn read_processed(
        &self,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))] node_id: &NodeId,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        start_time: DateTime,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        end_time: DateTime,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        processing_interval: f64,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        aggregate_type: &NodeId,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        aggregate_configuration: &AggregateConfiguration,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))] stepped: bool,
        continuation_point: Option<Vec<u8>>,
    ) -> Result<(Vec<DataValue>, Option<Vec<u8>>), StatusCode> {
        #[cfg(not(feature = "history-aggregates"))]
        {
            let _ = (
                node_id,
                start_time,
                end_time,
                processing_interval,
                aggregate_type,
                aggregate_configuration,
                stepped,
            );
            if continuation_point.is_some() {
                return Err(StatusCode::BadContinuationPointInvalid);
            }
            return Err(StatusCode::BadAggregateNotSupported);
        }

        #[cfg(feature = "history-aggregates")]
        {
            if continuation_point.is_some() {
                return Err(StatusCode::BadContinuationPointInvalid);
            }

            let mut raw_values = Vec::new();
            let mut next_token = None;
            loop {
                let (values, _modification_infos, token) = self
                    .read_raw_modified(
                        node_id, start_time, end_time, 100_000, true, false, next_token,
                    )
                    .await?;
                raw_values.extend(values);

                let Some(token) = token else {
                    break;
                };
                next_token = Some(token);
            }

            raw_values.sort_by_key(get_value_timestamp);

            let annotation_times: Vec<DateTime> = if aggregate_type == &NodeId::new(0u16, 2351u32) {
                match self.read_annotations(node_id, &[], None).await {
                    Ok((dvs, _)) => {
                        let mut timestamps: Vec<DateTime> = dvs
                            .iter()
                            .map(get_value_timestamp)
                            .filter(|timestamp| *timestamp >= start_time && *timestamp <= end_time)
                            .collect();
                        timestamps.sort();
                        timestamps
                    }
                    Err(_) => Vec::new(),
                }
            } else {
                Vec::new()
            };

            let processed_values = compute_processed_intervals(
                &raw_values,
                aggregate_type,
                aggregate_configuration,
                start_time,
                end_time,
                processing_interval,
                stepped,
                &annotation_times,
            );

            Ok((processed_values, None))
        }
    }

    /// Reads values at specific timestamps (OPC-10000-11 §6.5.5.2, `ReadAtTimeDetails`). An
    /// exact-timestamp raw match is returned as-is (`Raw`); otherwise a value is computed by
    /// interpolating between bounding raw values (`Interpolated`), selected per
    /// `use_simple_bounds` (immediate neighbors, regardless of quality) or not (search outward,
    /// bounded, for the nearest usable-quality neighbor on each side -- see
    /// `specs/107-history-read-at-time/research.md` R3/R7). `stepped` nodes step-hold `before`'s
    /// value verbatim (any `Variant` type); non-stepped nodes numerically interpolate (meaningful
    /// only for numeric types). Genuinely supports the standard ContinuationPoint mechanism (Part
    /// 11 §6.3) by paginating over `req_times` in fixed-size batches -- see research.md R9 for why
    /// this must NOT simply reject any supplied continuation point.
    #[allow(clippy::too_many_arguments)]
    async fn read_at_time(
        &self,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))] node_id: &NodeId,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        req_times: &[DateTime],
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))]
        use_simple_bounds: bool,
        #[cfg_attr(not(feature = "history-aggregates"), allow(unused_variables))] stepped: bool,
        continuation_point: Option<Vec<u8>>,
    ) -> Result<(Vec<DataValue>, Option<Vec<u8>>), StatusCode> {
        #[cfg(not(feature = "history-aggregates"))]
        {
            let _ = (node_id, req_times, use_simple_bounds, stepped);
            if continuation_point.is_some() {
                return Err(StatusCode::BadContinuationPointInvalid);
            }
            return Err(StatusCode::BadHistoryOperationUnsupported);
        }

        #[cfg(feature = "history-aggregates")]
        {
            let resume_index = match continuation_point {
                Some(token) => read_at_time::decode_resume_index(&token)?,
                None => 0,
            };
            if resume_index > req_times.len() {
                return Err(StatusCode::BadContinuationPointInvalid);
            }

            let batch_end = (resume_index + read_at_time::BATCH_LIMIT).min(req_times.len());
            let batch = &req_times[resume_index..batch_end];

            let mut results = Vec::with_capacity(batch.len());
            for &req_time in batch {
                results.push(
                    read_at_time::resolve_one(self, node_id, req_time, use_simple_bounds, stepped)
                        .await?,
                );
            }

            let next_token =
                (batch_end < req_times.len()).then(|| read_at_time::encode_resume_index(batch_end));

            Ok((results, next_token))
        }
    }

    /// Reads historical events from the history backend.
    async fn read_events(
        &self,
        _node_id: &NodeId,
        _start_time: DateTime,
        _end_time: DateTime,
        _num_values_per_node: u32,
        _filter: &EventFilter,
        _continuation_point: Option<Vec<u8>>,
    ) -> Result<(Vec<HistoryEventFieldList>, Option<Vec<u8>>), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Reads annotation data values from the history backend.
    async fn read_annotations(
        &self,
        _node_id: &NodeId,
        _req_times: &[DateTime],
        _continuation_point: Option<Vec<u8>>,
    ) -> Result<(Vec<DataValue>, Option<Vec<u8>>), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Updates or inserts historical data values.
    async fn update_data(
        &self,
        node_id: &NodeId,
        perform_insert_replace: PerformUpdateType,
        values: Vec<DataValue>,
    ) -> Result<Vec<StatusCode>, StatusCode>;

    /// Updates annotation history data values.
    async fn update_structure_data(
        &self,
        _node_id: &NodeId,
        _perform: PerformUpdateType,
        _values: Vec<DataValue>,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Updates historical event field lists.
    async fn update_event(
        &self,
        _node_id: &NodeId,
        _filter: &EventFilter,
        _events: Vec<HistoryEventFieldList>,
        _perform: PerformUpdateType,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Deletes raw or modified historical data over a time range.
    async fn delete_raw_modified(
        &self,
        _node_id: &NodeId,
        _is_delete_modified: bool,
        _start_time: DateTime,
        _end_time: DateTime,
    ) -> Result<StatusCode, StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Deletes historical data values at requested timestamps.
    async fn delete_at_time(
        &self,
        _node_id: &NodeId,
        _req_times: Vec<DateTime>,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Deletes historical events by event id.
    async fn delete_event(
        &self,
        _node_id: &NodeId,
        _event_ids: Vec<opcua_types::ByteString>,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Releases a backend-owned continuation point token.
    async fn release_continuation_point(&self, _token: Vec<u8>) -> Result<(), StatusCode> {
        Ok(())
    }
}

/// Helpers for [`HistoryStorageBackend::read_at_time`]. Kept in their own module since the logic
/// (bounded outward quality-search, continuation-token index encoding, stepped-vs-interpolated
/// value resolution) is meaningfully more involved than the other default methods on this trait.
#[cfg(feature = "history-aggregates")]
mod read_at_time {
    use opcua_types::{DataValue, DateTime, NodeId, StatusCode, StatusCodeValueType, Variant};

    use crate::aggregates::engine::{interpolated_bound_at, variant_to_f64};

    use super::HistoryStorageBackend;

    /// How many of the client's requested timestamps are resolved per call. `ReadAtTimeDetails`
    /// has no client-specified "max results" field of its own (unlike `ReadRawModifiedDetails`/
    /// `ReadProcessedDetails`), so this purely bounds server-side per-call work against a
    /// client-controlled `req_times` array -- see `specs/107-history-read-at-time/research.md` R9.
    pub(super) const BATCH_LIMIT: usize = 1_000;

    /// Initial candidate-batch size for the bounded outward quality-search
    /// (`use_simple_bounds = false`); doubles on each retry up to `OUTWARD_SEARCH_MAX_BATCH`.
    const OUTWARD_SEARCH_INITIAL_BATCH: u32 = 8;
    /// Upper bound on how many raw samples are scanned per direction, per requested timestamp,
    /// while searching outward past Bad-quality points -- bounds worst-case per-timestamp work
    /// regardless of how sparse or how much Bad-quality data precedes/follows it.
    const OUTWARD_SEARCH_MAX_BATCH: u32 = 512;

    pub(super) fn decode_resume_index(token: &[u8]) -> Result<usize, StatusCode> {
        let bytes: [u8; 8] = token
            .try_into()
            .map_err(|_| StatusCode::BadContinuationPointInvalid)?;
        Ok(u64::from_le_bytes(bytes) as usize)
    }

    pub(super) fn encode_resume_index(index: usize) -> Vec<u8> {
        (index as u64).to_le_bytes().to_vec()
    }

    fn is_good(value: &DataValue) -> bool {
        value.status.is_none_or(|status| status.is_good())
    }

    fn bad_no_data_at(timestamp: DateTime) -> DataValue {
        DataValue {
            value: None,
            status: Some(StatusCode::BadNoData),
            source_timestamp: Some(timestamp),
            server_timestamp: Some(timestamp),
            ..Default::default()
        }
    }

    /// Reconstructs an interpolated `f64` as the same numeric `Variant` type as `template` (the
    /// bounding raw value it was interpolated from), so a client reading e.g. a `Float` node's
    /// history doesn't get `Double` results back. Falls back to `Double` for a template type
    /// `variant_to_f64` wouldn't have accepted in the first place (unreachable in practice, since
    /// the caller only reaches here after confirming both bounds converted successfully).
    fn f64_to_matching_variant(value: f64, template: &Variant) -> Variant {
        match template {
            Variant::Float(_) => Variant::Float(value as f32),
            Variant::Int32(_) => Variant::Int32(value.round() as i32),
            Variant::UInt32(_) => Variant::UInt32(value.round() as u32),
            Variant::Int16(_) => Variant::Int16(value.round() as i16),
            Variant::UInt16(_) => Variant::UInt16(value.round() as u16),
            Variant::SByte(_) => Variant::SByte(value.round() as i8),
            Variant::Byte(_) => Variant::Byte(value.round() as u8),
            Variant::Int64(_) => Variant::Int64(value.round() as i64),
            Variant::UInt64(_) => Variant::UInt64(value.round() as u64),
            _ => Variant::Double(value),
        }
    }

    /// Resolves a single requested timestamp against `backend`'s raw history for `node_id`.
    pub(super) async fn resolve_one<B: HistoryStorageBackend + ?Sized>(
        backend: &B,
        node_id: &NodeId,
        req_time: DateTime,
        use_simple_bounds: bool,
        stepped: bool,
    ) -> Result<DataValue, StatusCode> {
        let far_future = DateTime::from(i64::MAX);

        // Exact match: a single-tick-wide forward window at req_time itself.
        let next_tick = DateTime::from(req_time.ticks().saturating_add(1));
        let (exact, _mods, _cp) = backend
            .read_raw_modified(node_id, req_time, next_tick, 1, false, false, None)
            .await?;
        if let Some(mut value) = exact.into_iter().next() {
            value.status = Some(
                value
                    .status
                    .unwrap_or(StatusCode::Good)
                    .set_value_type(StatusCodeValueType::Raw),
            );
            return Ok(value);
        }

        // No exact match: find bounding values per `use_simple_bounds`.
        let (before, after) = if use_simple_bounds {
            let before = backend
                .read_raw_reverse(node_id, req_time, 1)
                .await?
                .into_iter()
                .next()
                .filter(is_good);
            let (after_candidates, ..) = backend
                .read_raw_modified(node_id, req_time, far_future, 1, false, false, None)
                .await?;
            let after = after_candidates.into_iter().next().filter(is_good);
            (before, after)
        } else {
            (
                nearest_good_before(backend, node_id, req_time).await?,
                nearest_good_after(backend, node_id, req_time).await?,
            )
        };

        Ok(resolve_stepped_or_interpolated(
            req_time,
            before.as_ref(),
            after.as_ref(),
            stepped,
        ))
    }

    /// Bounded outward search for the nearest Good-quality raw value at or before `req_time`
    /// (exclusive of `req_time` itself -- callers only reach here once an exact match has
    /// already been ruled out).
    async fn nearest_good_before<B: HistoryStorageBackend + ?Sized>(
        backend: &B,
        node_id: &NodeId,
        req_time: DateTime,
    ) -> Result<Option<DataValue>, StatusCode> {
        let mut limit = OUTWARD_SEARCH_INITIAL_BATCH;
        loop {
            let candidates = backend.read_raw_reverse(node_id, req_time, limit).await?;
            if let Some(good) = candidates.iter().find(|v| is_good(v)) {
                return Ok(Some(good.clone()));
            }
            if (candidates.len() as u32) < limit || limit >= OUTWARD_SEARCH_MAX_BATCH {
                // Reached the true start of this node's history, or the bounded search limit.
                return Ok(None);
            }
            limit = (limit * 4).min(OUTWARD_SEARCH_MAX_BATCH);
        }
    }

    /// Bounded outward search for the nearest Good-quality raw value strictly after `req_time`.
    async fn nearest_good_after<B: HistoryStorageBackend + ?Sized>(
        backend: &B,
        node_id: &NodeId,
        req_time: DateTime,
    ) -> Result<Option<DataValue>, StatusCode> {
        let after_time = DateTime::from(req_time.ticks().saturating_add(1));
        let far_future = DateTime::from(i64::MAX);
        let mut limit = OUTWARD_SEARCH_INITIAL_BATCH;
        loop {
            let (candidates, ..) = backend
                .read_raw_modified(node_id, after_time, far_future, limit, false, false, None)
                .await?;
            if let Some(good) = candidates.iter().find(|v| is_good(v)) {
                return Ok(Some(good.clone()));
            }
            if (candidates.len() as u32) < limit || limit >= OUTWARD_SEARCH_MAX_BATCH {
                return Ok(None);
            }
            limit = (limit * 4).min(OUTWARD_SEARCH_MAX_BATCH);
        }
    }

    /// Computes the ReadAtTime result once bounding values are known: step-hold (any `Variant`
    /// type, closing CU 2991) for `stepped` nodes, numeric linear interpolation (reusing the
    /// Interpolated Aggregate's own ratio math) for non-stepped nodes. `Bad_NoData` if a required
    /// bound is missing -- deliberately does NOT fall through to `interpolated_bound_at`'s own
    /// one-sided step-hold arms for the non-stepped case, since a missing bound genuinely means
    /// "no data," not "hold the one side we have" (see research.md R3).
    fn resolve_stepped_or_interpolated(
        boundary: DateTime,
        before: Option<&DataValue>,
        after: Option<&DataValue>,
        stepped: bool,
    ) -> DataValue {
        if stepped {
            let Some(before) = before else {
                return bad_no_data_at(boundary);
            };
            let mut result = before.clone();
            result.source_timestamp = Some(boundary);
            result.server_timestamp = Some(boundary);
            result.status = Some(
                before
                    .status
                    .unwrap_or(StatusCode::Good)
                    .set_value_type(StatusCodeValueType::Interpolated),
            );
            return result;
        }

        let (Some(before), Some(after)) = (before, after) else {
            return bad_no_data_at(boundary);
        };
        let Some(numeric) = interpolated_bound_at(boundary, Some(before), Some(after), false)
            .map(|(value, _)| value)
        else {
            return bad_no_data_at(boundary);
        };
        let template = before
            .value
            .as_ref()
            .filter(|v| variant_to_f64(v).is_some())
            .or(after.value.as_ref())
            .cloned()
            .unwrap_or(Variant::Double(numeric));

        DataValue {
            value: Some(f64_to_matching_variant(numeric, &template)),
            status: Some(StatusCode::Good.set_value_type(StatusCodeValueType::Interpolated)),
            source_timestamp: Some(boundary),
            server_timestamp: Some(boundary),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A backend implementing ONLY the two required methods, to prove the five new
    // write methods default to Bad_HistoryOperationUnsupported (FR-011 backwards compat).
    struct MinimalBackend;

    #[async_trait]
    impl HistoryStorageBackend for MinimalBackend {
        async fn read_raw_modified(
            &self,
            _node_id: &NodeId,
            _start_time: DateTime,
            _end_time: DateTime,
            _num_values_per_node: u32,
            _return_bounds: bool,
            _is_read_modified: bool,
            _continuation_point: Option<Vec<u8>>,
        ) -> Result<HistoryRawModifiedResult, StatusCode> {
            Ok((Vec::new(), Vec::new(), None))
        }

        async fn update_data(
            &self,
            _node_id: &NodeId,
            _perform_insert_replace: PerformUpdateType,
            _values: Vec<DataValue>,
        ) -> Result<Vec<StatusCode>, StatusCode> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn new_write_methods_default_to_unsupported() {
        let b = MinimalBackend;
        let node = NodeId::null();
        assert_eq!(
            b.update_structure_data(&node, PerformUpdateType::Insert, Vec::new())
                .await,
            Err(StatusCode::BadHistoryOperationUnsupported)
        );
        assert_eq!(
            b.delete_at_time(&node, Vec::new()).await,
            Err(StatusCode::BadHistoryOperationUnsupported)
        );
        assert_eq!(
            b.delete_event(&node, Vec::new()).await,
            Err(StatusCode::BadHistoryOperationUnsupported)
        );
        assert_eq!(
            b.delete_raw_modified(&node, false, DateTime::null(), DateTime::null())
                .await,
            Err(StatusCode::BadHistoryOperationUnsupported)
        );
    }
}
