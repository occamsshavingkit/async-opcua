//! PubSub engine coordinator tests.

use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_pubsub::{
    engine::PubSubEngine, DataSetReaderConfig, MqttPublisher, PubSubConnectionConfig,
    PubSubPublisher, PublisherId, ReaderGroupConfig, SecurityGroup, TransportKind,
    UadpDataSetMessage, UadpNetworkMessage, UadpSecurityCodec, WriterGroupConfig,
};
use opcua_server::address_space::AddressSpace;
use opcua_types::{
    BinaryEncodable, ContextOwned, DateTime, MessageSecurityMode, StatusCode, Variant,
};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

fn empty_connection(connection_id: &str, address: &str) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        reader_groups: Vec::new(),
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
        address: address.to_string(),
        writer_groups: Vec::<WriterGroupConfig>::new(),
    }
}

fn address_space() -> Arc<RwLock<AddressSpace>> {
    Arc::new(RwLock::new(AddressSpace::new()))
}

#[test]
fn classifies_all_supported_pubsub_transport_addresses() {
    assert_eq!(
        TransportKind::from_address("mqtt://broker.local:1883").unwrap(),
        TransportKind::Mqtt
    );
    assert_eq!(
        TransportKind::from_address("udp://239.0.0.1:4840").unwrap(),
        TransportKind::Udp
    );
    assert_eq!(
        TransportKind::from_address("amqp://broker.local:5672/opcua.telemetry").unwrap(),
        TransportKind::Amqp
    );
    assert_eq!(
        TransportKind::from_address("ws://broker.local:9001/pubsub").unwrap(),
        TransportKind::WebSocket
    );
}

#[test]
fn classifies_standard_and_mixed_case_opc_udp_transport_addresses() {
    // OPC-10000-14 §§7.3.2.2-7.3.2.3 define `opc.udp` as the UDP PubSub URI scheme.
    assert_eq!(
        TransportKind::from_address("opc.udp://239.0.0.1:4840").unwrap(),
        TransportKind::Udp
    );
    assert_eq!(
        TransportKind::from_address("  OpC.UdP://239.0.0.1:4840  ").unwrap(),
        TransportKind::Udp
    );
}

#[test]
fn rejects_mqtts_transport_addresses() {
    // Given: a secure MQTT address unsupported by the publisher transport.
    // When: the address is classified.
    // Then: classification rejects it explicitly.
    assert_eq!(
        TransportKind::from_address("mqtts://broker.local:8883").unwrap_err(),
        StatusCode::BadNotSupported
    );
}

#[test]
fn classifies_mixed_case_mqtt_transport_addresses() {
    assert_eq!(
        TransportKind::from_address("MQTT://broker.local:1883").unwrap(),
        TransportKind::Mqtt
    );
    assert_eq!(
        TransportKind::from_address("MQTTS://broker.local:8883").unwrap_err(),
        StatusCode::BadNotSupported
    );
}

#[test]
fn classifies_mixed_case_non_mqtt_transport_addresses() {
    assert_eq!(
        TransportKind::from_address("  UdP://239.0.0.1:4840  ").unwrap(),
        TransportKind::Udp
    );
    assert_eq!(
        TransportKind::from_address("aMqP://broker.local:5672/topic").unwrap(),
        TransportKind::Amqp
    );
    assert_eq!(
        TransportKind::from_address("AMQPS://broker.local:5671/topic").unwrap(),
        TransportKind::Amqp
    );
    assert_eq!(
        TransportKind::from_address("Ws://broker.local:9001/pubsub").unwrap(),
        TransportKind::WebSocket
    );
    assert_eq!(
        TransportKind::from_address("WSS://broker.local:9443/pubsub").unwrap(),
        TransportKind::WebSocket
    );

    #[cfg(feature = "tsn")]
    assert_eq!(
        TransportKind::from_address("TsN://eth0").unwrap(),
        TransportKind::Tsn
    );
}

#[cfg(not(feature = "tsn"))]
#[test]
fn rejects_tsn_transport_addresses_when_feature_is_disabled() {
    // Given: a syntactically recognized TSN address without TSN support enabled.
    // When: the address is classified.
    // Then: classification reports that the transport is not supported.
    assert_eq!(
        TransportKind::from_address("tsn://eth0").unwrap_err(),
        StatusCode::BadNotSupported
    );
}

