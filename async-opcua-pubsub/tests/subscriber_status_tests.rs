//! Subscriber status and diagnostics integration tests.

use std::sync::Arc;
use std::time::{Duration, Instant};

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    DataSetReaderConfig, DataSetReaderKey, FieldTargetConfig, PubSubConnectionConfig, PublisherId,
    ReaderGroupConfig, SubscriberError, SubscriberRuntime, UadpDataSetMessage, UadpNetworkMessage,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    BinaryEncodable, ContextOwned, DataTypeId, NodeId, PubSubState, StatusCode, Variant,
};

fn target_space() -> (Arc<RwLock<AddressSpace>>, NodeId) {
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "Target");
    (Arc::new(RwLock::new(space)), target)
}

fn insert_target(space: &AddressSpace, name: &str) -> NodeId {
    let target = NodeId::new(1, name);
    VariableBuilder::new(&target, name, name)
        .data_type(DataTypeId::Double)
        .value(Variant::Double(0.0))
        .insert(space);
    target
}

fn connection(target: NodeId) -> PubSubConnectionConfig {
    scoped_connection(
        "conn",
        target,
        ReaderSettings {
            dataset_writer_id: 42,
            message_receive_timeout: Some(Duration::from_millis(10)),
            metadata_major_version: Some(1),
        },
    )
}

struct ReaderSettings {
    dataset_writer_id: u16,
    message_receive_timeout: Option<Duration>,
    metadata_major_version: Option<u32>,
}

fn scoped_connection(
    connection_id: &str,
    target: NodeId,
    settings: ReaderSettings,
) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
        address: "udp://127.0.0.1:4840".to_string(),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: vec![DataSetReaderConfig {
                name: Some("reader".to_string()),
                dataset_reader_id: 1,
                dataset_writer_id: settings.dataset_writer_id,
                publisher_id: Some(PublisherId::UInt16(11)),
                writer_group_id: Some(7),
                network_message_number: Some(3),
                message_receive_timeout: settings.message_receive_timeout,
                metadata_major_version: settings.metadata_major_version,
                target_variables: vec![FieldTargetConfig::value(0, target)],
                ..DataSetReaderConfig::default()
            }],
            ..ReaderGroupConfig::default()
        }],
    }
}

fn message(sequence_number: u16) -> UadpNetworkMessage {
    message_for_writer(42, sequence_number)
}

fn message_for_writer(dataset_writer_id: u16, sequence_number: u16) -> UadpNetworkMessage {
    UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id,
            sequence_number,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(sequence_number as f64)],
        }],
    }
}

