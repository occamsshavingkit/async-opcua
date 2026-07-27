//! In-memory historical data storage.

use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use opcua_core::sync::RwLock;
#[cfg(test)]
use opcua_types::{Annotation, Variant};
use opcua_types::{
    DataValue, DateTime, HistoryUpdateType, ModificationInfo, NodeId, PerformUpdateType,
    StatusCode, UAString,
};

use crate::history::{
    AnnotationContinuationPoint, HistoryRawModifiedResult, HistoryStorageBackend,
};

const DEFAULT_MAX_VALUES_PER_NODE: usize = 10_000;

/// A superseded value retained when a raw entry is replaced, updated-over, or deleted.
type ModifiedEntry = (DataValue, ModificationInfo);
/// Per-node modified-history store: original source-ticks → the superseded entries at that timestamp.
type ModifiedStore = HashMap<NodeId, BTreeMap<i64, Vec<ModifiedEntry>>>;
/// Per-node annotation-history store: source-ticks → annotation `DataValue`.
type AnnotationStore = HashMap<NodeId, BTreeMap<i64, DataValue>>;

/// In-memory historical data backend for raw `DataValue` history.
pub struct InMemoryDataHistory {
    raw_values: RwLock<HashMap<NodeId, BTreeMap<i64, DataValue>>>,
    modified_values: RwLock<ModifiedStore>,
    annotation_values: RwLock<AnnotationStore>,
    max_per_node: usize,
}

impl InMemoryDataHistory {
    /// Creates an in-memory data history backend with a default per-node cap.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_VALUES_PER_NODE)
    }

    /// Creates an in-memory data history backend with the given per-node cap.
    pub fn with_capacity(max_per_node: usize) -> Self {
        Self {
            raw_values: RwLock::new(HashMap::new()),
            modified_values: RwLock::new(HashMap::new()),
            annotation_values: RwLock::new(HashMap::new()),
            max_per_node,
        }
    }

    fn record_modified(
        &self,
        node_id: &NodeId,
        source_ticks: i64,
        value: DataValue,
        update_type: HistoryUpdateType,
    ) {
        // ponytail: user_name is empty because the storage layer has no request context.
        let info = ModificationInfo {
            modification_time: DateTime::now(),
            update_type,
            user_name: UAString::null(),
        };

        let mut modified_values = self.modified_values.write();
        modified_values
            .entry(node_id.clone())
            .or_default()
            .entry(source_ticks)
            .or_default()
            .push((value, info));
    }

    fn enforce_raw_capacity(&self, values: &mut BTreeMap<i64, DataValue>) {
        while values.len() > self.max_per_node {
            let Some(oldest_tick) = values.keys().next().copied() else {
                break;
            };
            values.remove(&oldest_tick);
        }
    }

    fn read_modified_values(
        &self,
        node_id: &NodeId,
        start_tick: i64,
        end_tick: i64,
        num_values_per_node: u32,
        continuation_point: Option<Vec<u8>>,
    ) -> Result<HistoryRawModifiedResult, StatusCode> {
        let Some(node_values) = self.modified_values.read().get(node_id).cloned() else {
            return Ok((Vec::new(), Vec::new(), None));
        };

        let continuation_position = decode_modified_continuation_position(continuation_point)?;
        let (effective_start, skip_at_start) =
            modified_effective_start(start_tick, continuation_position);
        let limit = (num_values_per_node > 0).then_some(num_values_per_node as usize);

        let capacity = limit.unwrap_or(0).min(node_values.len());
        let mut values = Vec::with_capacity(capacity);
        let mut modification_infos = Vec::with_capacity(capacity);
        let mut next_token = None;

        'ticks: for (tick, entries) in node_values.range(effective_start..end_tick) {
            let entry_start = if *tick == effective_start {
                skip_at_start
            } else {
                0
            };

            for (entry_index, (value, info)) in entries.iter().enumerate().skip(entry_start) {
                if limit.is_some_and(|limit| values.len() >= limit) {
                    next_token = Some(encode_modified_continuation_position(*tick, entry_index));
                    break 'ticks;
                }

                values.push(value.clone());
                modification_infos.push(info.clone());
            }
        }

        Ok((values, modification_infos, next_token))
    }
}

