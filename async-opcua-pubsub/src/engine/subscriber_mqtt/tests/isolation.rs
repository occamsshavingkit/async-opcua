use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_server::address_space::AddressSpace;
use opcua_types::{ContextOwned, MessageSecurityMode, NodeId, PubSubState, Variant};

use super::{
    connection, connection_with_id, forward_payload, forward_processed_payload, insert_target,
    reader, target_value,
};
use crate::{
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    engine::subscriber::SubscriberDatagramProcessor,
    DataSetReaderConfig, DataSetReaderKey, MessageEncoding, PubSubEngine, SecurityGroup,
    SubscriberError, SubscriberRuntime,
};
use opcua_types::BinaryEncodable;

fn runtime(
    address_space: Arc<RwLock<AddressSpace>>,
    readers: Vec<DataSetReaderConfig>,
) -> Arc<RwLock<SubscriberRuntime>> {
    Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(address_space, vec![connection(readers)])
            .expect("heterogeneous reader fixture should be valid"),
    ))
}

fn uadp_payload(dataset_writer_id: u16, value: f64) -> Vec<u8> {
    let message = UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(value)],
        }],
    };
    message.encode_to_vec(&ContextOwned::default().context())
}

#[tokio::test]
async fn mqtt_forwarder_rejects_plaintext_for_secured_owner() {
    // Given a secured MQTT reader and a plaintext UADP payload that otherwise matches it.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "SecuredOwnerTarget");
    let address_space = Arc::new(RwLock::new(space));
    let mut owner = reader(1, MessageEncoding::Uadp, target.clone());
    owner.security_mode = Some(MessageSecurityMode::SignAndEncrypt);
    owner.security_policy_uri = Some(SecurityPolicy::PubSubAes256Ctr.to_uri().to_string());
    owner.security_group_id = Some("line-a".to_string());
    let runtime = runtime(address_space.clone(), vec![owner.clone()]);
    let config = connection(vec![owner.clone()]);
    let mut engine = PubSubEngine::with_connections(address_space.clone(), vec![config.clone()]);
    engine.register_security_group(
        SecurityGroup::new("line-a", Duration::from_secs(3600))
            .expect("security group fixture should be valid"),
    );
    let security = engine
        .prepare_subscriber_security_processor(&config)
        .expect("secured reader fixture should prepare");
    let processor =
        SubscriberDatagramProcessor::new(runtime.clone(), "mqtt-conn", vec![owner], security);

    // When the owning forwarder receives the unauthenticated payload.
    forward_processed_payload(processor, 1, uadp_payload(42, 23.5)).await;

    // Then the secured reader rejects it without mutating its target.
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .read()
            .reader_status(1)
            .expect("owner status should exist")
            .security_failure_count,
        1
    );
}

#[tokio::test]
async fn mqtt_forwarder_keeps_duplicate_numeric_ids_connection_scoped() {
    // Given two MQTT connections whose readers share a numeric id and filter.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstConnectionTarget");
    let second_target = insert_target(&space, "SecondConnectionTarget");
    let address_space = Arc::new(RwLock::new(space));
    let owner = reader(1, MessageEncoding::Uadp, first_target.clone());
    let unrelated = reader(1, MessageEncoding::Uadp, second_target.clone());
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(
            address_space.clone(),
            vec![
                connection_with_id("first-mqtt", vec![owner.clone()]),
                connection_with_id("second-mqtt", vec![unrelated]),
            ],
        )
        .expect("duplicate numeric ids on distinct connections should be valid"),
    ));
    let processor =
        SubscriberDatagramProcessor::new(runtime.clone(), "first-mqtt", vec![owner], None);

    // When the first connection's forwarder receives a matching payload.
    forward_processed_payload(processor, 1, uadp_payload(42, 23.5)).await;

    // Then only the first connection's target and status are updated.
    assert_eq!(
        target_value(&address_space.read(), &first_target),
        Some(Variant::Double(23.5))
    );
    assert_eq!(
        target_value(&address_space.read(), &second_target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .read()
            .reader_status_by_key(&DataSetReaderKey::new("first-mqtt", 1))
            .expect("first reader status should exist")
            .accepted_count,
        1
    );
    assert_eq!(
        runtime
            .read()
            .reader_status_by_key(&DataSetReaderKey::new("second-mqtt", 1))
            .expect("second reader status should exist")
            .accepted_count,
        0
    );
}

