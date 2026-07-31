use std::{borrow::Cow, time::Duration};

use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

use crate::MqttDeliveryGuarantee;

mod forwarding;

use forwarding::PayloadForwarder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MqttBrokerAddress {
    host: String,
    port: u16,
}

impl MqttBrokerAddress {
    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) const fn port(&self) -> u16 {
        self.port
    }
}

/// Error returned when an MQTT broker address cannot be used by the subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MqttBrokerAddressError {
    /// The MQTT TLS transport is not implemented.
    #[error("mqtt tls transport is unsupported")]
    TlsUnsupported,
    /// The broker address does not contain a host.
    #[error("mqtt broker address has an invalid host")]
    InvalidHost,
    /// The broker port is not a valid `u16`.
    #[error("mqtt broker address has an invalid port")]
    InvalidPort,
    /// The broker authority contains unsupported extra components.
    #[error("mqtt broker address contains unsupported extra components")]
    ExtraComponents,
}

impl MqttBrokerAddressError {
    /// Maps this broker-address error to the corresponding OPC UA status code.
    #[must_use]
    pub const fn status_code(self) -> opcua_types::StatusCode {
        match self {
            Self::TlsUnsupported => opcua_types::StatusCode::BadNotSupported,
            Self::InvalidHost | Self::InvalidPort | Self::ExtraComponents => {
                opcua_types::StatusCode::BadConfigurationError
            }
        }
    }
}

pub(crate) fn parse_broker_address(
    address: &str,
) -> Result<MqttBrokerAddress, MqttBrokerAddressError> {
    let address = address.trim();
    if address.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(MqttBrokerAddressError::InvalidHost);
    }
    if address.ends_with(':') {
        return Err(MqttBrokerAddressError::InvalidPort);
    }
    let address = if address
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mqtt://"))
        || address
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mqtts://"))
    {
        Cow::Borrowed(address)
    } else {
        Cow::Owned(format!("mqtt://{address}"))
    };
    let address = Url::parse(address.as_ref()).map_err(|error| match error {
        url::ParseError::InvalidPort => {
            let has_extra_authority = address
                .rsplit_once(':')
                .and_then(|(prefix, _)| Url::parse(prefix).ok())
                .and_then(|prefix| prefix.port())
                .is_some();
            if has_extra_authority {
                MqttBrokerAddressError::ExtraComponents
            } else {
                MqttBrokerAddressError::InvalidPort
            }
        }
        _ => MqttBrokerAddressError::InvalidHost,
    })?;

    match address.scheme() {
        "mqtt" => {}
        "mqtts" => return Err(MqttBrokerAddressError::TlsUnsupported),
        _ => return Err(MqttBrokerAddressError::ExtraComponents),
    }
    if !address.username().is_empty()
        || address.password().is_some()
        || !matches!(address.path(), "" | "/")
        || address.query().is_some()
        || address.fragment().is_some()
    {
        return Err(MqttBrokerAddressError::ExtraComponents);
    }
    let host = match address.host() {
        Some(Host::Domain(host)) if !host.is_empty() => host.to_string(),
        Some(Host::Ipv4(host)) => host.to_string(),
        Some(Host::Ipv6(host)) => host.to_string(),
        _ => return Err(MqttBrokerAddressError::InvalidHost),
    };
    let port = address.port().unwrap_or(1883);
    if port == 0 {
        return Err(MqttBrokerAddressError::InvalidPort);
    }

    Ok(MqttBrokerAddress { host, port })
}

/// Maps the Part 14 broker delivery guarantee to MQTT QoS.
#[must_use]
pub fn quality_of_service(delivery_guarantee: MqttDeliveryGuarantee) -> QoS {
    match delivery_guarantee {
        MqttDeliveryGuarantee::BestEffort | MqttDeliveryGuarantee::AtMostOnce => QoS::AtMostOnce,
        MqttDeliveryGuarantee::AtLeastOnce => QoS::AtLeastOnce,
        MqttDeliveryGuarantee::ExactlyOnce => QoS::ExactlyOnce,
    }
}