impl Default for InMemoryDataHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HistoryStorageBackend for InMemoryDataHistory {
    async fn read_raw_modified(
        &self,
        node_id: &NodeId,
        start_time: DateTime,
        end_time: DateTime,
        num_values_per_node: u32,
        _return_bounds: bool,
        is_read_modified: bool,
        continuation_point: Option<Vec<u8>>,
    ) -> Result<HistoryRawModifiedResult, StatusCode> {
        let start_tick = start_time.ticks();
        let end_tick = end_time.ticks();
        if start_tick > end_tick {
            return Ok((Vec::new(), Vec::new(), None));
        }

        if is_read_modified {
            if start_tick == end_tick {
                return Ok((Vec::new(), Vec::new(), None));
            }

            return self.read_modified_values(
                node_id,
                start_tick,
                end_tick,
                num_values_per_node,
                continuation_point,
            );
        }

        let Some(node_values) = self.raw_values.read().get(node_id).cloned() else {
            return Ok((Vec::new(), Vec::new(), None));
        };

        let continuation_tick = decode_continuation_tick(continuation_point)?;
        let effective_start = continuation_tick.map_or(start_tick, |tick| tick.max(start_tick));
        let limit = (num_values_per_node > 0).then_some(num_values_per_node as usize);

        let mut values = Vec::with_capacity(limit.unwrap_or(0).min(node_values.len()));
        let mut next_token = None;
        for (tick, value) in node_values.range(effective_start..end_tick) {
            if limit.is_some_and(|limit| values.len() >= limit) {
                next_token = Some(encode_continuation_tick(*tick));
                break;
            }
            values.push(value.clone());
        }

        Ok((values, Vec::new(), next_token))
    }

    async fn read_raw_reverse(
        &self,
        node_id: &NodeId,
        at_or_before: DateTime,
        num_values_per_node: u32,
    ) -> Result<Vec<DataValue>, StatusCode> {
        let Some(node_values) = self.raw_values.read().get(node_id).cloned() else {
            return Ok(Vec::new());
        };

        let limit = (num_values_per_node > 0).then_some(num_values_per_node as usize);
        let iter = node_values.range(..=at_or_before.ticks()).rev();
        let values = match limit {
            Some(limit) => iter.take(limit).map(|(_, value)| value.clone()).collect(),
            None => iter.map(|(_, value)| value.clone()).collect(),
        };

        Ok(values)
    }

    async fn read_annotations(
        &self,
        node_id: &NodeId,
        req_times: &[DateTime],
        continuation_point: Option<Vec<u8>>,
    ) -> Result<(Vec<DataValue>, Option<Vec<u8>>), StatusCode> {
        let annotation_values = self.annotation_values.read();
        let node_values = annotation_values.get(node_id);
        if req_times.is_empty() {
            if continuation_point.is_some() {
                return Err(StatusCode::BadContinuationPointInvalid);
            }
            return Ok((
                node_values
                    .into_iter()
                    .flat_map(BTreeMap::values)
                    .cloned()
                    .collect(),
                None,
            ));
        }

        let resume =
            AnnotationContinuationPoint::decode(continuation_point.as_deref(), req_times.len())?;
        let (page_range, next_token) = resume.page(req_times.len());

        let Some(node_values) = node_values else {
            return Ok((Vec::new(), next_token));
        };

        let mut values = Vec::with_capacity(page_range.len());
        for req_time in &req_times[page_range] {
            if let Some(value) = node_values.get(&req_time.ticks()) {
                values.push(value.clone());
            }
        }

        Ok((values, next_token))
    }

    async fn update_data(
        &self,
        node_id: &NodeId,
        perform_insert_replace: PerformUpdateType,
        values: Vec<DataValue>,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        if values.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(values.len());
        let mut raw_values = self.raw_values.write();
        let node_values = raw_values.entry(node_id.clone()).or_default();

        for value in values {
            let source_ticks = source_ticks(&value);
            let status = match perform_insert_replace {
                PerformUpdateType::Insert => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        node_values.entry(source_ticks)
                    {
                        entry.insert(value);
                        self.enforce_raw_capacity(node_values);
                        StatusCode::GoodEntryInserted
                    } else {
                        StatusCode::BadEntryExists
                    }
                }
                PerformUpdateType::Replace => {
                    if let Some(old_value) = node_values.get(&source_ticks).cloned() {
                        self.record_modified(
                            node_id,
                            source_ticks,
                            old_value,
                            HistoryUpdateType::Replace,
                        );
                        node_values.insert(source_ticks, value);
                        StatusCode::GoodEntryReplaced
                    } else {
                        StatusCode::BadNoEntryExists
                    }
                }
                PerformUpdateType::Update => {
                    if let Some(old_value) = node_values.get(&source_ticks).cloned() {
                        self.record_modified(
                            node_id,
                            source_ticks,
                            old_value,
                            HistoryUpdateType::Update,
                        );
                        node_values.insert(source_ticks, value);
                        StatusCode::GoodEntryReplaced
                    } else {
                        node_values.insert(source_ticks, value);
                        self.enforce_raw_capacity(node_values);
                        StatusCode::GoodEntryInserted
                    }
                }
                PerformUpdateType::Remove => {
                    if let Some(value) = node_values.remove(&source_ticks) {
                        self.modified_values
                            .write()
                            .entry(node_id.clone())
                            .or_default()
                            .entry(source_ticks)
                            .or_default()
                            .push((value, delete_modification_info()));
                        StatusCode::Good
                    } else {
                        StatusCode::BadNoEntryExists
                    }
                }
            };
            results.push(status);
        }

