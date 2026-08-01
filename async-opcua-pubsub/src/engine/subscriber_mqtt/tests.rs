use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, ContextOwned, DataEncoding, DataTypeId, NodeId, NumericRange,
    TimestampsToReturn, Variant,
};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::MqttForwarder;
use crate::{
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    engine::subscriber::SubscriberDatagramProcessor,
    DataSetReaderConfig, FieldTargetConfig, MessageEncoding, PubSubConnectionConfig,
    ReaderGroupConfig, SubscriberRuntime,
};

fn insert_target(space: &AddressSpace, name: &str) -> NodeId {
    let node_id = NodeId::new(1, name);
    VariableBuilder::new(&node_id, name, name)
        .data_type(DataTypeId::Double)
        .value(Variant::Double(0.0))
        .insert(space);
    node_id
}

fn target_value(space: &AddressSpace, node_id: &NodeId) -> Option<Variant> {
    space
        .find(node_id)?
        .as_node()
        .get_attribute(
            TimestampsToReturn::Neither,
            AttributeId::Value,
            &NumericRange::None,
            &DataEncoding::Binary,
        )?
        .value
}

fn reader(
    dataset_reader_id: u16,
    message_encoding: MessageEncoding,
    target_node_id: NodeId,
) -> DataSetReaderConfig {
    DataSetReaderConfig {
        name: Some(format!("reader-{dataset_reader_id}")),
        dataset_reader_id,
        dataset_writer_id: 42,
        publisher_id: Some(PublisherId::UInt16(11)),
        writer_group_id: Some(7),
        network_message_number: Some(3),
        message_encoding,
        target_variables: vec![FieldTargetConfig::value(0, target_node_id)],
        ..DataSetReaderConfig::default()
    }
}

fn connection(readers: Vec<DataSetReaderConfig>) -> PubSubConnectionConfig {
    connection_with_id("mqtt-conn", readers)
}

fn connection_with_id(
    connection_id: &str,
    readers: Vec<DataSetReaderConfig>,
) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
        address: "mqtt://127.0.0.1:1883".to_string(),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: readers,
            ..ReaderGroupConfig::default()
        }],
    }
}

async fn forward_payload(
    runtime: Arc<RwLock<SubscriberRuntime>>,
    owner: DataSetReaderConfig,
    payload: Vec<u8>,
) {
    let reader_id = owner.dataset_reader_id;
    let processor = SubscriberDatagramProcessor::new(runtime, "mqtt-conn", vec![owner], None);
    forward_processed_payload(processor, reader_id, payload).await;
}

async fn forward_processed_payload(
    processor: SubscriberDatagramProcessor,
    reader_id: u16,
    payload: Vec<u8>,
) {
    forward_processed_payloads(processor, reader_id, vec![payload]).await;
}

async fn forward_processed_payloads(
    processor: SubscriberDatagramProcessor,
    reader_id: u16,
    payloads: Vec<Vec<u8>>,
) {
    let (payload_tx, payload_rx) = tokio::sync::mpsc::channel(payloads.len().max(1));
    let forwarder = MqttForwarder {
        processor,
        reader_id,
        payload_rx,
        cancel: CancellationToken::new(),
        connection_id: "mqtt-conn".to_string(),
        topic: "opcua/telemetry/7".to_string(),
    }
    .spawn();
    for payload in payloads {
        payload_tx
            .send(payload)
            .await
            .expect("bounded test channel should remain open");
    }
    drop(payload_tx);
    timeout(Duration::from_secs(1), forwarder)
        .await
        .expect("forwarder should stop when its receiver closes")
        .expect("forwarder should not panic");
}

#[tokio::test]
async fn mqtt_forwarder_applies_uadp_only_to_owner_when_json_reader_exists() {
    // Given a UADP MQTT reader and an unrelated JSON reader in the same runtime.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let uadp_target = insert_target(&space, "UadpTarget");
    let json_target = insert_target(&space, "JsonTarget");
    let address_space = Arc::new(RwLock::new(space));
    let owner = reader(1, MessageEncoding::Uadp, uadp_target.clone());
    let unrelated = reader(2, MessageEncoding::Json, json_target.clone());
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(
            address_space.clone(),
            vec![connection(vec![owner.clone(), unrelated])],
        )
        .expect("heterogeneous reader fixture should be valid"),
    ));
    let message = UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(17.5)],
        }],
    };
    let ctx_owned = ContextOwned::default();
    let payload = message.encode_to_vec(&ctx_owned.context());
    // When the owning forwarder receives one binary payload and its channel closes.
    forward_payload(runtime.clone(), owner, payload).await;

    // Then only the owning UADP reader applies the payload.
    assert_eq!(
        target_value(&address_space.read(), &uadp_target),
        Some(Variant::Double(17.5))
    );
    assert_eq!(
        target_value(&address_space.read(), &json_target),
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

mod isolation;
mod lifecycle;
mod security;