pub(crate) struct MqttSubscriberConfig {
    broker_address: MqttBrokerAddress,
    topic_filter: String,
    qos: QoS,
}

impl MqttSubscriberConfig {
    pub(crate) fn new(broker_address: MqttBrokerAddress, topic_filter: String, qos: QoS) -> Self {
        Self {
            broker_address,
            topic_filter,
            qos,
        }
    }
}

/// Starts an MQTT subscriber implementing the Broker DataSetReader transport
/// (OPC-10000-14 §6.4.2.6).
///
/// The subscriber uses the broker QueueName as its topic filter, forwards
/// received payload bytes over the supplied bounded channel, and reconnects
/// with exponential backoff. This compatibility entry point uses MQTT QoS 1.
///
/// # Errors
///
/// Returns [`MqttBrokerAddressError`] before spawning the subscriber task when
/// the broker address is invalid or requests the unsupported MQTT TLS transport.
pub fn start_mqtt_subscriber(
    broker_address: String,
    topic_filter: String,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Result<tokio::task::JoinHandle<()>, MqttBrokerAddressError> {
    start_mqtt_subscriber_with_cancel(
        broker_address,
        topic_filter,
        sender,
        CancellationToken::new(),
    )
}

/// Starts an MQTT subscriber that exits when `cancel_token` is cancelled.
///
/// # Errors
///
/// Returns [`MqttBrokerAddressError`] before spawning the subscriber task when
/// the broker address is invalid or requests the unsupported MQTT TLS transport.
pub fn start_mqtt_subscriber_with_cancel(
    broker_address: String,
    topic_filter: String,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    cancel_token: CancellationToken,
) -> Result<tokio::task::JoinHandle<()>, MqttBrokerAddressError> {
    let broker_address = parse_broker_address(&broker_address)?;
    Ok(start_mqtt_subscriber_with_config(
        MqttSubscriberConfig::new(broker_address, topic_filter, QoS::AtLeastOnce),
        sender,
        cancel_token,
    ))
}

pub(crate) fn start_mqtt_subscriber_with_config(
    config: MqttSubscriberConfig,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let forwarder = PayloadForwarder::new(sender);
        let mut backoff = Duration::from_secs(1);

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let client_id = format!("opcua-subscriber-{}", uuid::Uuid::new_v4());
            let mut options = MqttOptions::new(
                client_id,
                config.broker_address.host(),
                config.broker_address.port(),
            );
            options.set_keep_alive(Duration::from_secs(5));

            let (client, mut event_loop) = AsyncClient::new(options, 50);

            if let Err(error) = client
                .subscribe(config.topic_filter.clone(), config.qos)
                .await
            {
                tracing::warn!(
                    topic = %config.topic_filter,
                    ?error,
                    "failed to subscribe to MQTT topic filter"
                );
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    _ = sleep(backoff) => {}
                }
                backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                continue;
            }

            tracing::info!(topic = %config.topic_filter, "MQTT subscriber connected and subscribed");

            loop {
                let event = tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => return,
                    event = event_loop.poll() => event,
                };

                match event {
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        backoff = Duration::from_secs(1);
                        let payload = publish.payload.to_vec();
                        if let Err(error) = forwarder.forward(payload) {
                            match error {
                                TrySendError::Full(_) => {
                                    tracing::warn!(
                                        topic = %config.topic_filter,
                                        "MQTT subscriber datagram rejected; PubSub \
                                         datagram queue full (BadTooManyPublishRequests)"
                                    );
                                }
                                TrySendError::Closed(_) => {
                                    tracing::debug!(
                                        topic = %config.topic_filter,
                                        "subscriber channel closed; stopping"
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    Ok(_) => {
                        backoff = Duration::from_secs(1);
                    }
                    Err(error) => {
                        tracing::warn!(
                            topic = %config.topic_filter,
                            ?error,
                            "MQTT subscriber connection lost; reconnecting"
                        );
                        tokio::select! {
                            _ = cancel_token.cancelled() => return,
                            _ = sleep(backoff) => {}
                        }
                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests;