        Ok(results)
    }

    async fn update_structure_data(
        &self,
        node_id: &NodeId,
        perform: PerformUpdateType,
        values: Vec<DataValue>,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        if values.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(values.len());
        let mut annotation_values = self.annotation_values.write();
        let node_values = annotation_values.entry(node_id.clone()).or_default();

        for value in values {
            let source_ticks = source_ticks(&value);
            let status = match perform {
                PerformUpdateType::Insert => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        node_values.entry(source_ticks)
                    {
                        entry.insert(value);
                        StatusCode::GoodEntryInserted
                    } else {
                        StatusCode::BadEntryExists
                    }
                }
                PerformUpdateType::Replace => {
                    if let std::collections::btree_map::Entry::Occupied(mut entry) =
                        node_values.entry(source_ticks)
                    {
                        entry.insert(value);
                        StatusCode::GoodEntryReplaced
                    } else {
                        StatusCode::BadNoEntryExists
                    }
                }
                PerformUpdateType::Update => {
                    if let std::collections::btree_map::Entry::Occupied(mut entry) =
                        node_values.entry(source_ticks)
                    {
                        entry.insert(value);
                        StatusCode::GoodEntryReplaced
                    } else {
                        node_values.insert(source_ticks, value);
                        StatusCode::GoodEntryInserted
                    }
                }
                PerformUpdateType::Remove => {
                    if node_values.remove(&source_ticks).is_some() {
                        StatusCode::Good
                    } else {
                        StatusCode::BadNoEntryExists
                    }
                }
            };
            results.push(status);
        }

        Ok(results)
    }

    async fn delete_raw_modified(
        &self,
        node_id: &NodeId,
        is_delete_modified: bool,
        start_time: DateTime,
        end_time: DateTime,
    ) -> Result<StatusCode, StatusCode> {
        let start_ticks = start_time.ticks();
        let end_ticks = end_time.ticks();
        if start_ticks >= end_ticks {
            return Ok(StatusCode::BadNoData);
        }

        if is_delete_modified {
            let mut modified_values = self.modified_values.write();
            let Some(node_modified_values) = modified_values.get_mut(node_id) else {
                return Ok(StatusCode::BadNoData);
            };

            let ticks_to_remove = node_modified_values
                .range(start_ticks..end_ticks)
                .map(|(tick, _)| *tick)
                .collect::<Vec<_>>();

            let mut removed_count = 0;
            for tick in ticks_to_remove {
                if let Some(entries) = node_modified_values.remove(&tick) {
                    removed_count += entries.len();
                }
            }

            if node_modified_values.is_empty() {
                modified_values.remove(node_id);
            }

            return Ok(if removed_count > 0 {
                StatusCode::Good
            } else {
                StatusCode::BadNoData
            });
        }

        let mut raw_values = self.raw_values.write();
        let Some(node_values) = raw_values.get_mut(node_id) else {
            return Ok(StatusCode::BadNoData);
        };

        let values_to_delete = node_values
            .range(start_ticks..end_ticks)
            .map(|(tick, value)| (*tick, value.clone()))
            .collect::<Vec<_>>();

        if values_to_delete.is_empty() {
            return Ok(StatusCode::BadNoData);
        }

        {
            let mut modified_values = self.modified_values.write();
            let node_modified_values = modified_values.entry(node_id.clone()).or_default();
            for (tick, value) in &values_to_delete {
                node_modified_values
                    .entry(*tick)
                    .or_default()
                    .push((value.clone(), delete_modification_info()));
            }
        }

        for (tick, _) in values_to_delete {
            node_values.remove(&tick);
        }

        Ok(StatusCode::Good)
    }

    async fn delete_at_time(
        &self,
        node_id: &NodeId,
        req_times: Vec<DateTime>,
    ) -> Result<Vec<StatusCode>, StatusCode> {
        if req_times.is_empty() {
            return Ok(Vec::new());
        }

        let mut status_codes = Vec::with_capacity(req_times.len());
        let mut raw_values = self.raw_values.write();
        let Some(node_values) = raw_values.get_mut(node_id) else {
            for _ in req_times {
                status_codes.push(StatusCode::BadNoEntryExists);
            }
            return Ok(status_codes);
        };

        let mut modified_values = self.modified_values.write();
        for req_time in req_times {
            let source_ticks = req_time.ticks();
            if let Some(value) = node_values.remove(&source_ticks) {
                modified_values
                    .entry(node_id.clone())
                    .or_default()
                    .entry(source_ticks)
                    .or_default()
                    .push((value, delete_modification_info()));
                status_codes.push(StatusCode::Good);
            } else {
                status_codes.push(StatusCode::BadNoEntryExists);
            }
        }

        Ok(status_codes)
    }
}

