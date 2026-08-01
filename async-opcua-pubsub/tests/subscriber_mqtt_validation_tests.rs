//! MQTT subscriber startup validation regression tests.

use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_pubsub::{
    DataSetReaderConfig, MessageEncoding, PubSubConnectionConfig, PubSubEngine, ReaderGroupConfig,
    SecurityGroup,
};
use opcua_server::address_space::AddressSpace;
use opcua_types::{MessageSecurityMode, StatusCode};

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

#[tokio::test]
async fn secured_json_mqtt_reader_is_rejected_before_startup() {
    // Given a JSON MQTT reader configured with UADP message security material.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let connection = PubSubConnectionConfig {
        connection_id: "secured-json".to_string(),
        name: "secured-json".to_string(),
        address: "mqtt://broker.example:1883".to_string(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            security_mode: Some(MessageSecurityMode::SignAndEncrypt),
            security_policy_uri: Some(SecurityPolicy::PubSubAes256Ctr.to_uri().to_string()),
            security_group_id: Some("line-a".to_string()),
            dataset_readers: vec![DataSetReaderConfig {
                message_encoding: MessageEncoding::Json,
                ..DataSetReaderConfig::default()
            }],
        }],
        writer_groups: Vec::new(),
    };
    let mut engine = PubSubEngine::with_connections(address_space, vec![connection]);
    engine
        .register_security_group(SecurityGroup::new("line-a", Duration::from_secs(3600)).unwrap());

    // When subscriber startup prepares the configured reader.
    let result = engine.start_subscribers().await;
    if result.is_ok() {
        engine.stop_subscribers().await;
    }

    // Then authenticated JSON is rejected before any transport task is committed.
    assert_eq!(result, Err(StatusCode::BadNotSupported));
    assert!(!engine.subscribers_are_running());
    assert_eq!(engine.active_subscriber_handle_count(), 0);
}
