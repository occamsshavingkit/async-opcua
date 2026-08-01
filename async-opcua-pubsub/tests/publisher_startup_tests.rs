//! Publisher startup rollback regression tests.

use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_pubsub::{MessageEncoding, PubSubConnectionConfig, PubSubEngine, WriterGroupConfig};
use opcua_server::address_space::AddressSpace;
use opcua_types::StatusCode;
use tokio::runtime::Handle;

fn connection(
    connection_id: &str,
    address: &str,
    writer_groups: Vec<WriterGroupConfig>,
) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
        address: address.to_string(),
        writer_groups,
        reader_groups: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_preflights_publishers_before_spawning_tasks() {
    // Given: a valid AMQP publisher with a writer group followed by unsupported MQTT over TLS.
    let earlier_amqp = connection(
        "amqp",
        "amqp://127.0.0.1:5672/opcua.telemetry",
        vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 1000,
            encoding: MessageEncoding::Uadp,
            dataset_writers: Vec::new(),
        }],
    );
    let later_mqtt = connection("mqtt", "mqtts://127.0.0.1:8883", Vec::new());
    let mut engine = PubSubEngine::with_connections(
        Arc::new(RwLock::new(AddressSpace::new())),
        vec![earlier_amqp, later_mqtt],
    );
    let runtime = Handle::current();
    let tasks_before_start = runtime.metrics().num_alive_tasks();

    // When: publisher startup validates the configured connections.
    let result = engine.start();

    // Then: startup reports the unsupported address without leaving a writer-group task alive.
    assert_eq!(result, Err(StatusCode::BadNotSupported));
    assert_eq!(runtime.metrics().num_alive_tasks(), tasks_before_start);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_rejects_malformed_udp_before_spawning_earlier_publisher() {
    // Given: a valid AMQP publisher with a writer group followed by malformed UDP.
    let earlier_amqp = connection(
        "amqp",
        "amqp://127.0.0.1:5672/opcua.telemetry",
        vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 1000,
            encoding: MessageEncoding::Uadp,
            dataset_writers: Vec::new(),
        }],
    );
    let later_udp = connection("udp", "udp://not-a-socket-address", Vec::new());
    let mut engine = PubSubEngine::with_connections(
        Arc::new(RwLock::new(AddressSpace::new())),
        vec![earlier_amqp, later_udp],
    );
    let runtime = Handle::current();
    let tasks_before_start = runtime.metrics().num_alive_tasks();

    // When: publisher startup validates the configured connections.
    let result = engine.start();

    // Then: startup rejects malformed UDP without leaving the earlier publisher alive.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
    assert_eq!(runtime.metrics().num_alive_tasks(), tasks_before_start);
}

#[tokio::test]
async fn startup_accepts_standard_mixed_case_opc_udp_address() {
    // Given: the OPC-10000-14 §§7.3.2.2-7.3.2.3 UDP URI scheme.
    let udp = connection("udp", "OpC.UdP://127.0.0.1:4840", Vec::new());
    let mut engine =
        PubSubEngine::with_connections(Arc::new(RwLock::new(AddressSpace::new())), vec![udp]);

    // When: publisher startup preflights the connection.
    let result = engine.start();

    // Then: the standards-compliant scheme is accepted before any writer groups run.
    assert_eq!(result, Ok(()));
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_rejects_malformed_amqp_before_spawning_earlier_publisher() {
    // Given: a valid AMQP publisher with a writer group followed by malformed AMQP.
    let earlier_amqp = connection(
        "amqp",
        "amqp://127.0.0.1:5672/opcua.telemetry",
        vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 1000,
            encoding: MessageEncoding::Uadp,
            dataset_writers: Vec::new(),
        }],
    );
    let later_amqp = connection("later-amqp", "amqp://bad host/telemetry", Vec::new());
    let mut engine = PubSubEngine::with_connections(
        Arc::new(RwLock::new(AddressSpace::new())),
        vec![earlier_amqp, later_amqp],
    );
    let runtime = Handle::current();
    let tasks_before_start = runtime.metrics().num_alive_tasks();

    // When: publisher startup validates the configured connections.
    let result = engine.start();

    // Then: startup rejects malformed AMQP without leaving the earlier publisher alive.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
    assert_eq!(runtime.metrics().num_alive_tasks(), tasks_before_start);
}