fn source_ticks(value: &DataValue) -> i64 {
    value.source_timestamp.unwrap_or_else(DateTime::now).ticks()
}

fn delete_modification_info() -> ModificationInfo {
    ModificationInfo {
        modification_time: DateTime::now(),
        update_type: HistoryUpdateType::Delete,
        // ponytail: user_name is empty because the storage layer has no request context.
        user_name: UAString::null(),
    }
}

fn decode_continuation_tick(token: Option<Vec<u8>>) -> Result<Option<i64>, StatusCode> {
    let Some(token) = token else {
        return Ok(None);
    };
    let bytes: [u8; 8] = token
        .as_slice()
        .try_into()
        .map_err(|_| StatusCode::BadContinuationPointInvalid)?;
    Ok(Some(i64::from_le_bytes(bytes)))
}

fn encode_continuation_tick(tick: i64) -> Vec<u8> {
    tick.to_le_bytes().to_vec()
}

fn modified_effective_start(
    start_tick: i64,
    continuation_position: Option<(i64, usize)>,
) -> (i64, usize) {
    match continuation_position {
        Some((tick, entry_index)) if tick >= start_tick => (tick, entry_index),
        _ => (start_tick, 0),
    }
}

fn decode_modified_continuation_position(
    token: Option<Vec<u8>>,
) -> Result<Option<(i64, usize)>, StatusCode> {
    let Some(token) = token else {
        return Ok(None);
    };

    match token.len() {
        8 => {
            let tick = decode_continuation_tick(Some(token))?;
            Ok(tick.map(|tick| (tick, 0)))
        }
        16 => {
            let tick_bytes: [u8; 8] = token[..8]
                .try_into()
                .map_err(|_| StatusCode::BadContinuationPointInvalid)?;
            let index_bytes: [u8; 8] = token[8..]
                .try_into()
                .map_err(|_| StatusCode::BadContinuationPointInvalid)?;
            let entry_index = usize::try_from(u64::from_le_bytes(index_bytes))
                .map_err(|_| StatusCode::BadContinuationPointInvalid)?;
            Ok(Some((i64::from_le_bytes(tick_bytes), entry_index)))
        }
        _ => Err(StatusCode::BadContinuationPointInvalid),
    }
}

