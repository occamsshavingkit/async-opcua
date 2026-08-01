// allow: SIZE_OK — MQTT transport regression fixtures are intentionally colocated in this test module.
use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use super::{lock_cache, publisher::REQUEST_CHANNEL_CAPACITY, MAX_CACHE_SIZE};
use crate::{
    MessageEncoding, MqttPublisher, PubSubConnectionConfig, PubSubPublisher, WriterGroupConfig,
};

struct Qos1Publish {
    packet_identifier: u16,
    topic: String,
    payload: Vec<u8>,
}

async fn read_mqtt_packet(socket: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut fixed_header = [0_u8; 1];
    socket.read_exact(&mut fixed_header).await?;

    let mut remaining_length = 0_usize;
    for multiplier in [1_usize, 128, 16_384, 2_097_152] {
        let mut encoded = [0_u8; 1];
        socket.read_exact(&mut encoded).await?;
        remaining_length += usize::from(encoded[0] & 0x7f) * multiplier;
        if encoded[0] & 0x80 == 0 {
            let mut body = vec![0_u8; remaining_length];
            socket.read_exact(&mut body).await?;
            return Ok((fixed_header[0], body));
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "MQTT remaining length exceeds four bytes",
    ))
}

async fn accept_mqtt311_connection(listener: &TcpListener) -> TcpStream {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let (mut socket, _) = tokio::time::timeout_at(deadline, listener.accept())
            .await
            .expect("timed out waiting for MQTT client connection")
            .expect("local MQTT broker accept failed");
        let connect = tokio::time::timeout_at(deadline, read_mqtt_packet(&mut socket))
            .await
            .expect("timed out waiting for MQTT CONNECT");
        match connect {
            Ok((fixed_header, _connect_body)) => {
                assert_eq!(fixed_header >> 4, 1, "expected MQTT CONNECT packet");
                socket
                    .write_all(&[0x20, 0x02, 0x00, 0x00])
                    .await
                    .expect("failed to send successful MQTT 3.1.1 CONNACK");
                return socket;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                ) => {}
            Err(error) => panic!("failed to read MQTT CONNECT: {error}"),
        }
    }
}

async fn read_qos1_publish(socket: &mut TcpStream) -> std::io::Result<Qos1Publish> {
    let (fixed_header, body) = read_mqtt_packet(socket).await?;
    assert_eq!(fixed_header >> 4, 3, "expected MQTT PUBLISH packet");
    assert_eq!(
        (fixed_header >> 1) & 0x03,
        1,
        "incoming MQTT PUBLISH must use QoS1"
    );
    assert!(
        body.len() >= 4,
        "QoS1 PUBLISH body must contain a topic and packet identifier"
    );

    let topic_length = usize::from(u16::from_be_bytes([body[0], body[1]]));
    let packet_identifier_offset = 2 + topic_length;
    assert!(
        body.len() >= packet_identifier_offset + 2,
        "QoS1 PUBLISH body ended before its packet identifier"
    );
    let topic = String::from_utf8(body[2..packet_identifier_offset].to_vec())
        .expect("MQTT PUBLISH topic must be valid UTF-8");
    let packet_identifier = u16::from_be_bytes([
        body[packet_identifier_offset],
        body[packet_identifier_offset + 1],
    ]);
    let payload = body[packet_identifier_offset + 2..].to_vec();

    Ok(Qos1Publish {
        packet_identifier,
        topic,
        payload,
    })
}

#[tokio::test]
async fn graceful_cancellation_restores_all_qos1_messages_in_fifo_order() {
    // Given: enough cached messages to block the MQTT request channel before it is polled.
    const INITIAL_CACHE_SIZE: usize = 100;
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    for index in 0..INITIAL_CACHE_SIZE {
        publisher.publish_immediate(format!("test/{index}"), Vec::new());
    }
    let connection = PubSubConnectionConfig {
        connection_id: "graceful-cancellation".to_string(),
        name: "graceful-cancellation".to_string(),
        address: "mqtt://127.0.0.1:1".to_string(),
        writer_groups: vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 60_000,
            encoding: MessageEncoding::Json,
            dataset_writers: Vec::new(),
        }],
        reader_groups: Vec::new(),
    };
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(connection, cancel_token.clone())
        .expect("valid MQTT endpoint syntax should start publishing");

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if lock_cache(&publisher.cache).len() <= INITIAL_CACHE_SIZE - REQUEST_CHANNEL_CAPACITY {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("MQTT publisher did not fill its request channel");

    // When: graceful cancellation is requested while cache publication is blocked.
    cancel_token.cancel();
    tokio::time::timeout(Duration::from_secs(1), coordinator)
        .await
        .expect("MQTT coordinator ignored cancellation while cache publication was blocked")
        .expect("MQTT coordinator task failed during graceful cancellation");

    // Then: every locally owned message is restored in its original FIFO order.
    let expected_topics: Vec<_> = (0..INITIAL_CACHE_SIZE)
        .map(|index| format!("test/{index}"))
        .collect();
    let actual_topics: Vec<_> = lock_cache(&publisher.cache)
        .iter()
        .map(|(topic, _payload)| topic.clone())
        .collect();
    assert_eq!(actual_topics, expected_topics);
}