#[tokio::test]
async fn manages_connection_configs_and_udp_publisher_lifecycle() {
    let mut engine = PubSubEngine::new(address_space());
    let config = empty_connection("udp-1", "udp://127.0.0.1:4840");

    engine.add_connection(config.clone());

    assert_eq!(engine.connection_configs(), &[config]);
    assert_eq!(engine.active_handle_count(), 0);

    engine.start().unwrap();

    assert!(engine.is_running());
    assert_eq!(engine.active_handle_count(), 1);

    engine.stop().await;

    assert!(!engine.is_running());
    assert_eq!(engine.active_handle_count(), 0);
}

#[test]
fn replaces_connection_configs_from_config_snapshot() {
    let mut engine = PubSubEngine::new(address_space());
    let initial = empty_connection("udp-1", "udp://127.0.0.1:4840");
    let replacement = empty_connection("mqtt-1", "mqtt://broker.local:1883");
    engine.add_connection(initial);

    engine.replace_connections(vec![replacement.clone()]);

    assert_eq!(engine.connection_configs(), &[replacement]);
}

#[tokio::test]
async fn replacing_connections_while_subscribers_run_invalidates_runtime() {
    let mut engine = PubSubEngine::new(address_space());
    let mut initial = empty_connection("udp-1", "udp://127.0.0.1:0");
    initial.reader_groups.push(ReaderGroupConfig {
        reader_group_id: 1,
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 9,
            dataset_writer_id: 7,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });
    engine.add_connection(initial);

    engine.start_subscribers().await.unwrap();
    assert!(engine.subscriber_status(9).is_some());

    engine.replace_connections(Vec::new());
    engine.stop_subscribers().await;
    engine.start_subscribers().await.unwrap();

    assert!(engine.subscriber_status(9).is_none());
}

#[tokio::test]
async fn adding_connection_while_subscribers_run_rebuilds_runtime_on_restart() {
    // Given: a running subscriber with reader 9 and a second valid local UDP reader configuration.
    let mut initial = empty_connection("udp-1", "udp://127.0.0.1:0");
    initial.reader_groups.push(ReaderGroupConfig {
        reader_group_id: 1,
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 9,
            dataset_writer_id: 7,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });
    let mut engine = PubSubEngine::new(address_space());
    engine.add_connection(initial);
    engine.start_subscribers().await.unwrap();
    assert!(engine.subscriber_status(9).is_some());

    let mut added = empty_connection("udp-2", "udp://127.0.0.1:0");
    added.reader_groups.push(ReaderGroupConfig {
        reader_group_id: 2,
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 10,
            dataset_writer_id: 8,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });

    // When: the new connection is added while running, then subscribers are stopped and restarted.
    engine.add_connection(added);
    engine.stop_subscribers().await;
    engine.start_subscribers().await.unwrap();

    // Then: the restarted runtime exposes the newly added reader status.
    assert!(engine.subscriber_status(10).is_some());
    engine.stop_subscribers().await;
}

