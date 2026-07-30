//! MQTT subscriber startup validation regression tests.

use opcua_pubsub::transport::mqtt::{start_mqtt_subscriber, MqttBrokerAddressError};

#[test]
fn malformed_mqtt_broker_is_rejected_before_subscriber_task_starts() {
    // Given: a malformed MQTT broker address and a bounded payload channel.
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);

    // When: the public subscriber startup API validates the address.
    let result = start_mqtt_subscriber(
        "mqtt://broker example:1883".to_string(),
        "opcua/telemetry".to_string(),
        sender,
    );

    // Then: startup fails before a subscriber task is spawned.
    assert_eq!(result.err(), Some(MqttBrokerAddressError::InvalidHost));
}
