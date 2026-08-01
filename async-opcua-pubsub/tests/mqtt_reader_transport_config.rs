//! MQTT DataSetReader transport configuration regression tests.

use opcua_pubsub::{
    transport::mqtt::quality_of_service, DataSetReaderConfig, MqttDeliveryGuarantee,
    MqttReaderTransportConfig,
};
use opcua_types::{
    BrokerDataSetReaderTransportDataType, BrokerTransportQualityOfService, DataSetReaderDataType,
    ExtensionObject, UAString,
};
use rumqttc::QoS;

#[test]
fn dataset_reader_preserves_broker_transport_settings_when_configured() {
    // Given
    let source = DataSetReaderDataType {
        transport_settings: ExtensionObject::from_message(BrokerDataSetReaderTransportDataType {
            queue_name: UAString::from("factory/+/telemetry"),
            requested_delivery_guarantee: BrokerTransportQualityOfService::ExactlyOnce,
            ..BrokerDataSetReaderTransportDataType::default()
        }),
        ..DataSetReaderDataType::default()
    };

    // When
    let snapshot = DataSetReaderConfig::from_data_type(&source, 7);

    // Then
    assert_eq!(
        snapshot.mqtt_transport,
        Some(MqttReaderTransportConfig {
            topic_filter: Some("factory/+/telemetry".to_string()),
            delivery_guarantee: MqttDeliveryGuarantee::ExactlyOnce,
        })
    );
}

#[test]
fn dataset_reader_uses_configured_mqtt_topic_when_present() {
    // Given
    let reader = DataSetReaderConfig {
        writer_group_id: Some(12),
        mqtt_transport: Some(MqttReaderTransportConfig {
            topic_filter: Some("factory/+/telemetry".to_string()),
            delivery_guarantee: MqttDeliveryGuarantee::AtLeastOnce,
        }),
        ..DataSetReaderConfig::default()
    };

    // When
    let topic_filter = reader.mqtt_topic_filter(3);

    // Then
    assert_eq!(topic_filter, "factory/+/telemetry");
}

#[test]
fn dataset_reader_falls_back_to_telemetry_topic_when_queue_name_is_absent() {
    // Given
    let reader = DataSetReaderConfig {
        writer_group_id: Some(12),
        ..DataSetReaderConfig::default()
    };

    // When
    let topic_filter = reader.mqtt_topic_filter(3);

    // Then
    assert_eq!(topic_filter, "opcua/telemetry/12");
}

#[test]
fn broker_delivery_guarantees_map_to_mqtt_qos() {
    // Given
    let cases = [
        (MqttDeliveryGuarantee::BestEffort, QoS::AtMostOnce),
        (MqttDeliveryGuarantee::AtMostOnce, QoS::AtMostOnce),
        (MqttDeliveryGuarantee::AtLeastOnce, QoS::AtLeastOnce),
        (MqttDeliveryGuarantee::ExactlyOnce, QoS::ExactlyOnce),
    ];

    for (delivery_guarantee, expected_qos) in cases {
        // When
        let qos = quality_of_service(delivery_guarantee);

        // Then
        assert_eq!(qos, expected_qos);
    }
}
