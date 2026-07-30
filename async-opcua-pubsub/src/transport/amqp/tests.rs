use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use opcua_types::StatusCode;
use tokio_util::sync::CancellationToken;

use crate::{PubSubConnectionConfig, PubSubPublisher};

use super::*;

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
