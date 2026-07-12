//! MQTT subscriber transport integration tests.
//!
//! These tests exercise the broker DataSetReader receive path that T011
//! (`transport::mqtt::start_mqtt_subscriber`) and T012 (engine wiring) added.
//! mosquitto is not assumed to be available in the test environment, so rather
//! than requiring a live broker we replicate the channel→runtime handoff that
//! `PubSubEngine::spawn_mqtt_subscribers` performs (see `engine.rs`): an mpsc
//! channel carries opaque payload bytes from the subscriber task to a
//! forwarder that feeds `SubscriberRuntime::process_datagram`.
//!
//! Reference: OPC-10000-14 §6.4.2.6 (Broker DataSetReader transport).

use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    transport::mqtt::start_mqtt_subscriber, DataSetReaderConfig, FieldTargetConfig,
    PubSubConnectionConfig, PublisherId, ReaderGroupConfig, SubscriberRuntime, UadpDataSetMessage,
    UadpNetworkMessage,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, ContextOwned, DataEncoding, DataTypeId, NodeId, NumericRange,
    TimestampsToReturn, Variant,
};
use tokio_util::sync::CancellationToken;

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
        name: Some("reader-mqtt-a".to_string()),
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

/// Mirrors the UDP test's `connection` helper but uses an `mqtt://` address so
/// the config classifies as a supported subscriber transport
/// (`validate_subscriber_config` accepts `mqtt://` per T012).
fn mqtt_connection(reader: DataSetReaderConfig) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "mqtt-conn".to_string(),
        name: "mqtt-conn".to_string(),
        address: "mqtt://127.0.0.1:1883".to_string(),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: vec![reader],
            ..ReaderGroupConfig::default()
        }],
    }
}

/// Replicates the forwarder task from `PubSubEngine::spawn_mqtt_subscribers`
/// (`engine.rs`): drain `payload_rx` and call `process_datagram` on the
/// runtime for each received payload.
fn spawn_forwarder(
    runtime: Arc<RwLock<SubscriberRuntime>>,
    mut payload_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                payload = payload_rx.recv() => {
                    let Some(payload) = payload else { break };
                    let ctx_owned = ContextOwned::default();
                    let ctx = ctx_owned.context();
                    let _ = runtime.write().process_datagram(&payload, &ctx);
                }
            }
        }
    })
}

/// Poll the address space briefly so the test can observe the forwarder task
/// applying the MQTT-delivered payload without a hard `sleep`.
async fn await_target(space: Arc<RwLock<AddressSpace>>, target: &NodeId, expected: Variant) {
    for _ in 0..100 {
        if target_value(&space.read(), target) == Some(expected.clone()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!(
        "target {:?} never reached {:?} (got {:?})",
        target,
        expected,
        target_value(&space.read(), target)
    );
}

#[tokio::test]
async fn mqtt_receive_channel_processes_uadp_payload_through_runtime() {
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "MqttTarget", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(
            address_space.clone(),
            vec![mqtt_connection(reader(vec![target.clone()]))],
        )
        .unwrap(),
    ));

    // Channel mirrors the one in `spawn_mqtt_subscribers` (engine.rs).
    let (payload_tx, payload_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(128);
    let cancel = CancellationToken::new();
    let forwarder = spawn_forwarder(runtime.clone(), payload_rx, cancel.clone());

    // Encode a UADP NetworkMessage exactly as `MqttPublisher` would publish it
    // and as `start_mqtt_subscriber` would forward the raw bytes.
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let payload = network_msg(dataset_msg(42, 1, vec![Variant::Double(77.0)])).encode_to_vec(&ctx);

    payload_tx.send(payload).await.unwrap();

    await_target(address_space.clone(), &target, Variant::Double(77.0)).await;

    cancel.cancel();
    let _ = forwarder.await;
}

#[tokio::test]
async fn mqtt_receive_channel_applies_multiple_payloads_in_order() {
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let a = insert_target(&space, "A", Variant::Double(0.0));
    let b = insert_target(&space, "B", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(
            address_space.clone(),
            vec![mqtt_connection(reader(vec![a.clone(), b.clone()]))],
        )
        .unwrap(),
    ));

    let (payload_tx, payload_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(128);
    let cancel = CancellationToken::new();
    let forwarder = spawn_forwarder(runtime.clone(), payload_rx, cancel.clone());

    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    for (seq, va, vb) in [(1u16, 11.0, 22.0), (2u16, 33.0, 44.0), (3u16, 55.0, 66.0)] {
        let payload = network_msg(dataset_msg(
            42,
            seq,
            vec![Variant::Double(va), Variant::Double(vb)],
        ))
        .encode_to_vec(&ctx);
        payload_tx.send(payload).await.unwrap();
    }

    await_target(address_space.clone(), &a, Variant::Double(55.0)).await;
    await_target(address_space.clone(), &b, Variant::Double(66.0)).await;

    cancel.cancel();
    let _ = forwarder.await;
}

/// Verifies `start_mqtt_subscriber` (T011) compiles, returns an abortable
/// `JoinHandle`, and never panics even when no broker is reachable. The task
/// is cancelled immediately so the connection attempt is best-effort.
#[tokio::test]
async fn start_mqtt_subscriber_returns_abortable_handle_without_broker() {
    let (payload_tx, _payload_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1);
    let handle = start_mqtt_subscriber(
        "mqtt://127.0.0.1:1".to_string(),
        "opcua/telemetry/7".to_string(),
        payload_tx,
    );

    // Task must exist and remain live (reconnecting with backoff) rather than
    // completing or panicking immediately.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !handle.is_finished(),
        "subscriber should be retrying, not exited"
    );

    handle.abort();
    // Await to propagate the abort so the runtime drops the task cleanly.
    let _ = handle.await;
}