#[test]
fn unscoped_process_network_message_rejects_multi_connection_runtime() {
    // Given a runtime whose readers belong to two distinct connections.
    let (space, target) = target_space();
    let mut runtime = SubscriberRuntime::with_connections(
        space,
        vec![
            connection(target.clone()),
            scoped_connection(
                "second",
                target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
        ],
    )
    .expect("distinct connection ids should construct a runtime");

    // When an already-decoded message is submitted without connection scope.
    let result = runtime.process_network_message(&message(1));

    // Then ambiguous ingress is rejected instead of dispatching across both connections.
    assert_eq!(result, Err(StatusCode::BadInvalidArgument));
}

#[test]
fn unscoped_process_datagram_rejects_multi_connection_runtime() {
    // Given a runtime whose readers belong to two distinct connections.
    let (space, target) = target_space();
    let mut runtime = SubscriberRuntime::with_connections(
        space,
        vec![
            connection(target.clone()),
            scoped_connection(
                "second",
                target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
        ],
    )
    .expect("distinct connection ids should construct a runtime");
    let context = ContextOwned::default();
    let payload = message(1).encode_to_vec(&context.context());

    // When an encoded datagram is submitted without connection scope.
    let result = runtime.process_datagram(&payload, &context.context());

    // Then ambiguous ingress is rejected instead of dispatching across both connections.
    assert_eq!(result, Err(StatusCode::BadInvalidArgument));
}

#[test]
fn duplicate_numeric_ids_keep_accepted_and_filtered_statuses_connection_scoped() {
    // Given two connections with the same numeric reader id and distinct filters.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstTarget");
    let second_target = insert_target(&space, "SecondTarget");
    let mut runtime = SubscriberRuntime::with_connections(
        Arc::new(RwLock::new(space)),
        vec![
            scoped_connection(
                "first",
                first_target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
            scoped_connection(
                "second",
                second_target,
                ReaderSettings {
                    dataset_writer_id: 99,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
        ],
    )
    .expect("duplicate numeric ids on distinct connections should be valid");
    let first_key = DataSetReaderKey::new("first", 1);
    let second_key = DataSetReaderKey::new("second", 1);
    let context = ContextOwned::default();
    let payload = message_for_writer(42, 1).encode_to_vec(&context.context());

    // When a message matches only the first connection's reader.
    runtime
        .process_datagram_for_connection("first", &payload, &context.context())
        .expect("first connection should process the message");
    runtime
        .process_datagram_for_connection("second", &payload, &context.context())
        .expect("second connection should filter the message");

    // Then status accounting remains independent and numeric lookup is ambiguous.
    let first = runtime
        .reader_status_by_key(&first_key)
        .expect("first reader status should exist");
    let second = runtime
        .reader_status_by_key(&second_key)
        .expect("second reader status should exist");
    assert_eq!((first.accepted_count, first.filtered_count), (1, 0));
    assert_eq!((second.accepted_count, second.filtered_count), (0, 1));
    assert_eq!(runtime.reader_status(1), None);
}

#[test]
fn duplicate_numeric_ids_keep_application_drops_connection_scoped() {
    // Given matching readers where only the first connection has a missing target.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let missing_target = NodeId::new(1, "MissingTarget");
    let valid_target = insert_target(&space, "ValidTarget");
    let mut runtime = SubscriberRuntime::with_connections(
        Arc::new(RwLock::new(space)),
        vec![
            scoped_connection(
                "first",
                missing_target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
            scoped_connection(
                "second",
                valid_target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
        ],
    )
    .expect("duplicate numeric ids on distinct connections should be valid");

    // When the shared filter matches both readers.
    runtime
        .process_network_message_for_connection("first", &message(1))
        .expect("first connection should process the message");
    runtime
        .process_network_message_for_connection("second", &message(1))
        .expect("second connection should process the message");

    // Then the failed application is charged only to its connection-scoped key.
    let first = runtime
        .reader_status_by_key(&DataSetReaderKey::new("first", 1))
        .expect("first reader status should exist");
    let second = runtime
        .reader_status_by_key(&DataSetReaderKey::new("second", 1))
        .expect("second reader status should exist");
    assert_eq!((first.accepted_count, first.dropped_count), (0, 1));
    assert_eq!((second.accepted_count, second.dropped_count), (1, 0));
}

#[test]
fn duplicate_numeric_ids_keep_timeout_configuration_connection_scoped() {
    // Given two operational readers with the same numeric id and distinct timeouts.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstTimeoutTarget");
    let second_target = insert_target(&space, "SecondTimeoutTarget");
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(
        Arc::new(RwLock::new(space)),
        vec![
            scoped_connection(
                "first",
                first_target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: Some(Duration::from_millis(10)),
                    metadata_major_version: None,
                },
            ),
            scoped_connection(
                "second",
                second_target,
                ReaderSettings {
                    dataset_writer_id: 99,
                    message_receive_timeout: Some(Duration::from_millis(100)),
                    metadata_major_version: None,
                },
            ),
        ],
    )
    .expect("duplicate numeric ids on distinct connections should be valid");
    runtime
        .process_network_message_for_connection_at("first", &message_for_writer(42, 1), now)
        .expect("first message should process");
    runtime
        .process_network_message_for_connection_at(
            "second",
            &message_for_writer(99, 1),
            now + Duration::from_millis(1),
        )
        .expect("second message should process");

    // When only the shorter timeout has elapsed.
    runtime.check_timeouts_at(now + Duration::from_millis(20));

    // Then only the owning key enters the timeout error state.
    let first = runtime
        .reader_status_by_key(&DataSetReaderKey::new("first", 1))
        .expect("first reader status should exist");
    let second = runtime
        .reader_status_by_key(&DataSetReaderKey::new("second", 1))
        .expect("second reader status should exist");
    assert_eq!(
        first.last_error,
        Some(SubscriberError::MessageReceiveTimeout)
    );
    assert_eq!(first.timeout_count, 1);
    assert_eq!(second.state, PubSubState::Operational);
    assert_eq!(second.timeout_count, 0);
}

#[test]
fn duplicate_numeric_ids_keep_metadata_state_connection_scoped() {
    // Given two readers with the same numeric id and distinct metadata versions.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstMetadataTarget");
    let second_target = insert_target(&space, "SecondMetadataTarget");
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(
        Arc::new(RwLock::new(space)),
        vec![
            scoped_connection(
                "first",
                first_target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: Some(Duration::from_millis(10)),
                    metadata_major_version: Some(1),
                },
            ),
            scoped_connection(
                "second",
                second_target,
                ReaderSettings {
                    dataset_writer_id: 99,
                    message_receive_timeout: Some(Duration::from_millis(10)),
                    metadata_major_version: Some(2),
                },
            ),
        ],
    )
    .expect("duplicate numeric ids on distinct connections should be valid");

    // When only the first reader observes an incompatible major version.
    runtime
        .observe_metadata_major_version_for_key_at(&DataSetReaderKey::new("first", 1), 9, now)
        .expect("first metadata observation should be accepted");
    runtime.check_timeouts_at(now + Duration::from_millis(11));

    // Then only the first key records the metadata error and legacy mutation is rejected.
    let first = runtime
        .reader_status_by_key(&DataSetReaderKey::new("first", 1))
        .expect("first reader status should exist");
    let second = runtime
        .reader_status_by_key(&DataSetReaderKey::new("second", 1))
        .expect("second reader status should exist");
    assert_eq!(
        first.last_error,
        Some(SubscriberError::MetadataMajorVersionMismatch)
    );
    assert_eq!(second.last_error, None);
    assert_eq!(
        runtime
            .observe_metadata_major_version_at(1, 3, now)
            .expect_err("ambiguous numeric metadata lookup must fail"),
        StatusCode::BadNotFound
    );
}

#[test]
fn duplicate_connection_id_with_distinct_reader_ids_returns_configuration_error() {
    // Given two connection records with the same connection id and distinct reader ids.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstConnectionTarget");
    let second_target = insert_target(&space, "SecondConnectionTarget");
    let first = scoped_connection(
        "duplicate",
        first_target,
        ReaderSettings {
            dataset_writer_id: 42,
            message_receive_timeout: None,
            metadata_major_version: None,
        },
    );
    let mut second = scoped_connection(
        "duplicate",
        second_target,
        ReaderSettings {
            dataset_writer_id: 99,
            message_receive_timeout: None,
            metadata_major_version: None,
        },
    );
    second.reader_groups[0].dataset_readers[0].dataset_reader_id = 2;

    // When runtime construction validates the connection identities.
    let result =
        SubscriberRuntime::with_connections(Arc::new(RwLock::new(space)), vec![first, second]);

    // Then duplicate connection ids are rejected independently of reader keys.
    assert_eq!(result.err(), Some(StatusCode::BadConfigurationError));
}

#[test]
fn duplicate_full_reader_key_returns_configuration_error() {
    // Given two connection records that would create the same full reader key.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstDuplicateTarget");
    let second_target = insert_target(&space, "SecondDuplicateTarget");

    // When runtime construction binds their readers.
    let result = SubscriberRuntime::with_connections(
        Arc::new(RwLock::new(space)),
        vec![
            scoped_connection(
                "duplicate",
                first_target,
                ReaderSettings {
                    dataset_writer_id: 42,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
            scoped_connection(
                "duplicate",
                second_target,
                ReaderSettings {
                    dataset_writer_id: 99,
                    message_receive_timeout: None,
                    metadata_major_version: None,
                },
            ),
        ],
    );

    // Then the fallible constructor reports configuration failure without panicking.
    assert_eq!(result.err(), Some(StatusCode::BadConfigurationError));
}

#[test]
fn first_valid_message_moves_reader_to_operational() {
    let (space, target) = target_space();
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();

    assert_eq!(
        runtime.reader_status(1).unwrap().state,
        PubSubState::PreOperational
    );
    runtime
        .process_network_message_at(&message(1), now)
        .unwrap();

    let status = runtime.reader_status(1).unwrap();
    assert_eq!(status.state, PubSubState::Operational);
    assert_eq!(status.accepted_count, 1);
}

#[test]
fn message_receive_timeout_moves_operational_reader_to_error() {
    let (space, target) = target_space();
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();
    runtime
        .process_network_message_at(&message(1), now)
        .unwrap();

    runtime.check_timeouts_at(now + Duration::from_millis(11));

    let status = runtime.reader_status(1).unwrap();
    assert_eq!(status.state, PubSubState::Error);
    assert_eq!(
        status.last_error,
        Some(SubscriberError::MessageReceiveTimeout)
    );
    assert_eq!(status.timeout_count, 1);
}

#[test]
fn next_valid_message_recovers_timeout_error() {
    let (space, target) = target_space();
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();
    runtime
        .process_network_message_at(&message(1), now)
        .unwrap();
    runtime.check_timeouts_at(now + Duration::from_millis(11));

    runtime
        .process_network_message_at(&message(2), now + Duration::from_millis(12))
        .unwrap();

    assert_eq!(
        runtime.reader_status(1).unwrap().state,
        PubSubState::Operational
    );
}

#[test]
fn sequence_gap_and_duplicate_are_diagnosed() {
    let (space, target) = target_space();
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();

    runtime
        .process_network_message_at(&message(1), now)
        .unwrap();
    runtime
        .process_network_message_at(&message(3), now + Duration::from_millis(1))
        .unwrap();
    runtime
        .process_network_message_at(&message(3), now + Duration::from_millis(2))
        .unwrap();

    let status = runtime.reader_status(1).unwrap();
    assert_eq!(status.sequence_gap_count, 1);
    assert_eq!(status.duplicate_count, 1);
    assert_eq!(status.last_sequence_number, Some(3));
}

#[test]
fn metadata_major_version_gap_errors_after_receive_timeout() {
    let (space, target) = target_space();
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();
    runtime
        .process_network_message_at(&message(1), now)
        .expect("first message");

    runtime
        .observe_metadata_major_version_at(1, 2, now + Duration::from_millis(1))
        .unwrap();
    runtime.check_timeouts_at(now + Duration::from_millis(12));

    let status = runtime.reader_status(1).unwrap();
    assert_eq!(status.state, PubSubState::Error);
    assert_eq!(
        status.last_error,
        Some(SubscriberError::MetadataMajorVersionMismatch)
    );
}

#[test]
fn duplicate_sequence_does_not_reset_receive_timeout() {
    let (space, target) = target_space();
    let now = Instant::now();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();
    runtime
        .process_network_message_at(&message(1), now)
        .unwrap();

    let mid = now + Duration::from_millis(5);
    runtime
        .process_network_message_at(&message(1), mid)
        .unwrap();

    let status = runtime.reader_status(1).unwrap();
    assert_eq!(status.duplicate_count, 1);
    assert_eq!(status.state, PubSubState::Operational);

    runtime.check_timeouts_at(now + Duration::from_millis(11));

    let status = runtime.reader_status(1).unwrap();
    assert_eq!(status.state, PubSubState::Error);
    assert_eq!(
        status.last_error,
        Some(SubscriberError::MessageReceiveTimeout)
    );
}

#[test]
fn unknown_reader_metadata_observation_returns_bad_not_found() {
    let (space, target) = target_space();
    let mut runtime = SubscriberRuntime::with_connections(space, vec![connection(target)]).unwrap();

    assert_eq!(
        runtime
            .observe_metadata_major_version_at(99, 2, Instant::now())
            .unwrap_err(),
        StatusCode::BadNotFound
    );
}