#[tokio::test]
async fn graceful_cancellation_restoration_prefers_oldest_within_cache_budget() {
    // Given: a full cache whose oldest messages move into MQTT-owned publish state.
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    for index in 0..MAX_CACHE_SIZE {
        publisher.publish_immediate(format!("old/{index}"), Vec::new());
    }
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "bounded-cancellation".to_string(),
                name: "bounded-cancellation".to_string(),
                address: "mqtt://127.0.0.1:1".to_string(),
                writer_groups: Vec::new(),
                reader_groups: Vec::new(),
            },
            cancel_token.clone(),
        )
        .expect("valid MQTT endpoint syntax should start publishing");
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if lock_cache(&publisher.cache).len() <= MAX_CACHE_SIZE - REQUEST_CHANNEL_CAPACITY {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("MQTT publisher did not move messages into publish state");
    for index in 0..MAX_CACHE_SIZE {
        publisher.publish_immediate(format!("new/{index}"), Vec::new());
    }

    // When: cancellation restores MQTT-owned messages into an already full cache.
    cancel_token.cancel();
    tokio::time::timeout(Duration::from_secs(1), coordinator)
        .await
        .expect("MQTT coordinator ignored bounded-cache cancellation")
        .expect("MQTT coordinator failed during bounded-cache cancellation");

    // Then: the single cache budget keeps the oldest locally owned message.
    let restored = lock_cache(&publisher.cache);
    assert_eq!(
        (
            restored.len(),
            restored.front().map(|(topic, _payload)| topic.as_str())
        ),
        (MAX_CACHE_SIZE, Some("old/0"))
    );
}

#[tokio::test]
async fn aborting_coordinator_stops_writer_group_future() {
    // Given: a publisher with a short-interval writer group and an unreachable MQTT broker.
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let connection = PubSubConnectionConfig {
        connection_id: "abort-regression".to_string(),
        name: "abort-regression".to_string(),
        address: "mqtt://127.0.0.1:1".to_string(),
        writer_groups: vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 1,
            encoding: MessageEncoding::Json,
            dataset_writers: Vec::new(),
        }],
        reader_groups: Vec::new(),
    };
    let cancel_token = CancellationToken::new();

    // When: the coordinator is aborted before the runtime yields, without cancelling the token.
    let coordinator = publisher
        .start_publishing(connection, cancel_token)
        .expect("valid MQTT endpoint syntax should start publishing");
    coordinator.abort();
    let join_error = coordinator
        .await
        .expect_err("coordinator completed instead of being aborted");
    assert!(
        join_error.is_cancelled(),
        "coordinator task was not cancelled"
    );

    // Then: aborting the coordinator also prevents its writer group from populating the cache.
    assert!(
        publisher.cache.lock().unwrap().is_empty(),
        "writer-group task continued after coordinator abort"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn aborting_coordinator_drops_writer_future_before_returning() {
    // Given: a coordinator that has taken direct ownership of one writer future.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let address_space_weak = Arc::downgrade(&address_space);
    let publisher = MqttPublisher::new(address_space);
    let connection = PubSubConnectionConfig {
        connection_id: "abort-drop-regression".to_string(),
        name: "abort-drop-regression".to_string(),
        address: "mqtt://127.0.0.1:1".to_string(),
        writer_groups: vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 60_000,
            encoding: MessageEncoding::Json,
            dataset_writers: Vec::new(),
        }],
        reader_groups: Vec::new(),
    };
    let coordinator = publisher
        .start_publishing(connection, CancellationToken::new())
        .expect("valid MQTT endpoint syntax should start publishing");
    drop(publisher);
    assert!(
        address_space_weak.upgrade().is_some(),
        "MQTT coordinator did not retain the address space before abort"
    );

    // When: the coordinator is aborted and awaited to completion.
    coordinator.abort();
    let join_error = coordinator
        .await
        .expect_err("coordinator completed instead of being aborted");
    assert!(join_error.is_cancelled(), "coordinator was not cancelled");

    // Then: no writer future retains the publisher's address space.
    assert!(
        address_space_weak.upgrade().is_none(),
        "MQTT writer future remained alive after the aborted coordinator returned"
    );
}

