use super::*;

mod backpressure;

#[test]
fn broker_address_error_implements_std_error() {
    fn assert_error<T: std::error::Error>() {}

    assert_error::<MqttBrokerAddressError>();
}

#[test]
fn mqtts_broker_address_is_rejected_when_tls_is_unsupported() {
    // Given
    let broker_address = "mqtts://broker.example:8883";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::TlsUnsupported));
}

#[test]
fn whitespace_wrapped_mqtts_broker_address_is_rejected_when_tls_is_unsupported() {
    // Given
    let broker_address = " mqtts://broker.example:8883 ";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::TlsUnsupported));
}

#[test]
fn whitespace_wrapped_mqtt_broker_address_preserves_explicit_port() {
    // Given
    let broker_address = " mqtt://broker.example:1884 ";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(
        result,
        Ok(MqttBrokerAddress {
            host: "broker.example".to_string(),
            port: 1884,
        })
    );
}

#[test]
fn ascii_control_whitespace_wrapped_mqtt_broker_address_preserves_explicit_port() {
    // Given
    let broker_address = " \t\nmqtt://broker.example:1884\r\n ";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(
        result,
        Ok(MqttBrokerAddress {
            host: "broker.example".to_string(),
            port: 1884,
        })
    );
}

#[test]
fn bare_broker_address_uses_default_mqtt_port() {
    // Given
    let broker_address = "broker.example";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(
        result,
        Ok(MqttBrokerAddress {
            host: "broker.example".to_string(),
            port: 1883,
        })
    );
}

#[test]
fn broker_address_accepts_normalized_root_path() {
    let result = parse_broker_address("mqtt://broker.example/");

    assert_eq!(
        result,
        Ok(MqttBrokerAddress {
            host: "broker.example".to_string(),
            port: 1883,
        })
    );
}

#[test]
fn broker_address_normalizes_ipv6_host_without_brackets() {
    let result = parse_broker_address("mqtt://[::1]:1883");

    assert_eq!(
        result,
        Ok(MqttBrokerAddress {
            host: "::1".to_string(),
            port: 1883,
        })
    );
}

#[test]
fn broker_address_returns_extra_components_for_unrelated_uri_scheme() {
    assert_eq!(
        parse_broker_address("http://broker.example:1883"),
        Err(MqttBrokerAddressError::ExtraComponents)
    );
}

#[test]
fn broker_address_rejects_empty_host() {
    // Given
    let broker_address = "mqtt://:1883";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::InvalidHost));
}

#[test]
fn broker_address_rejects_whitespace_in_host() {
    // Given
    let broker_address = "mqtt://broker example:1883";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::InvalidHost));
}

#[test]
fn broker_address_rejects_ascii_control_characters_in_authority() {
    for broker_address in [
        "mqtt://broker\n.example:1883",
        "mqtt://broker\r.example:1883",
        "mqtt://broker\t.example:1883",
    ] {
        assert_eq!(
            parse_broker_address(broker_address),
            Err(MqttBrokerAddressError::InvalidHost),
            "broker address with ASCII control character should be rejected: {broker_address:?}"
        );
    }
}

#[test]
fn broker_address_rejects_invalid_port() {
    // Given
    let broker_address = "mqtt://broker.example:not-a-port";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::InvalidPort));
}

#[test]
fn broker_address_rejects_explicit_empty_port() {
    // Given
    let broker_address = "mqtt://broker.example:";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::InvalidPort));
}

#[test]
fn bare_broker_address_rejects_explicit_empty_port() {
    // Given
    let broker_address = "broker.example:";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::InvalidPort));
}

#[test]
fn broker_address_rejects_port_zero() {
    // Given
    let broker_address = "mqtt://broker.example:0";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::InvalidPort));
}

#[test]
fn broker_address_rejects_extra_authority_components() {
    // Given
    let broker_address = "mqtt://broker.example:1883:extra";

    // When
    let result = parse_broker_address(broker_address);

    // Then
    assert_eq!(result, Err(MqttBrokerAddressError::ExtraComponents));
}

#[test]
fn broker_address_rejects_uri_components_outside_authority() {
    for broker_address in [
        "mqtt://broker.example/path",
        "mqtt://broker.example?query",
        "mqtt://broker.example#fragment",
        "mqtt://user@broker.example",
    ] {
        assert_eq!(
            parse_broker_address(broker_address),
            Err(MqttBrokerAddressError::ExtraComponents),
            "broker address should be rejected: {broker_address}"
        );
    }
}

#[test]
fn mqtts_broker_address_is_rejected_before_subscriber_task_starts() {
    // Given
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);

    // When
    let result = start_mqtt_subscriber(
        "mqtts://broker.example:8883".to_string(),
        "opcua/topic".to_string(),
        sender,
    );

    // Then
    assert!(matches!(
        result,
        Err(MqttBrokerAddressError::TlsUnsupported)
    ));
}