fn encode_modified_continuation_position(tick: i64, entry_index: usize) -> Vec<u8> {
    let mut token = encode_continuation_tick(tick);
    if entry_index > 0 {
        token.extend_from_slice(&(entry_index as u64).to_le_bytes());
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_constructs_in_memory_data_history() {
        let _history = InMemoryDataHistory::default();
    }

    fn dv_at(ticks: i64, value: i32) -> DataValue {
        DataValue {
            value: Some(Variant::Int32(value)),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::from(ticks)),
            server_timestamp: Some(DateTime::from(ticks)),
            ..Default::default()
        }
    }

    fn annotation_at(ticks: i64) -> DataValue {
        let annotation = Annotation {
            message: format!("annotation-{ticks}").into(),
            user_name: "tester".into(),
            annotation_time: DateTime::from(ticks),
        };
        DataValue::new_at(
            opcua_types::ExtensionObject::from_message(annotation),
            DateTime::from(ticks),
        )
    }

    #[tokio::test]
    async fn read_annotations_resumes_from_continuation_point() {
        // Given: more requested annotation timestamps than one server batch.
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "annotation-paged");
        let req_times = (0..1_001i64).map(DateTime::from).collect::<Vec<_>>();
        history
            .update_structure_data(
                &node_id,
                PerformUpdateType::Insert,
                (0..1_001i64).map(annotation_at).collect(),
            )
            .await
            .expect("insert annotations");

        // When: the first page is read and its continuation point is resumed.
        let (first_page, continuation_point) = history
            .read_annotations(&node_id, &req_times, None)
            .await
            .expect("read first annotation page");
        let continuation_point = continuation_point.expect("first page continuation point");
        let (second_page, final_continuation_point) = history
            .read_annotations(&node_id, &req_times, Some(continuation_point))
            .await
            .expect("resume annotation read");

        // Then: pagination returns every requested annotation exactly once.
        assert_eq!(first_page.len(), 1_000);
        assert_eq!(second_page.len(), 1);
        assert_eq!(source_ticks(&first_page[0]), 0);
        assert_eq!(source_ticks(&first_page[999]), 999);
        assert_eq!(source_ticks(&second_page[0]), 1_000);
        assert!(final_continuation_point.is_none());
    }

    #[tokio::test]
    async fn read_raw_reverse_returns_nearest_values_descending() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "reverse-test");
        history
            .update_data(
                &node_id,
                PerformUpdateType::Insert,
                vec![dv_at(10, 1), dv_at(20, 2), dv_at(30, 3)],
            )
            .await
            .expect("insert should succeed");

        let values = history
            .read_raw_reverse(&node_id, DateTime::from(25), 10)
            .await
            .expect("read_raw_reverse should succeed");
        assert_eq!(
            values.iter().map(|v| v.value.clone()).collect::<Vec<_>>(),
            vec![Some(Variant::Int32(2)), Some(Variant::Int32(1))],
            "should return values at or before the timestamp, closest first"
        );
    }

    #[tokio::test]
    async fn read_raw_reverse_includes_exact_match_at_boundary() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "reverse-exact");
        history
            .update_data(&node_id, PerformUpdateType::Insert, vec![dv_at(20, 2)])
            .await
            .expect("insert should succeed");

        let values = history
            .read_raw_reverse(&node_id, DateTime::from(20), 10)
            .await
            .expect("read_raw_reverse should succeed");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].value, Some(Variant::Int32(2)));
    }

    #[tokio::test]
    async fn read_raw_reverse_truncates_at_num_values_per_node() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "reverse-truncate");
        history
            .update_data(
                &node_id,
                PerformUpdateType::Insert,
                vec![dv_at(10, 1), dv_at(20, 2), dv_at(30, 3)],
            )
            .await
            .expect("insert should succeed");

        let values = history
            .read_raw_reverse(&node_id, DateTime::from(30), 2)
            .await
            .expect("read_raw_reverse should succeed");
        assert_eq!(
            values.iter().map(|v| v.value.clone()).collect::<Vec<_>>(),
            vec![Some(Variant::Int32(3)), Some(Variant::Int32(2))]
        );
    }

    #[tokio::test]
    async fn read_raw_reverse_returns_empty_when_nothing_at_or_before() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "reverse-empty");
        history
            .update_data(&node_id, PerformUpdateType::Insert, vec![dv_at(20, 2)])
            .await
            .expect("insert should succeed");

        let values = history
            .read_raw_reverse(&node_id, DateTime::from(10), 10)
            .await
            .expect("read_raw_reverse should succeed");
        assert!(values.is_empty());
    }

    #[tokio::test]
    async fn read_raw_reverse_returns_empty_for_unknown_node() {
        let history = InMemoryDataHistory::new();
        let values = history
            .read_raw_reverse(&NodeId::new(2, "never-seen"), DateTime::from(100), 10)
            .await
            .expect("read_raw_reverse should succeed");
        assert!(values.is_empty());
    }

    // Feature 035 / FR-007: a backend that does not support annotations (uses the default
    // `read_annotations`, which returns Bad_HistoryOperationUnsupported) must make AnnotationCount
    // return 0, never propagate the error.
    struct NoAnnotationBackend;

    #[async_trait]
    impl HistoryStorageBackend for NoAnnotationBackend {
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
        // read_annotations + read_processed use the trait defaults.
    }

    #[tokio::test]
    async fn annotation_count_is_zero_when_backend_has_no_annotation_support() {
        let b = NoAnnotationBackend;
        let (vals, _cp) = b
            .read_processed(
                &NodeId::new(2, "x"),
                DateTime::from(0),
                DateTime::from(100_000_000),
                100_000.0,
                &NodeId::new(0u16, 2351u32),
                &opcua_types::AggregateConfiguration::default(),
                true,
                None,
            )
            .await
            .expect("read_processed must not error even without annotation support");
        // Every interval reports 0 (the annotation read failed → empty set), all Good — no error/panic.
        assert!(vals
            .iter()
            .all(|v| v.value == Some(Variant::Int32(0)) && v.status == Some(StatusCode::Good)));
    }

    // Feature 107 / OPC-10000-11 §6.5.5.2: `read_at_time`'s default impl, exercised through a
    // real `InMemoryDataHistory` backend (it doesn't override the default, so this is the real
    // implementation, not a mock).

    fn dv_bad_at(ticks: i64, value: i32) -> DataValue {
        DataValue {
            value: Some(Variant::Int32(value)),
            status: Some(StatusCode::BadNoData),
            source_timestamp: Some(DateTime::from(ticks)),
            server_timestamp: Some(DateTime::from(ticks)),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn read_at_time_exact_match_is_marked_raw() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-exact");
        history
            .update_data(&node_id, PerformUpdateType::Insert, vec![dv_at(20, 2)])
            .await
            .unwrap();

        let (values, next) = history
            .read_at_time(&node_id, &[DateTime::from(20)], false, false, None)
            .await
            .expect("read_at_time should succeed");
        assert!(next.is_none());
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].value, Some(Variant::Int32(2)));
        assert_eq!(
            values[0].status.map(|s| s.value_type()),
            Some(opcua_types::StatusCodeValueType::Raw)
        );
    }

    #[tokio::test]
    async fn read_at_time_interpolates_between_bounds_when_not_stepped() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-interp");
        history
            .update_data(
                &node_id,
                PerformUpdateType::Insert,
                vec![dv_at(0, 10), dv_at(10, 20)],
            )
            .await
            .unwrap();

        let (values, _next) = history
            .read_at_time(&node_id, &[DateTime::from(5)], false, false, None)
            .await
            .expect("read_at_time should succeed");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].value, Some(Variant::Int32(15)));
        assert_eq!(
            values[0].status.map(|s| s.value_type()),
            Some(opcua_types::StatusCodeValueType::Interpolated)
        );
    }

    #[tokio::test]
    async fn read_at_time_simple_bounds_uses_immediate_neighbor_even_if_bad() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-simple-bad");
        history
            .update_data(
                &node_id,
                PerformUpdateType::Insert,
                vec![dv_at(0, 10), dv_bad_at(5, 999), dv_at(10, 20)],
            )
            .await
            .unwrap();

        // use_simple_bounds = true: the immediately-adjacent sample at 5 is itself Bad, so no
        // substitution further afield -- Bad_NoData for a request between 5 and 10.
        let (values, _next) = history
            .read_at_time(&node_id, &[DateTime::from(7)], true, false, None)
            .await
            .expect("read_at_time should succeed");
        assert_eq!(values[0].status, Some(StatusCode::BadNoData));
    }

    #[tokio::test]
    async fn read_at_time_interpolated_bounds_search_past_bad_quality() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-search-past-bad");
        history
            .update_data(
                &node_id,
                PerformUpdateType::Insert,
                vec![dv_at(0, 10), dv_bad_at(5, 999), dv_at(10, 20)],
            )
            .await
            .unwrap();

        // use_simple_bounds = false, requesting T=6 (not T=5, which would be an exact match
        // against the Bad sample itself -- a value found exactly at the requested timestamp is
        // Raw regardless of its own quality, per spec; that's a different case, covered above).
        // The Bad sample at 5 is skipped, searching outward to the Good samples at 0 and 10 --
        // interpolating to 16 at T=6.
        let (values, _next) = history
            .read_at_time(&node_id, &[DateTime::from(6)], false, false, None)
            .await
            .expect("read_at_time should succeed");
        assert_eq!(values[0].value, Some(Variant::Int32(16)));
        assert_eq!(
            values[0].status.map(|s| s.value_type()),
            Some(opcua_types::StatusCodeValueType::Interpolated)
        );
    }

    #[tokio::test]
    async fn read_at_time_returns_bad_no_data_outside_recorded_range() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-out-of-range");
        history
            .update_data(&node_id, PerformUpdateType::Insert, vec![dv_at(10, 1)])
            .await
            .unwrap();

        let (values, _next) = history
            .read_at_time(
                &node_id,
                &[DateTime::from(0), DateTime::from(1000)],
                false,
                false,
                None,
            )
            .await
            .expect("read_at_time should succeed");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].status, Some(StatusCode::BadNoData));
        assert_eq!(values[1].status, Some(StatusCode::BadNoData));
    }

    #[tokio::test]
    async fn read_at_time_resolves_multiple_timestamps_independently_in_request_order() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-multi");
        history
            .update_data(
                &node_id,
                PerformUpdateType::Insert,
                vec![dv_at(0, 10), dv_at(10, 20)],
            )
            .await
            .unwrap();

        // Duplicate + out-of-order requested timestamps: 5 (interpolated), 0 (exact),
        // -100 (out of range), 5 again (interpolated, repeated).
        let req_times = vec![
            DateTime::from(5),
            DateTime::from(0),
            DateTime::from(-100),
            DateTime::from(5),
        ];
        let (values, _next) = history
            .read_at_time(&node_id, &req_times, false, false, None)
            .await
            .expect("read_at_time should succeed");
        assert_eq!(values.len(), 4);
        assert_eq!(values[0].value, Some(Variant::Int32(15)));
        assert_eq!(values[1].value, Some(Variant::Int32(10)));
        assert_eq!(values[2].status, Some(StatusCode::BadNoData));
        assert_eq!(values[3].value, Some(Variant::Int32(15)));
    }

    #[tokio::test]
    async fn read_at_time_stepped_node_step_holds_any_variant_type() {
        // CU 2991: a structured/non-numeric historized value resolves via the Stepped path,
        // which never touches numeric interpolation at all.
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-stepped-structured");
        let structured = DataValue {
            value: Some(Variant::from(UAString::from("log entry"))),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::from(0)),
            server_timestamp: Some(DateTime::from(0)),
            ..Default::default()
        };
        history
            .update_data(&node_id, PerformUpdateType::Insert, vec![structured])
            .await
            .unwrap();

        let (values, _next) = history
            .read_at_time(&node_id, &[DateTime::from(50)], false, true, None)
            .await
            .expect("read_at_time should succeed");
        assert_eq!(
            values[0].value,
            Some(Variant::from(UAString::from("log entry")))
        );
        assert_eq!(
            values[0].status.map(|s| s.value_type()),
            Some(opcua_types::StatusCodeValueType::Interpolated)
        );
    }

    #[tokio::test]
    async fn read_at_time_rejects_invalid_continuation_point() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-bad-cp");

        let err = history
            .read_at_time(
                &node_id,
                &[DateTime::from(0)],
                false,
                false,
                Some(vec![1, 2, 3]), // not a valid 8-byte resume index
            )
            .await
            .expect_err("a malformed continuation point must be rejected");
        assert_eq!(err, StatusCode::BadContinuationPointInvalid);
    }

    #[tokio::test]
    async fn read_at_time_pages_across_multiple_calls_via_continuation_point() {
        let history = InMemoryDataHistory::new();
        let node_id = NodeId::new(2, "at-time-paged");
        history
            .update_data(&node_id, PerformUpdateType::Insert, vec![dv_at(0, 42)])
            .await
            .unwrap();

        // More requested timestamps than one batch (BATCH_LIMIT = 1_000) -- every one resolves
        // the same way (out of range), so only the *count* and continuation-token chaining are
        // under test here, not per-value correctness (already covered by the tests above).
        let req_times: Vec<DateTime> = (0..1_500i64).map(DateTime::from).collect();

        let (first_batch, cp) = history
            .read_at_time(&node_id, &req_times, false, false, None)
            .await
            .expect("first batch should succeed");
        assert_eq!(first_batch.len(), 1_000);
        let cp =
            cp.expect("more than one batch worth of timestamps must yield a continuation token");

        let (second_batch, cp2) = history
            .read_at_time(&node_id, &req_times, false, false, Some(cp))
            .await
            .expect("second batch should succeed");
        assert_eq!(second_batch.len(), 500);
        assert!(cp2.is_none(), "no further batches remain");
    }
}
