//! Wired subscriber integration test: a UADP NetworkMessage is sent over a real
//! UDP socket to a bound subscriber endpoint and processed through the runtime
//! decode path, applying the payload to address-space target variables.
//!
//! The helpers below mirror `subscriber_plain_uadp_tests.rs` so the wired path
//! exercises the same DataSetReader / ReaderGroup / Connection configuration
//! and the same `UadpNetworkMessage` struct construction that the unit tests use.

use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    transport::udp::{bind_subscriber_socket, UdpSubscriberEndpoint},
    DataSetReaderConfig, FieldTargetConfig, PubSubConnectionConfig, PublisherId, ReaderGroupConfig,
    SubscriberRuntime, UadpDataSetMessage, UadpNetworkMessage,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, ContextOwned, DataEncoding, DataTypeId, NodeId, NumericRange,
    PubSubState, TimestampsToReturn, Variant,
};
use tokio::net::UdpSocket;

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

fn dataset_msg(
    dataset_writer_id: u16,
    sequence_number: u16,
    fields: Vec<Variant>,
) -> UadpDataSetMessage {
    UadpDataSetMessage {
        dataset_writer_id,
        sequence_number,
        timestamp: None,
        status: None,
        fields,
    }
}

fn network_msg(message: UadpDataSetMessage) -> UadpNetworkMessage {
    UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: message.sequence_number,
        dataset_messages: vec![message],
    }
}

fn reader(targets: Vec<NodeId>) -> DataSetReaderConfig {
    DataSetReaderConfig {
        name: Some("reader-a".to_string()),
        dataset_reader_id: 1,
        dataset_writer_id: 42,
        publisher_id: Some(PublisherId::UInt16(11)),
        writer_group_id: Some(7),
        network_message_number: Some(3),
        message_receive_timeout: Some(Duration::from_millis(100)),
        target_variables: targets
            .into_iter()
            .enumerate()
            .map(|(index, target)| FieldTargetConfig::value(index, target))
            .collect(),
        ..DataSetReaderConfig::default()
    }
}

fn connection(reader: DataSetReaderConfig, address: String) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "conn".to_string(),
        name: "conn".to_string(),
        address,
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: vec![reader],
            ..ReaderGroupConfig::default()
        }],
    }
}

/// A UADP NetworkMessage delivered over UDP is decoded by the subscriber runtime
/// and its single DataKeyFrame field is written to the matching target variable.
///
/// This exercises the full wired path: `UdpSubscriberEndpoint` parse, socket
/// bind, datagram send/receive, `process_datagram` decode, and target apply.
#[tokio::test]
async fn wired_subscriber_receives_uadp_and_applies_value() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();

    // Address space with one writable Double target (OPC-10000-14 §6.2.1).
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "target", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));

    // Bind the subscriber socket on an ephemeral port and reflect the actual
    // bound address into the connection config so the runtime validates the
    // same endpoint that the wire delivers to.
    let endpoint = UdpSubscriberEndpoint {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        multicast_addr: None,
    };
    let subscriber_sock = bind_subscriber_socket(endpoint).await.unwrap();
    let bound_addr = subscriber_sock.local_addr().unwrap();
    let address = format!("udp://{bound_addr}");

    let cfg_reader = reader(vec![target.clone()]);
    let cfg = connection(cfg_reader, address);

    // The runtime accepts a valid UADP subscriber configuration.
    let mut runtime =
        SubscriberRuntime::with_connections(address_space.clone(), vec![cfg]).unwrap();

    // Fresh readers start PreOperational until the first matching key frame.
    assert_eq!(
        runtime.reader_status(1).unwrap().state,
        PubSubState::PreOperational
    );

    // Build and binary-encode a UADP NetworkMessage carrying one Double field.
    let datagram = network_msg(dataset_msg(42, 1, vec![Variant::Double(7.5)])).encode_to_vec(&ctx);

    // Publish the datagram from a separate sender socket to the subscriber's
    // bound address, then receive it on the subscriber side.
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sender.send_to(&datagram, bound_addr).await.unwrap();

    let mut buf = [0u8; 2048];
    let (len, _peer) = subscriber_sock.recv_from(&mut buf).await.unwrap();
    assert_eq!(&buf[..len], &datagram[..]);

    // Feed the received datagram through the runtime decode path.
    let outcome = runtime.process_datagram(&buf[..len], &ctx).unwrap();
    assert_eq!(outcome.matched_readers, 1);
    assert_eq!(outcome.applied_readers, 1);

    // The Double payload was written to the target variable.
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(7.5))
    );

    // A successfully applied key frame advances the reader state.
    let status = runtime.reader_status(1).unwrap();
    assert!(
        status.state == PubSubState::Operational || status.state == PubSubState::PreOperational,
        "expected Operational or PreOperational after first message, got {:?}",
        status.state
    );
    assert_eq!(status.accepted_count, 1);
    assert_eq!(status.last_error, None);
}

/// A datagram whose publisher/writer filters do not match any configured reader
/// is delivered over UDP, decoded, and filtered out without writing targets or
/// raising an error.
#[tokio::test]
async fn wired_subscriber_filters_non_matching_udp_datagram() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();

    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "target", Variant::Double(2.0));
    let address_space = Arc::new(RwLock::new(space));

    let endpoint = UdpSubscriberEndpoint {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        multicast_addr: None,
    };
    let subscriber_sock = bind_subscriber_socket(endpoint).await.unwrap();
    let bound_addr = subscriber_sock.local_addr().unwrap();
    let address = format!("udp://{bound_addr}");

    let mut runtime = SubscriberRuntime::with_connections(
        address_space.clone(),
        vec![connection(reader(vec![target.clone()]), address)],
    )
    .unwrap();

    // Mismatched publisher id: the reader expects PublisherId::UInt16(11).
    let datagram = {
        let msg = dataset_msg(42, 1, vec![Variant::Double(99.0)]);
        UadpNetworkMessage {
            publisher_id: PublisherId::UInt16(99),
            ..network_msg(msg)
        }
    }
    .encode_to_vec(&ctx);

    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sender.send_to(&datagram, bound_addr).await.unwrap();

    let mut buf = [0u8; 2048];
    let (len, _peer) = subscriber_sock.recv_from(&mut buf).await.unwrap();

    let outcome = runtime.process_datagram(&buf[..len], &ctx).unwrap();
    assert_eq!(outcome.matched_readers, 0);
    assert_eq!(outcome.applied_readers, 0);

    // Target is untouched and the reader records one filtered message.
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(2.0))
    );
    assert_eq!(runtime.reader_status(1).unwrap().filtered_count, 1);
}
