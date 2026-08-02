//! MQTT subscriber transport integration tests.
//!
//! These tests exercise the broker DataSetReader receive path that T011
//! (`transport::mqtt::start_mqtt_subscriber`) and T012 (engine wiring) added.
//! mosquitto is not assumed to be available in every test environment. The
//! live-broker test starts it when the binary is present; the remaining tests
//! cover the channel→runtime handoff that `PubSubEngine::spawn_mqtt_subscribers`
//! performs (see `engine.rs`).
//!
//! Reference: OPC-10000-14 §6.4.2.6 (Broker DataSetReader transport).

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    engine::resolve_mqtt_topic_filter,
    transport::mqtt::{
        delivery_guarantee_to_mqtt_qos, start_mqtt_subscriber, start_mqtt_subscriber_with_cancel,
    },
    DataSetReaderConfig, FieldTargetConfig, PubSubConnectionConfig, PubSubEngine, PublisherId,
    ReaderGroupConfig, SubscriberRuntime, UadpDataSetMessage, UadpNetworkMessage,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, BrokerDataSetReaderTransportDataType,
    BrokerTransportQualityOfService, ContextOwned, DataEncoding, DataSetReaderDataType, DataTypeId,
    ExtensionObject, NodeId, NumericRange, TimestampsToReturn, UAString, Variant,
};
use rumqttc::{AsyncClient, MqttOptions, QoS};
use tokio_util::sync::CancellationToken;

struct MosquittoBroker {
    child: Child,
    config_path: PathBuf,
    port: u16,
}

impl Drop for MosquittoBroker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.config_path);
    }
}

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

async fn start_mosquitto_broker() -> Option<MosquittoBroker> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral MQTT port");
    let port = listener.local_addr().expect("read MQTT port").port();
    drop(listener);

    let config_path = std::env::temp_dir().join(format!(
        "async-opcua-mosquitto-{}-{port}.conf",
        std::process::id()
    ));
    fs::write(
        &config_path,
        format!("listener {port} 127.0.0.1\nallow_anonymous true\n"),
    )
    .expect("write mosquitto test config");

    let child = match Command::new("mosquitto")
        .arg("-c")
        .arg(&config_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let _ = fs::remove_file(&config_path);
            eprintln!("skipping live MQTT broker test: mosquitto not found on PATH");
            return None;
        }
        Err(error) => panic!("failed to start mosquitto: {error}"),
    };

    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return Some(MosquittoBroker {
                child,
                config_path,
                port,
            });
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let mut broker = MosquittoBroker {
        child,
        config_path,
        port,
    };
    let _ = broker.child.kill();
    panic!("mosquitto did not listen on port {port}");
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
fn mqtt_connection_at(address: String, reader: DataSetReaderConfig) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "mqtt-conn".to_string(),
        name: "mqtt-conn".to_string(),
        address,
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            dataset_readers: vec![reader],
            ..ReaderGroupConfig::default()
        }],
    }
}

fn mqtt_connection(reader: DataSetReaderConfig) -> PubSubConnectionConfig {
    mqtt_connection_at("mqtt://127.0.0.1:1883".to_string(), reader)
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

async fn target_reaches(
    space: Arc<RwLock<AddressSpace>>,
    target: &NodeId,
    expected: Variant,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if target_value(&space.read(), target) == Some(expected.clone()) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[tokio::test]
async fn mqtt_subscriber_receives_uadp_from_live_mosquitto() {
    let Some(broker) = start_mosquitto_broker().await else {
        return;
    };

    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "LiveMqttTarget", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));
    let mut engine = PubSubEngine::with_connections(
        address_space.clone(),
        vec![mqtt_connection_at(
            format!("mqtt://127.0.0.1:{}", broker.port),
            reader(vec![target.clone()]),
        )],
    );
    engine.start_subscribers().unwrap();

    let mut options = MqttOptions::new(
        format!("async-opcua-test-publisher-{}", std::process::id()),
        "127.0.0.1",
        broker.port,
    );
    options.set_keep_alive(Duration::from_secs(5));
    let (client, mut event_loop) = AsyncClient::new(options, 10);
    let event_loop_handle = tokio::spawn(async move {
        loop {
            if event_loop.poll().await.is_err() {
                break;
            }
        }
    });

    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let payload = network_msg(dataset_msg(42, 1, vec![Variant::Double(123.5)])).encode_to_vec(&ctx);

    let mut observed = false;
    for _ in 0..20 {
        client
            .publish(
                "opcua/telemetry/7",
                QoS::AtLeastOnce,
                false,
                payload.clone(),
            )
            .await
            .unwrap();

        if target_reaches(
            address_space.clone(),
            &target,
            Variant::Double(123.5),
            Duration::from_millis(100),
        )
        .await
        {
            observed = true;
            break;
        }
    }

    event_loop_handle.abort();
    let _ = event_loop_handle.await;
    engine.stop_subscribers().await;

    assert!(
        observed,
        "target {:?} never reached {:?} through live MQTT broker (got {:?})",
        target,
        Variant::Double(123.5),
        target_value(&address_space.read(), &target)
    );
}