#[tokio::test]
async fn unsupported_amqp_reader_does_not_start_earlier_mqtt_subscriber() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mqtt_address = format!("mqtt://{}", listener.local_addr().unwrap());
    let mut mqtt = empty_connection("1-mqtt", &mqtt_address);
    mqtt.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            publisher_id: Some(PublisherId::String("publisher-1".to_string())),
            writer_group_id: Some(1),
            dataset_reader_id: 9,
            dataset_writer_id: 7,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });

    let mut amqp = empty_connection("2-amqp", "amqp://127.0.0.1:5672/topic");
    amqp.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            publisher_id: Some(PublisherId::String("publisher-2".to_string())),
            writer_group_id: Some(2),
            dataset_reader_id: 10,
            dataset_writer_id: 8,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });

    let mut engine = PubSubEngine::new(address_space());
    engine.replace_connections(vec![mqtt, amqp]);

    assert_eq!(
        engine.start_subscribers().await.unwrap_err(),
        StatusCode::BadNotSupported
    );
    tokio::task::yield_now().await;
    assert_eq!(engine.active_subscriber_handle_count(), 0);
    assert!(!engine.subscribers_are_running());
    assert!(
        tokio::time::timeout(Duration::from_millis(500), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn malformed_later_udp_reader_does_not_start_earlier_mqtt_subscriber() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mqtt_address = format!("mqtt://{}", listener.local_addr().unwrap());
    let mut mqtt = empty_connection("1-mqtt", &mqtt_address);
    mqtt.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            publisher_id: Some(PublisherId::String("publisher-1".to_string())),
            writer_group_id: Some(1),
            dataset_reader_id: 9,
            dataset_writer_id: 7,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });

    let mut malformed_udp = empty_connection("2-udp", "udp://127.0.0.1:not-a-port");
    malformed_udp.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 10,
            dataset_writer_id: 8,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });

    let mut engine = PubSubEngine::new(address_space());
    engine.replace_connections(vec![mqtt, malformed_udp]);

    assert_eq!(
        engine.start_subscribers().await.unwrap_err(),
        StatusCode::BadConfigurationError
    );
    tokio::task::yield_now().await;
    assert_eq!(engine.active_subscriber_handle_count(), 0);
    assert!(!engine.subscribers_are_running());
    assert!(
        tokio::time::timeout(Duration::from_millis(500), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn udp_bind_failure_is_reported_without_committing_subscriber_state() {
    // Given: a configured UDP subscriber whose port is already occupied.
    let occupied = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = format!("udp://{}", occupied.local_addr().unwrap());
    let mut connection = empty_connection("udp-conflict", &address);
    connection.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 11,
            dataset_writer_id: 7,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });
    let mut engine = PubSubEngine::with_connections(address_space(), vec![connection]);

    // When: subscriber startup attempts to bind the occupied UDP port.
    let result = tokio::time::timeout(Duration::from_millis(500), engine.start_subscribers())
        .await
        .unwrap();

    // Then: startup fails atomically before running state or reader status is committed.
    assert_eq!(result.unwrap_err(), StatusCode::BadCommunicationError);
    assert!(!engine.subscribers_are_running());
    assert_eq!(engine.active_subscriber_handle_count(), 0);
    assert!(engine.subscriber_status(11).is_none());
}

#[tokio::test]
async fn mixed_subscriber_preparation_is_atomic_across_udp_and_mqtt_resources() {
    // Given: an earlier valid UDP connection, an MQTT connection, and a later UDP conflict.
    let earlier_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let earlier_address = format!("udp://{}", earlier_socket.local_addr().unwrap());
    drop(earlier_socket);

    let conflicting_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let conflicting_address = format!("udp://{}", conflicting_socket.local_addr().unwrap());
    let mqtt_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mqtt_address = format!("mqtt://{}", mqtt_listener.local_addr().unwrap());

    let mut earlier_udp = empty_connection("1-udp", &earlier_address);
    earlier_udp.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 21,
            dataset_writer_id: 7,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });
    let mut mqtt = empty_connection("2-mqtt", &mqtt_address);
    mqtt.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 22,
            dataset_writer_id: 8,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });
    let mut conflicting_udp = empty_connection("3-udp", &conflicting_address);
    conflicting_udp.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 23,
            dataset_writer_id: 9,
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    });
    let mut engine =
        PubSubEngine::with_connections(address_space(), vec![earlier_udp, mqtt, conflicting_udp]);

    // When: startup prepares all subscriber resources and encounters the occupied later UDP port.
    let result = tokio::time::timeout(Duration::from_millis(500), engine.start_subscribers())
        .await
        .unwrap();

    // Then: no resource or subscriber state is committed, including the MQTT TCP connection.
    assert_eq!(result.unwrap_err(), StatusCode::BadCommunicationError);
    assert_eq!(engine.active_subscriber_handle_count(), 0);
    assert!(!engine.subscribers_are_running());
    assert!(engine.subscriber_status(21).is_none());
    assert!(engine.subscriber_status(22).is_none());
    assert!(engine.subscriber_status(23).is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(500), mqtt_listener.accept())
            .await
            .is_err()
    );

    // Then: the earlier UDP port is immediately available after failed preparation.
    let rebound = tokio::time::timeout(
        Duration::from_millis(500),
        UdpSocket::bind(earlier_address.strip_prefix("udp://").unwrap()),
    )
    .await
    .unwrap();
    assert!(rebound.is_ok());
}

#[tokio::test]
async fn mqtt_publisher_rejects_mqtts_before_spawning() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mqtts_address = format!("mqtts://{}", listener.local_addr().unwrap());
    let publisher = MqttPublisher::new(address_space());
    let result = publisher.start_publishing(
        empty_connection("mqtts-1", &mqtts_address),
        CancellationToken::new(),
    );

    assert_eq!(result.err(), Some(StatusCode::BadNotSupported));
    assert!(
        tokio::time::timeout(Duration::from_millis(500), listener.accept())
            .await
            .is_err()
    );
}

