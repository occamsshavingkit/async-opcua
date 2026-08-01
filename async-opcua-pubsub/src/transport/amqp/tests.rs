use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use opcua_types::StatusCode;
use tokio_util::sync::CancellationToken;

use crate::{MessageEncoding, PubSubConnectionConfig, PubSubPublisher};

use super::*;

mod logging;

#[tokio::test]
async fn start_publishing_rejects_malformed_broker_before_spawn() {
    // Given: an AMQP publisher configured with a malformed broker address.
    let publisher = AmqpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let config = PubSubConnectionConfig {
        connection_id: "malformed-amqp".to_string(),
        name: "malformed-amqp".to_string(),
        address: "amqp://bad host/telemetry".to_string(),
        writer_groups: Vec::new(),
        reader_groups: Vec::new(),
    };

    // When: publishing is started directly.
    let result = publisher.start_publishing(config, CancellationToken::new());

    // Then: malformed configuration is rejected instead of returning a spawned handle.
    match result {
        Err(status) => assert_eq!(status, StatusCode::BadConfigurationError),
        Ok(handle) => {
            handle.abort();
            panic!("malformed AMQP broker returned a publisher handle");
        }
    }
}

#[test]
fn parses_amqp_address_with_prefix_and_queue() {
    let settings = parse_amqp_address("amqp://broker.local:5673/plant.telemetry")
        .expect("AMQP address should parse");

    assert_eq!(settings.broker_url, "amqp://broker.local:5673");
    assert_eq!(settings.routing_key, "plant.telemetry");
}

#[test]
fn parses_mixed_case_amqp_scheme_with_canonical_broker_url() {
    let settings = parse_amqp_address("AmQp://broker.local:5673/plant.telemetry")
        .expect("mixed-case AMQP scheme should parse");

    assert_eq!(settings.broker_url, "amqp://broker.local:5673");
    assert_eq!(settings.routing_key, "plant.telemetry");
}

#[test]
fn parses_mixed_case_amqps_scheme_with_canonical_broker_url() {
    let settings = parse_amqp_address("aMqPs://broker.local/plant.telemetry")
        .expect("mixed-case AMQPS scheme should parse");

    assert_eq!(settings.broker_url, "amqps://broker.local:5671");
    assert_eq!(settings.routing_key, "plant.telemetry");
}

#[test]
fn parses_amqp_address_without_prefix_using_default_port_and_queue() {
    let settings = parse_amqp_address("broker.local").expect("bare AMQP address should parse");

    assert_eq!(settings.broker_url, "amqp://broker.local:5672");
    assert_eq!(settings.routing_key, "opcua.telemetry");
}

#[test]
fn publish_immediate_keeps_bounded_fifo_cache() {
    let publisher = AmqpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));

    for i in 0..1100 {
        publisher.publish_immediate(format!("key-{i}"), vec![i as u8]);
    }

    assert_eq!(publisher.cached_message_count(), 1000);
    let first = publisher.pop_cached_message();
    assert_eq!(first, Some(("key-100".to_string(), vec![100u8])));
}

#[test]
fn publish_immediate_recovers_from_poisoned_cache_mutex() {
    // Given: a publisher whose private message cache mutex is poisoned by a standard thread.
    let publisher = AmqpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let cache = Arc::clone(&publisher.cache);
    let poisoner = std::thread::spawn(move || {
        let _cache_guard = cache.lock().unwrap();
        panic!("deliberately poison the AMQP message cache mutex");
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

#[tokio::test]
async fn aborting_coordinator_stops_writer_group_future() {
    let publisher = AmqpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "amqp-regression".to_string(),
                name: "AMQP regression".to_string(),
                address: "amqp://127.0.0.1:1/regression".to_string(),
                writer_groups: vec![crate::WriterGroupConfig {
                    writer_group_id: 1,
                    publishing_interval: 10,
                    encoding: MessageEncoding::Json,
                    dataset_writers: vec![crate::DataSetWriterConfig {
                        dataset_writer_id: 1,
                        dataset_name: "regression".to_string(),
                        published_dataset: crate::PublishedDataSetConfig {
                            published_variables: Vec::new(),
                            configuration_version: Default::default(),
                        },
                    }],
                }],
                reader_groups: Vec::new(),
            },
            cancel_token,
        )
        .expect("AMQP publisher should start");

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if publisher.cached_message_count() > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("writer group should populate the cache");

    coordinator.abort();
    assert!(coordinator
        .await
        .expect_err("coordinator was aborted")
        .is_cancelled());
    while publisher.pop_cached_message().is_some() {}
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(publisher.cached_message_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn aborting_coordinator_drops_writer_future_before_returning() {
    // Given: a coordinator that has taken direct ownership of one writer future.
    let publisher = AmqpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "amqp-abort-drop-regression".to_string(),
                name: "AMQP abort drop regression".to_string(),
                address: "amqp://127.0.0.1:1/regression".to_string(),
                writer_groups: vec![crate::WriterGroupConfig {
                    writer_group_id: 1,
                    publishing_interval: 60_000,
                    encoding: MessageEncoding::Json,
                    dataset_writers: Vec::new(),
                }],
                reader_groups: Vec::new(),
            },
            CancellationToken::new(),
        )
        .expect("AMQP publisher should start");
    tokio::time::timeout(Duration::from_secs(1), async {
        while Arc::strong_count(&publisher.address_space) < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("AMQP coordinator did not take ownership of the writer future");

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
        "AMQP writer future remained alive after the aborted coordinator returned"
    );
}

#[tokio::test]
async fn graceful_cancellation_stops_cache_production() {
    let publisher = AmqpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "amqp-graceful-shutdown".to_string(),
                name: "AMQP graceful shutdown".to_string(),
                address: "amqp://127.0.0.1:1/graceful-shutdown".to_string(),
                writer_groups: vec![crate::WriterGroupConfig {
                    writer_group_id: 1,
                    publishing_interval: 10,
                    encoding: MessageEncoding::Json,
                    dataset_writers: vec![crate::DataSetWriterConfig {
                        dataset_writer_id: 1,
                        dataset_name: "graceful-shutdown".to_string(),
                        published_dataset: crate::PublishedDataSetConfig {
                            published_variables: Vec::new(),
                            configuration_version: Default::default(),
                        },
                    }],
                }],
                reader_groups: Vec::new(),
            },
            cancel_token.clone(),
        )
        .expect("AMQP publisher should start");

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if publisher.cached_message_count() > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("writer group should populate the cache");

    cancel_token.cancel();
    tokio::time::timeout(Duration::from_secs(2), coordinator)
        .await
        .expect("coordinator should stop before the deadline")
        .expect("coordinator should shut down successfully");
    while publisher.pop_cached_message().is_some() {}
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(publisher.cached_message_count(), 0);
}