#[tokio::test]
async fn mqtt_forwarder_applies_json_only_to_owner_when_uadp_reader_exists() {
    // Given a JSON MQTT reader and an unrelated UADP reader in the same runtime.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let json_target = insert_target(&space, "JsonOwnerTarget");
    let uadp_target = insert_target(&space, "UadpUnrelatedTarget");
    let address_space = Arc::new(RwLock::new(space));
    let owner = reader(1, MessageEncoding::Json, json_target.clone());
    let unrelated = reader(2, MessageEncoding::Uadp, uadp_target.clone());
    let runtime = runtime(address_space.clone(), vec![owner.clone(), unrelated]);
    let payload = serde_json::to_vec(&serde_json::json!({
        "MessageId": "msg-1",
        "MessageType": "ua-data",
        "PublisherId": "11",
        "WriterGroupId": 7,
        "Messages": [{
            "DataSetWriterId": 42,
            "SequenceNumber": 1,
            "Payload": {"Field": 23.5}
        }]
    }))
    .expect("JSON test payload should encode");

    // When the owning forwarder receives one JSON payload.
    forward_payload(runtime.clone(), owner, payload).await;

    // Then only the owning JSON reader applies the payload.
    assert_eq!(
        target_value(&address_space.read(), &json_target),
        Some(Variant::Double(23.5))
    );
    assert_eq!(
        target_value(&address_space.read(), &uadp_target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .read()
            .reader_status(2)
            .expect("unrelated reader status should exist")
            .filtered_count,
        0
    );
}

#[tokio::test]
async fn mqtt_forwarder_records_filter_only_for_owner() {
    // Given two UADP readers and a payload that does not match the owning reader.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let owner_target = insert_target(&space, "FilteredOwnerTarget");
    let unrelated_target = insert_target(&space, "FilteredUnrelatedTarget");
    let address_space = Arc::new(RwLock::new(space));
    let owner = reader(1, MessageEncoding::Uadp, owner_target);
    let unrelated = reader(2, MessageEncoding::Uadp, unrelated_target);
    let runtime = runtime(address_space, vec![owner.clone(), unrelated]);

    // When the owning forwarder receives a payload for another DataSetWriter.
    forward_payload(runtime.clone(), owner, uadp_payload(99, 1.0)).await;

    // Then only the owning reader records the filter.
    let runtime = runtime.read();
    assert_eq!(
        runtime
            .reader_status(1)
            .expect("owner status should exist")
            .filtered_count,
        1
    );
    assert_eq!(
        runtime
            .reader_status(2)
            .expect("unrelated status should exist")
            .filtered_count,
        0
    );
}

#[tokio::test]
async fn mqtt_forwarder_records_application_error_only_for_owner() {
    // Given an owning reader with a missing target and an unrelated reader.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let owner = reader(
        1,
        MessageEncoding::Uadp,
        NodeId::new(1, "MissingOwnerTarget"),
    );
    let unrelated = reader(
        2,
        MessageEncoding::Uadp,
        NodeId::new(1, "MissingUnrelatedTarget"),
    );
    let runtime = runtime(address_space, vec![owner.clone(), unrelated]);

    // When the owning forwarder receives a matching payload.
    forward_payload(runtime.clone(), owner, uadp_payload(42, 1.0)).await;

    // Then only the owner enters error state with a dropped application.
    let runtime = runtime.read();
    let owner_status = runtime.reader_status(1).expect("owner status should exist");
    assert_eq!(owner_status.state, PubSubState::Error);
    assert_eq!(
        owner_status.last_error,
        Some(SubscriberError::TargetNotFound)
    );
    assert_eq!(owner_status.dropped_count, 1);
    let unrelated_status = runtime
        .reader_status(2)
        .expect("unrelated status should exist");
    assert_eq!(unrelated_status.state, PubSubState::PreOperational);
    assert_eq!(unrelated_status.last_error, None);
    assert_eq!(unrelated_status.dropped_count, 0);
}
