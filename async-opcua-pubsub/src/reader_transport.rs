use std::time::Duration;

use opcua_types::{
    AttributeId, BrokerDataSetReaderTransportDataType, BrokerTransportQualityOfService,
    DataSetReaderDataType, ExtensionObject, FieldTargetDataType, JsonDataSetReaderMessageDataType,
    MessageSecurityMode, NumericRange, TargetVariablesDataType, UAString, Variant,
};
use serde::{Deserialize, Serialize};

use crate::{
    codec::uadp::PublisherId,
    config::{DataSetReaderConfig, FieldTargetConfig, MessageEncoding},
};

/// Delivery guarantee requested by an MQTT DataSetReader.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MqttDeliveryGuarantee {
    /// Deliver without broker acknowledgement.
    BestEffort,
    /// Deliver at most once.
    AtMostOnce,
    /// Deliver at least once.
    #[default]
    AtLeastOnce,
    /// Deliver exactly once.
    ExactlyOnce,
}

/// MQTT-specific transport settings retained in the local reader snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MqttReaderTransportConfig {
    /// MQTT topic filter sourced from `BrokerDataSetReaderTransportDataType.QueueName`.
    pub topic_filter: Option<String>,
    /// Delivery guarantee sourced from `RequestedDeliveryGuarantee`.
    pub delivery_guarantee: MqttDeliveryGuarantee,
}

impl DataSetReaderConfig {
    /// Converts a Part 14 `DataSetReaderDataType` into the local dataset-reader config model.
    #[must_use]
    pub fn from_data_type(value: &DataSetReaderDataType, dataset_reader_id: u16) -> Self {
        Self {
            name: non_empty_ua_string(&value.name),
            dataset_reader_id,
            dataset_writer_id: value.data_set_writer_id,
            publisher_id: publisher_id_from_variant(&value.publisher_id),
            writer_group_id: Some(value.writer_group_id),
            network_message_number: None,
            message_receive_timeout: duration_from_millis(value.message_receive_timeout),
            metadata_major_version: Some(
                value.data_set_meta_data.configuration_version.major_version,
            ),
            security_mode: security_mode_option(value.security_mode),
            security_policy_uri: None,
            security_group_id: non_empty_ua_string(&value.security_group_id),
            message_encoding: message_encoding_from_settings(&value.message_settings),
            target_variables: target_variables_from_subscribed_data_set(&value.subscribed_data_set),
            subscribed_variables: Vec::new(),
            mqtt_transport: mqtt_transport_from_settings(&value.transport_settings),
            ..Self::default()
        }
    }

    /// Resolves the configured MQTT topic filter or the legacy telemetry topic fallback.
    #[must_use]
    pub fn mqtt_topic_filter(&self, reader_group_id: u16) -> String {
        self.mqtt_transport
            .as_ref()
            .and_then(|transport| transport.topic_filter.as_ref())
            .filter(|topic_filter| !topic_filter.is_empty())
            .cloned()
            .unwrap_or_else(|| match self.writer_group_id {
                Some(writer_group_id) => format!("opcua/telemetry/{writer_group_id}"),
                None => format!("opcua/telemetry/{reader_group_id}"),
            })
    }
}

fn mqtt_transport_from_settings(settings: &ExtensionObject) -> Option<MqttReaderTransportConfig> {
    let settings = settings.inner_as::<BrokerDataSetReaderTransportDataType>()?;
    Some(MqttReaderTransportConfig {
        topic_filter: non_empty_ua_string(&settings.queue_name),
        delivery_guarantee: delivery_guarantee(settings.requested_delivery_guarantee),
    })
}

fn delivery_guarantee(value: BrokerTransportQualityOfService) -> MqttDeliveryGuarantee {
    match value {
        BrokerTransportQualityOfService::BestEffort => MqttDeliveryGuarantee::BestEffort,
        BrokerTransportQualityOfService::AtMostOnce => MqttDeliveryGuarantee::AtMostOnce,
        BrokerTransportQualityOfService::ExactlyOnce => MqttDeliveryGuarantee::ExactlyOnce,
        BrokerTransportQualityOfService::NotSpecified
        | BrokerTransportQualityOfService::AtLeastOnce => MqttDeliveryGuarantee::AtLeastOnce,
    }
}

fn message_encoding_from_settings(settings: &ExtensionObject) -> MessageEncoding {
    if settings.inner_is::<JsonDataSetReaderMessageDataType>() {
        MessageEncoding::Json
    } else {
        MessageEncoding::Uadp
    }
}

fn target_variables_from_subscribed_data_set(
    subscribed_data_set: &ExtensionObject,
) -> Vec<FieldTargetConfig> {
    let Some(targets) = subscribed_data_set.inner_as::<TargetVariablesDataType>() else {
        return Vec::new();
    };
    targets
        .target_variables
        .as_deref()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, target)| field_target_from_data_type(index, target))
        .collect()
}

fn field_target_from_data_type(
    dataset_field_index: usize,
    target: &FieldTargetDataType,
) -> FieldTargetConfig {
    FieldTargetConfig {
        dataset_field_index,
        dataset_field_id: if target.data_set_field_id == opcua_types::Guid::null() {
            None
        } else {
            Some(target.data_set_field_id.clone())
        },
        target_node_id: target.target_node_id.clone(),
        attribute_id: AttributeId::from_u32(target.attribute_id).unwrap_or(AttributeId::Value),
        index_range: numeric_range_to_option(&target.write_index_range),
        override_value_handling: target.override_value_handling,
    }
}

fn numeric_range_to_option(range: &NumericRange) -> Option<String> {
    if range.is_none() {
        None
    } else {
        Some(range.to_string())
    }
}

fn non_empty_ua_string(value: &UAString) -> Option<String> {
    let value = value.to_string();
    (!value.is_empty()).then_some(value)
}

fn security_mode_option(value: MessageSecurityMode) -> Option<MessageSecurityMode> {
    (value != MessageSecurityMode::Invalid).then_some(value)
}

fn duration_from_millis(value: f64) -> Option<Duration> {
    value
        .is_finite()
        .then_some(value)
        .filter(|millis| *millis > 0.0)
        .map(|millis| Duration::from_secs_f64(millis / 1000.0))
}

fn publisher_id_from_variant(value: &Variant) -> Option<PublisherId> {
    match value {
        Variant::Byte(value) => Some(PublisherId::Byte(*value)),
        Variant::UInt16(value) => Some(PublisherId::UInt16(*value)),
        Variant::UInt32(value) => Some(PublisherId::UInt32(*value)),
        Variant::UInt64(value) => Some(PublisherId::UInt64(*value)),
        Variant::String(value) => Some(PublisherId::String(value.to_string())),
        Variant::Empty => None,
        _ => None,
    }
}
