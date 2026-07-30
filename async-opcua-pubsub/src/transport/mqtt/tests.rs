use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use tokio_util::sync::CancellationToken;

use crate::{
    MessageEncoding, MqttPublisher, PubSubConnectionConfig, PubSubPublisher, WriterGroupConfig,
};

#[tokio::test]
async fn graceful_cancellation_interrupts_blocked_publish_and_drops_writer_future() {
    // Given: enough cached messages to block the MQTT request channel before it is polled.
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    for index in 0..100 {
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
            if publisher.cache.lock().unwrap().len() == 49 {
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

    // Then: returning from the coordinator proves its owned writer future was dropped.
    assert_eq!(
        Arc::strong_count(&publisher.cache),
        1,
        "MQTT coordinator returned before its writer task was cleaned up"
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
    let publisher = MqttPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
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
    tokio::time::timeout(Duration::from_secs(1), async {
        while Arc::strong_count(&publisher.address_space) < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("MQTT coordinator did not take ownership of the writer future");

    // When: the coordinator is aborted and awaited to completion.
    let observed_address_space = Arc::clone(&publisher.address_space);
    let observed_strong_count = tokio::spawn(async move {
        coordinator.abort();
        let join_error = coordinator
            .await
            .expect_err("coordinator completed instead of being aborted");
        assert!(join_error.is_cancelled(), "coordinator was not cancelled");
        Arc::strong_count(&observed_address_space)
    })
    .await
    .expect("ownership observer task failed");

    // Then: no writer future retains the publisher's address space.
    assert_eq!(
        observed_strong_count, 2,
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
    let backlog_remaining = publisher.cache.lock().unwrap().len();

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
    let cache = publisher
        .cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        cache.back(),
        Some(&("test/poisoned-cache".to_string(), vec![1, 2, 3]))
    );
}
