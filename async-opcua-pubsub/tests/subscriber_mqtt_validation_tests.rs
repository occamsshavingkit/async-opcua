//! MQTT subscriber startup validation regression tests.

use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_pubsub::{DataSetReaderConfig, PubSubConnectionConfig, PubSubEngine, ReaderGroupConfig};
use opcua_server::address_space::AddressSpace;
use opcua_types::StatusCode;

#[tokio::test]
async fn mqtt_subscriber_startup_is_atomic_when_a_connection_is_malformed() {
    // Given: two reader-bearing MQTT connections, with the malformed one second.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let reader_group = || ReaderGroupConfig {
        dataset_readers: vec![DataSetReaderConfig::default()],
        ..ReaderGroupConfig::default()
    };
    let connections = vec![
        PubSubConnectionConfig {
            connection_id: "valid-mqtt".to_string(),
            name: "valid-mqtt".to_string(),
            address: "mqtt://broker.example:1883".to_string(),
            reader_groups: vec![reader_group()],
            writer_groups: Vec::new(),
        },
        PubSubConnectionConfig {
            connection_id: "malformed-mqtt".to_string(),
            name: "malformed-mqtt".to_string(),
            address: "mqtt://broker example:1883".to_string(),
            reader_groups: vec![reader_group()],
            writer_groups: Vec::new(),
        },
    ];
    let mut engine = PubSubEngine::with_connections(address_space, connections);

    // When: subscriber startup validates and dispatches both connections.
    let result = engine.start_subscribers().await;

    // Then: malformed configuration fails startup without publishing partial state.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
    assert!(!engine.subscribers_are_running());
    assert_eq!(engine.active_subscriber_handle_count(), 0);
}
