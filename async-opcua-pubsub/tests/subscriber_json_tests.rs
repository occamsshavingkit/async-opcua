//! JSON subscriber runtime integration tests.
//!
//! Validates the end-to-end JSON PubSub subscriber pipeline described in
//! OPC-10000-14 §7.2.5.4: a JSON-encoded NetworkMessage datagram is decoded,
//! matched against configured DataSetReaders, and its payload fields are
//! applied to target Variables in the AddressSpace.

use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    DataSetReaderConfig, FieldTargetConfig, MessageEncoding, PubSubConnectionConfig, PublisherId,
    ReaderGroupConfig, SubscriberRuntime, UadpDataSetMessage, UadpNetworkMessage,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, ContextOwned, DataEncoding, DataTypeId, NodeId, NumericRange,
    TimestampsToReturn, Variant,
};

fn target_value(space: &AddressSpace, node: &NodeId) -> Option<Variant> {
    space
        .find(node)?
        .as_node()
        .get_attribute(
            TimestampsToReturn::Neither,
            AttributeId::Value,
            &NumericRange::None,
            &DataEncoding::Binary,
        )?
        .value
}

fn insert_target(space: &AddressSpace, name: &str, value: Variant) -> NodeId {
    let node_id = NodeId::new(1, name);
    VariableBuilder::new(&node_id, name, name)
        .data_type(DataTypeId::Double)
        .value(value)
        .insert(space);
    node_id
}

fn json_reader(targets: Vec<NodeId>) -> DataSetReaderConfig {
    DataSetReaderConfig {
        name: Some("json-reader-a".to_string()),
        dataset_reader_id: 1,
        dataset_writer_id: 100,
        publisher_id: Some(PublisherId::UInt16(42)),
        writer_group_id: Some(7),
        message_receive_timeout: Some(Duration::from_millis(100)),
        message_encoding: MessageEncoding::Json,
        target_variables: targets
            .into_iter()
            .enumerate()
            .map(|(index, target)| FieldTargetConfig::value(index, target))
            .collect(),
        ..DataSetReaderConfig::default()
    }
}

fn json_connection(reader: DataSetReaderConfig) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "json-conn".to_string(),
        name: "json-conn".to_string(),
        address: "udp://127.0.0.1:4840".to_string(),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: vec![reader],
            ..ReaderGroupConfig::default()
        }],
    }
}

fn mixed_connection(readers: Vec<DataSetReaderConfig>) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "mixed-conn".to_string(),
        name: "mixed-conn".to_string(),
        address: "udp://127.0.0.1:4840".to_string(),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: readers,
            ..ReaderGroupConfig::default()
        }],
    }
}

fn uadp_reader(target: NodeId) -> DataSetReaderConfig {
    DataSetReaderConfig {
        name: Some("uadp-reader-b".to_string()),
        dataset_reader_id: 2,
        dataset_writer_id: 42,
        publisher_id: Some(PublisherId::UInt16(11)),
        writer_group_id: Some(7),
        network_message_number: Some(3),
        message_receive_timeout: Some(Duration::from_millis(100)),
        target_variables: vec![FieldTargetConfig::value(0, target)],
        ..DataSetReaderConfig::default()
    }
}

fn uadp_datagram(payload: f64) -> Vec<u8> {
    UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(payload)],
        }],
    }
    .encode_to_vec(&ContextOwned::default().context())
}

/// A JSON NetworkMessage per OPC-10000-14 §7.2.5.4 with one DataSetMessage.
fn json_datagram(publisher_id: &str, payload_field_name: &str, payload_value: f64) -> Vec<u8> {
    format!(
        r#"{{"MessageId":"msg-1","MessageType":"ua-data","PublisherId":"{publisher_id}","WriterGroupId":7,"Messages":[{{"DataSetWriterId":100,"SequenceNumber":1,"Payload":{{"{payload_field_name}":{payload_value}}}}}]}}"#
    )
    .into_bytes()
}

#[test]
fn json_network_message_updates_target_variable() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();

    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "Field", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));
    let mut runtime = SubscriberRuntime::with_connections(
        address_space.clone(),
        vec![json_connection(json_reader(vec![target.clone()]))],
    )
    .unwrap();

    let datagram = json_datagram("42", "Field", 2.5);
    let outcome = runtime.process_datagram(&datagram, &ctx).unwrap();

    assert_eq!(outcome.applied_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(2.5))
    );
}

#[test]
fn json_network_message_with_wrong_publisher_is_filtered() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();

    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "Field", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));
    let mut runtime = SubscriberRuntime::with_connections(
        address_space.clone(),
        vec![json_connection(json_reader(vec![target.clone()]))],
    )
    .unwrap();

    let datagram = json_datagram("99", "Field", 2.5);
    let outcome = runtime.process_datagram(&datagram, &ctx).unwrap();

    assert_eq!(outcome.applied_readers, 0);
    assert_eq!(outcome.filtered_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
}

#[test]
fn malformed_json_datagram_is_rejected_without_panic() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let mut runtime = SubscriberRuntime::with_connections(
        address_space,
        vec![json_connection(json_reader(Vec::new()))],
    )
    .unwrap();

    assert!(runtime.process_datagram(b"{not valid json", &ctx).is_err());
}

#[test]
fn mixed_json_and_uadp_runtime_routes_binary_datagram_to_uadp_reader() {
    // OPC-10000-14 §7.2.5.4: a subscriber accepts the configured NetworkMessage encoding.
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();

    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let uadp_target = insert_target(&space, "UadpField", Variant::Double(0.0));
    let json_target = insert_target(&space, "JsonField", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));
    let mut runtime = SubscriberRuntime::with_connections(
        address_space.clone(),
        vec![mixed_connection(vec![
            json_reader(vec![json_target.clone()]),
            uadp_reader(uadp_target.clone()),
        ])],
    )
    .unwrap();

    let outcome = runtime
        .process_datagram(&uadp_datagram(12.5), &ctx)
        .unwrap();

    assert_eq!(outcome.applied_readers, 1);
    let space = address_space.read();
    assert_eq!(
        target_value(&space, &uadp_target),
        Some(Variant::Double(12.5))
    );
    assert_eq!(
        target_value(&space, &json_target),
        Some(Variant::Double(0.0))
    );
}
