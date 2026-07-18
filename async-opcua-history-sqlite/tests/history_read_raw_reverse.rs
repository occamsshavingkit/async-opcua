//! Tests for `SqliteHistoryBackend::read_raw_reverse` (feature 107, `specs/107-history-read-at-time/`).
//! Mirrors the in-memory backend's equivalent test coverage so both backends are proven
//! identical for this new bounding-value primitive.

use opcua_history_sqlite::SqliteHistoryBackend;
use opcua_server::history::HistoryStorageBackend;
use opcua_types::{DataValue, DateTime, NodeId, PerformUpdateType, Variant};

fn at(ticks: i64, v: i32) -> DataValue {
    DataValue::new_at(Variant::from(v), DateTime::from(ticks))
}

fn node() -> NodeId {
    NodeId::new(2, "ReadRawReverseVar")
}

fn values(dvs: &[DataValue]) -> Vec<Option<Variant>> {
    dvs.iter().map(|v| v.value.clone()).collect()
}

#[tokio::test]
async fn returns_nearest_values_descending() {
    let b = SqliteHistoryBackend::new_in_memory().unwrap();
    let n = node();
    b.update_data(
        &n,
        PerformUpdateType::Insert,
        vec![at(10, 1), at(20, 2), at(30, 3)],
    )
    .await
    .unwrap();

    let result = b
        .read_raw_reverse(&n, DateTime::from(25), 10)
        .await
        .expect("read_raw_reverse should succeed");
    assert_eq!(
        values(&result),
        vec![Some(Variant::Int32(2)), Some(Variant::Int32(1))]
    );
}

#[tokio::test]
async fn includes_exact_match_at_boundary() {
    let b = SqliteHistoryBackend::new_in_memory().unwrap();
    let n = node();
    b.update_data(&n, PerformUpdateType::Insert, vec![at(20, 2)])
        .await
        .unwrap();

    let result = b
        .read_raw_reverse(&n, DateTime::from(20), 10)
        .await
        .expect("read_raw_reverse should succeed");
    assert_eq!(values(&result), vec![Some(Variant::Int32(2))]);
}

#[tokio::test]
async fn truncates_at_num_values_per_node() {
    let b = SqliteHistoryBackend::new_in_memory().unwrap();
    let n = node();
    b.update_data(
        &n,
        PerformUpdateType::Insert,
        vec![at(10, 1), at(20, 2), at(30, 3)],
    )
    .await
    .unwrap();

    let result = b
        .read_raw_reverse(&n, DateTime::from(30), 2)
        .await
        .expect("read_raw_reverse should succeed");
    assert_eq!(
        values(&result),
        vec![Some(Variant::Int32(3)), Some(Variant::Int32(2))]
    );
}

#[tokio::test]
async fn returns_empty_when_nothing_at_or_before() {
    let b = SqliteHistoryBackend::new_in_memory().unwrap();
    let n = node();
    b.update_data(&n, PerformUpdateType::Insert, vec![at(20, 2)])
        .await
        .unwrap();

    let result = b
        .read_raw_reverse(&n, DateTime::from(10), 10)
        .await
        .expect("read_raw_reverse should succeed");
    assert!(result.is_empty());
}

#[tokio::test]
async fn returns_empty_for_unknown_node() {
    let b = SqliteHistoryBackend::new_in_memory().unwrap();
    let result = b
        .read_raw_reverse(&NodeId::new(2, "never-seen"), DateTime::from(100), 10)
        .await
        .expect("read_raw_reverse should succeed");
    assert!(result.is_empty());
}