#[tokio::test]
async fn start_mqtt_subscriber_stops_when_cancelled() {
    let (payload_tx, _payload_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1);
    let cancel = CancellationToken::new();
    let handle = start_mqtt_subscriber_with_cancel(
        "mqtt://127.0.0.1:1".to_string(),
        "opcua/telemetry/7".to_string(),
        QoS::AtLeastOnce,
        payload_tx,
        cancel.clone(),
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel();

    tokio::time::timeout(Duration::from_secs(1), handle)
        .await
        .expect("MQTT subscriber should stop after cancellation")
        .expect("MQTT subscriber task should not panic");
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
        QoS::AtLeastOnce,
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

// --- T011: RequestedDeliveryGuarantee → MQTT QoS (OPC-10000-14 §7.3.4.5) ------

/// `delivery_guarantee_to_mqtt_qos` maps the four concrete
/// `BrokerTransportQualityOfService` values per OPC-10000-14 §7.3.4.5
/// (AtMostOnce/BestEffort → QoS 0, AtLeastOnce → QoS 1, ExactlyOnce → QoS 2).
/// `None` (absent broker transport) has no mapping and defaults to AtLeastOnce
/// (QoS 1), the prior hard-coded QoS. An explicit `NotSpecified` is rejected
/// upstream by config validation (§6.4.2.6.4 — see
/// `broker_reader_with_not_specified_delivery_guarantee_is_rejected`); its arm
/// here is a defensive fallback, also AtLeastOnce.
#[test]
fn mqtt_qos_maps_each_delivery_guarantee_per_spec() {
    use opcua_types::BrokerTransportQualityOfService as Qos;

    // §7.3.4.5 concrete mappings.
    assert_eq!(
        delivery_guarantee_to_mqtt_qos(Some(Qos::BestEffort)),
        QoS::AtMostOnce
    );
    assert_eq!(
        delivery_guarantee_to_mqtt_qos(Some(Qos::AtMostOnce)),
        QoS::AtMostOnce
    );
    assert_eq!(
        delivery_guarantee_to_mqtt_qos(Some(Qos::AtLeastOnce)),
        QoS::AtLeastOnce
    );
    assert_eq!(
        delivery_guarantee_to_mqtt_qos(Some(Qos::ExactlyOnce)),
        QoS::ExactlyOnce
    );
    // Absent guarantee (no broker transport): operational default.
    assert_eq!(delivery_guarantee_to_mqtt_qos(None), QoS::AtLeastOnce);
    // Explicit NotSpecified: defensive fallback only (validation rejects it).
    assert_eq!(
        delivery_guarantee_to_mqtt_qos(Some(Qos::NotSpecified)),
        QoS::AtLeastOnce
    );
}

// --- T012: QueueName → MQTT topic filter (OPC-10000-14 §6.4.2.6.1) -----------

/// `resolve_mqtt_topic_filter` honours the reader's broker `QueueName`
/// (§6.4.2.6.1) and only falls back to the group-id topic convention when no
/// QueueName is configured, so pre-existing configs keep working.
#[test]
fn mqtt_topic_filter_prefers_reader_queue_name() {
    // Explicit QueueName wins outright.
    let mut with_queue = reader(Vec::new());
    with_queue.queue_name = Some("factory/line-a/telemetry".to_string());
    assert_eq!(
        resolve_mqtt_topic_filter(&with_queue, 7),
        "factory/line-a/telemetry"
    );

    // No QueueName: fall back to the writer_group_id convention.
    let mut with_writer_group = reader(Vec::new());
    with_writer_group.writer_group_id = Some(9);
    assert_eq!(
        resolve_mqtt_topic_filter(&with_writer_group, 7),
        "opcua/telemetry/9"
    );

    // No QueueName and no writer_group_id: fall back to the reader_group_id.
    let mut with_neither = reader(Vec::new());
    with_neither.writer_group_id = None;
    assert_eq!(
        resolve_mqtt_topic_filter(&with_neither, 7),
        "opcua/telemetry/7"
    );
}

// --- T011 + T012 config model: from_data_type preserves broker transport ----

/// `DataSetReaderConfig::from_data_type` carries the broker
/// `QueueName` and `RequestedDeliveryGuarantee` out of
/// `BrokerDataSetReaderTransportDataType` transport settings (§6.4.2.6).
#[test]
fn dataset_reader_from_data_type_preserves_broker_transport_settings() {
    let transport = BrokerDataSetReaderTransportDataType {
        queue_name: UAString::from("sensors/temperature"),
        requested_delivery_guarantee: BrokerTransportQualityOfService::ExactlyOnce,
        ..BrokerDataSetReaderTransportDataType::default()
    };
    let source = DataSetReaderDataType {
        name: UAString::from("broker-reader"),
        transport_settings: ExtensionObject::from_message(transport),
        ..DataSetReaderDataType::default()
    };

    let config = DataSetReaderConfig::from_data_type(&source, 1);

    assert_eq!(config.queue_name.as_deref(), Some("sensors/temperature"));
    assert_eq!(
        config.requested_delivery_guarantee,
        Some(BrokerTransportQualityOfService::ExactlyOnce)
    );
}

/// An empty `QueueName` normalizes to `None` (so the group-id topic convention
/// applies), while an explicit `NotSpecified` guarantee is PRESERVED as
/// `Some(NotSpecified)` — not folded to `None` — so config validation can reject
/// it per §6.4.2.6.4 (see `broker_reader_with_not_specified_delivery_guarantee_is_rejected`).
#[test]
fn dataset_reader_from_data_type_preserves_default_broker_transport_settings() {
    let source = DataSetReaderDataType {
        name: UAString::from("broker-reader"),
        transport_settings: ExtensionObject::from_message(
            BrokerDataSetReaderTransportDataType::default(),
        ),
        ..DataSetReaderDataType::default()
    };

    let config = DataSetReaderConfig::from_data_type(&source, 1);

    // Null/empty QueueName → None (group-id topic fallback applies).
    assert_eq!(config.queue_name, None);
    // Explicit NotSpecified is preserved, not silently defaulted.
    assert_eq!(
        config.requested_delivery_guarantee,
        Some(BrokerTransportQualityOfService::NotSpecified)
    );
}

/// OPC-10000-14 §6.4.2.6.4: "NotSpecified is not allowed on the DataSetReader."
/// A broker reader with an explicit `NotSpecified` delivery guarantee is
/// rejected at subscriber-config validation rather than silently defaulting the
/// QoS; an absent guarantee (`None`) is accepted.
#[test]
fn broker_reader_with_not_specified_delivery_guarantee_is_rejected() {
    let mut rejected = reader(Vec::new());
    rejected.requested_delivery_guarantee = Some(BrokerTransportQualityOfService::NotSpecified);
    assert_eq!(
        mqtt_connection(rejected).validate_subscriber_config(),
        Err(opcua_types::StatusCode::BadConfigurationError)
    );

    // An absent guarantee (the common case) is accepted and defaults the QoS.
    let accepted = mqtt_connection(reader(Vec::new()));
    assert!(accepted.validate_subscriber_config().is_ok());
}

/// End-to-end proof of T012: a DataSetReader whose broker `QueueName` is set
/// subscribes to that exact topic, so a publisher writing the QueueName topic
/// reaches the target variable through the live broker.
#[tokio::test]
async fn mqtt_subscriber_receives_uadp_on_configured_queue_name_from_live_mosquitto() {
    let Some(broker) = start_mosquitto_broker().await else {
        return;
    };

    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "LiveMqttQueueNameTarget", Variant::Double(0.0));
    let address_space = Arc::new(RwLock::new(space));

    let mut reader = reader(vec![target.clone()]);
    // T012: the configured QueueName becomes the MQTT subscription topic.
    reader.queue_name = Some("opcua/custom/queue-name".to_string());

    let mut engine = PubSubEngine::with_connections(
        address_space.clone(),
        vec![mqtt_connection_at(
            format!("mqtt://127.0.0.1:{}", broker.port),
            reader,
        )],
    );
    engine.start_subscribers().unwrap();

    let mut options = MqttOptions::new(
        format!("async-opcua-test-publisher-{}", std::process::id()),
        "127.0.0.1",
        broker.port,
    );
    options.set_keep_alive(Duration::from_secs(5));
    let (client, mut event_loop) = AsyncClient::new(options, 10);
    let event_loop_handle = tokio::spawn(async move {
        loop {
            if event_loop.poll().await.is_err() {
                break;
            }
        }
    });

    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let payload = network_msg(dataset_msg(42, 1, vec![Variant::Double(222.0)])).encode_to_vec(&ctx);

    let mut observed = false;
    for _ in 0..20 {
        // Publish to the *QueueName* topic, not the group-id-derived one.
        client
            .publish(
                "opcua/custom/queue-name",
                QoS::AtLeastOnce,
                false,
                payload.clone(),
            )
            .await
            .unwrap();

        if target_reaches(
            address_space.clone(),
            &target,
            Variant::Double(222.0),
            Duration::from_millis(100),
        )
        .await
        {
            observed = true;
            break;
        }
    }

    event_loop_handle.abort();
    let _ = event_loop_handle.await;
    engine.stop_subscribers().await;

    assert!(
        observed,
        "target {:?} never reached {:?} through the configured QueueName topic (got {:?})",
        target,
        Variant::Double(222.0),
        target_value(&address_space.read(), &target)
    );
}
