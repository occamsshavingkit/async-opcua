use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, ContextOwned, DataEncoding, DataTypeId, NodeId, NumericRange,
    TimestampsToReturn, Variant,
};
use tokio::{net::UdpSocket, time::timeout};
use tokio_util::sync::CancellationToken;

use super::{PubSubEngine, SubscriberDatagramProcessor};
use crate::{
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    DataSetReaderConfig, DataSetReaderKey, FieldTargetConfig, PubSubConnectionConfig,
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

fn connection(
    connection_id: &str,
    address: String,
    dataset_reader_id: u16,
    target: NodeId,
) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
        address,
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: dataset_reader_id,
            dataset_readers: vec![DataSetReaderConfig {
                dataset_reader_id,
                dataset_writer_id: 42,
                publisher_id: Some(PublisherId::UInt16(11)),
                writer_group_id: Some(7),
                network_message_number: Some(3),
                target_variables: vec![FieldTargetConfig::value(0, target)],
                ..DataSetReaderConfig::default()
            }],
            ..ReaderGroupConfig::default()
        }],
    }
}

#[tokio::test]
async fn udp_ingress_applies_only_to_readers_on_owning_connection() {
    // Given two UDP connections whose readers share filters but target different variables.
    let first_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let second_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let first_address = first_socket.local_addr().unwrap();
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = insert_target(&space, "FirstConnectionTarget");
    let second_target = insert_target(&space, "SecondConnectionTarget");
    let address_space = Arc::new(RwLock::new(space));
    let first_connection = connection(
        "first-connection",
        format!("udp://{first_address}"),
        1,
        first_target.clone(),
    );
    let second_connection = connection(
        "second-connection",
        format!("udp://{}", second_socket.local_addr().unwrap()),
        1,
        second_target.clone(),
    );
    let connections = vec![first_connection.clone(), second_connection];
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(address_space.clone(), connections.clone()).unwrap(),
    ));
    let engine = PubSubEngine::with_connections(address_space.clone(), connections);
    let first_readers = first_connection.reader_groups[0].dataset_readers.clone();
    let processor = SubscriberDatagramProcessor::new(
        runtime.clone(),
        &first_connection.connection_id,
        first_readers,
        None,
    );
    let cancel = CancellationToken::new();
    let handles =
        engine.spawn_udp_subscriber(first_connection, first_socket, processor, cancel.clone());
    let payload = UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(41.5)],
        }],
    }
    .encode_to_vec(&ContextOwned::default().context());

    // When a matching datagram arrives only on the first connection's socket.
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sender.send_to(&payload, first_address).await.unwrap();
    timeout(Duration::from_secs(1), async {
        loop {
            if runtime
                .read()
                .reader_status_by_key(&DataSetReaderKey::new("first-connection", 1))
                .unwrap()
                .accepted_count
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    for handle in handles {
        handle.await.unwrap();
    }

    // Then only the reader owned by that connection applies the value.
    assert_eq!(
        target_value(&address_space.read(), &first_target),
        Some(Variant::Double(41.5))
    );
    assert_eq!(
        target_value(&address_space.read(), &second_target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .read()
            .reader_status_by_key(&DataSetReaderKey::new("first-connection", 1))
            .unwrap()
            .accepted_count,
        1
    );
    assert_eq!(
        runtime
            .read()
            .reader_status_by_key(&DataSetReaderKey::new("second-connection", 1))
            .unwrap()
            .accepted_count,
        0
    );
}