#[test]
fn rejects_unknown_transport_addresses() {
    assert!(TransportKind::from_address("ftp://broker.local/pubsub").is_err());
}

#[test]
fn datagram_processing_does_not_revalidate_transport_address() {
    // Given: a reader configuration whose transport address is irrelevant after startup preflight.
    let mut connection = empty_connection("subscriber", "ftp://broker.local/pubsub");
    connection.reader_groups.push(ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig::default()],
        ..ReaderGroupConfig::default()
    });
    let mut engine = PubSubEngine::with_connections(address_space(), vec![connection]);
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();

    // When: a malformed datagram reaches per-datagram processing.
    let result = engine.process_subscriber_datagram("subscriber", &[0xff, 0x00], &ctx);

    // Then: payload decoding, not transport reclassification, determines the error.
    assert_eq!(result, Err(StatusCode::BadDecodingError));
}

#[test]
fn start_rejects_unknown_transport_without_marking_engine_running() {
    let mut engine = PubSubEngine::new(address_space());
    engine.add_connection(empty_connection("bad-1", "ftp://broker.local/pubsub"));

    assert_eq!(engine.start().unwrap_err(), StatusCode::BadInvalidArgument);
    assert!(!engine.is_running());
    assert_eq!(engine.active_handle_count(), 0);
}

#[tokio::test]
async fn start_rejects_mqtts_without_starting_a_publisher() {
    // Given: an engine configured with a secure MQTT connection.
    let mut engine = PubSubEngine::new(address_space());
    engine.add_connection(empty_connection("mqtts-1", "mqtts://broker.local:8883"));

    // When: startup validates the configured transport.
    let status = engine.start().unwrap_err();

    // Then: secure MQTT is rejected before any publisher task starts.
    assert_eq!(status, StatusCode::BadNotSupported);
    assert!(!engine.is_running());
    assert_eq!(engine.active_handle_count(), 0);
}

#[test]
fn encodes_publisher_uadp_with_registered_security_group() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut engine = PubSubEngine::new(address_space());
    let security_group = SecurityGroup::new("group-1", Duration::from_secs(3600)).unwrap();
    let key_set = security_group.current_key_set().clone();
    engine.register_security_group(security_group);

    let message = UadpNetworkMessage {
        publisher_id: PublisherId::String("publisher-1".to_string()),
        writer_group_id: 1,
        network_message_number: 0,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 10,
            sequence_number: 1,
            timestamp: Some(DateTime::now()),
            status: Some(StatusCode::Good),
            fields: vec![Variant::from(42.0f64)],
        }],
    };

    let secured = engine
        .sign_publisher_uadp_message("group-1", SecurityPolicy::PubSubAes256Ctr, &message, &ctx)
        .unwrap();

    // Secured bytes carry the SecurityHeader + appended HMAC signature, so they differ from and
    // are longer than the plain encoding.
    let plain = message.encode_to_vec(&ctx);
    assert_ne!(secured, plain);
    assert!(secured.len() > plain.len());

    let decoded = UadpSecurityCodec::new(
        MessageSecurityMode::Sign,
        SecurityPolicy::PubSubAes256Ctr,
        key_set,
    )
    .decode_network_message(&secured, &ctx)
    .unwrap();
    assert_eq!(decoded, message);
}

#[test]
fn decodes_subscriber_uadp_with_registered_security_group() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut engine = PubSubEngine::new(address_space());
    let security_group = SecurityGroup::new("group-1", Duration::from_secs(3600)).unwrap();
    let key_set = security_group.current_key_set().clone();
    engine.register_security_group(security_group);

    let message = UadpNetworkMessage {
        publisher_id: PublisherId::String("publisher-1".to_string()),
        writer_group_id: 1,
        network_message_number: 0,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 10,
            sequence_number: 1,
            timestamp: Some(DateTime::now()),
            status: Some(StatusCode::Good),
            fields: vec![Variant::from(42.0f64)],
        }],
    };

    let secured = UadpSecurityCodec::new(
        MessageSecurityMode::SignAndEncrypt,
        SecurityPolicy::PubSubAes256Ctr,
        key_set,
    )
    .encode_network_message(&message, &ctx)
    .unwrap();

    let decoded = engine
        .decode_subscriber_uadp_message(
            "group-1",
            MessageSecurityMode::SignAndEncrypt,
            SecurityPolicy::PubSubAes256Ctr,
            &secured,
            &ctx,
        )
        .unwrap();

    assert_eq!(decoded, message);
}