#[tokio::test]
async fn mqtt_event_loop_connects_with_sustained_cache_backlog() {
    // Given: a local TCP listener and more cached messages than the MQTT request channel holds.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local TCP listener should bind");
    let address = listener
        .local_addr()
        .expect("local TCP listener should expose its address");
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    for index in 0..100 {
        publisher.publish_immediate(format!("test/{index}"), Vec::new());
    }
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "cache-backlog-connect".to_string(),
                name: "cache-backlog-connect".to_string(),
                address: format!("mqtt://{address}"),
                writer_groups: Vec::new(),
                reader_groups: Vec::new(),
            },
            cancel_token.clone(),
        )
        .expect("valid MQTT endpoint syntax should start publishing");

    // When: the broker waits for the publisher to open its TCP connection.
    let accepted = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await;
    let backlog_remaining = lock_cache(&publisher.cache).len();

    // Then: event-loop polling opens the connection without draining the entire backlog first.
    cancel_token.cancel();
    if !coordinator.is_finished() {
        coordinator.abort();
    }
    let _ = coordinator.await;
    let (_socket, _peer) = accepted
        .expect("MQTT event loop did not open the broker TCP connection")
        .expect("local broker TCP accept failed");
    assert!(
        backlog_remaining > 0,
        "MQTT cache backlog was drained before the broker connection was accepted"
    );
}

#[tokio::test]
async fn qos1_cached_message_is_redelivered_after_disconnect_before_puback() {
    // Given: one application message and a broker that drops its first connection before PUBACK.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local TCP listener should bind");
    let address = listener
        .local_addr()
        .expect("local TCP listener should expose its address");
    let broker = tokio::spawn(async move {
        let mut first_socket = accept_mqtt311_connection(&listener).await;
        let first_publish =
            tokio::time::timeout(Duration::from_secs(2), read_qos1_publish(&mut first_socket))
                .await
                .expect("timed out waiting for initial QoS1 PUBLISH")
                .expect("failed to read initial QoS1 PUBLISH");
        assert_ne!(
            first_publish.packet_identifier, 0,
            "QoS1 PUBLISH packet identifier must be non-zero"
        );
        drop(first_socket);

        let mut second_socket = accept_mqtt311_connection(&listener).await;
        let second_publish = tokio::time::timeout(
            Duration::from_secs(2),
            read_qos1_publish(&mut second_socket),
        )
        .await
        .expect("timed out waiting for redelivered QoS1 PUBLISH before PUBACK")
        .expect("failed to read redelivered QoS1 PUBLISH");
        assert_ne!(
            second_publish.packet_identifier, 0,
            "redelivered QoS1 PUBLISH packet identifier must be non-zero"
        );
        assert_eq!(second_publish.topic, first_publish.topic);
        assert_eq!(second_publish.payload, first_publish.payload);
    });

    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    publisher.publish_immediate(
        "regression/qos1/redelivery-before-puback".to_string(),
        b"application-owned-until-puback".to_vec(),
    );
    let cancel_token = CancellationToken::new();

    // When: publishing starts and the broker forces a reconnect before acknowledging the message.
    let mut coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "qos1-redelivery-before-puback".to_string(),
                name: "qos1-redelivery-before-puback".to_string(),
                address: format!("mqtt://{address}"),
                writer_groups: Vec::new(),
                reader_groups: Vec::new(),
            },
            cancel_token.clone(),
        )
        .expect("valid MQTT endpoint syntax should start publishing");
    let broker_result = broker.await;

    // Then: broker assertions prove the same application-owned message was redelivered.
    cancel_token.cancel();
    if tokio::time::timeout(Duration::from_secs(1), &mut coordinator)
        .await
        .is_err()
    {
        coordinator.abort();
        let _ = coordinator.await;
    }
    broker_result.expect("local MQTT broker task failed");
}

#[test]
fn publish_immediate_recovers_from_poisoned_cache_mutex() {
    // Given: a publisher whose private message cache mutex is poisoned by a standard thread.
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let cache = Arc::clone(&publisher.cache);
    let poisoner = std::thread::spawn(move || {
        let _cache_guard = cache.lock().unwrap();
        panic!("deliberately poison the MQTT message cache mutex");
    });
    assert!(poisoner.join().is_err(), "poisoning thread should panic");

    // When: a message is published immediately after the mutex is poisoned.
    publisher.publish_immediate("test/poisoned-cache".to_string(), vec![1, 2, 3]);

    // Then: the message is queued despite the poisoned mutex.
    let cache = lock_cache(&publisher.cache);
    assert_eq!(
        cache.back(),
        Some(&("test/poisoned-cache".to_string(), vec![1, 2, 3]))
    );
}
