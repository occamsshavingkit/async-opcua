use std::time::Duration;

use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::MqttDeliveryGuarantee;

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
    broker_address: String,
    topic_filter: String,
    qos: QoS,
}

impl MqttSubscriberConfig {
    pub(crate) fn new(broker_address: String, topic_filter: String, qos: QoS) -> Self {
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
pub fn start_mqtt_subscriber(
    broker_address: String,
    topic_filter: String,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    start_mqtt_subscriber_with_cancel(
        broker_address,
        topic_filter,
        sender,
        CancellationToken::new(),
    )
}

/// Starts an MQTT subscriber that exits when `cancel_token` is cancelled.
pub fn start_mqtt_subscriber_with_cancel(
    broker_address: String,
    topic_filter: String,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    start_mqtt_subscriber_with_config(
        MqttSubscriberConfig::new(broker_address, topic_filter, QoS::AtLeastOnce),
        sender,
        cancel_token,
    )
}

pub(crate) fn start_mqtt_subscriber_with_config(
    config: MqttSubscriberConfig,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let address = config
                .broker_address
                .strip_prefix("mqtt://")
                .unwrap_or(&config.broker_address);
            let mut address_parts = address.split(':');
            let host = address_parts.next().unwrap_or_default().to_string();
            let port = address_parts
                .next()
                .and_then(|port| port.parse::<u16>().ok())
                .unwrap_or(1883);

            let client_id = format!("opcua-subscriber-{}", uuid::Uuid::new_v4());
            let mut options = MqttOptions::new(client_id, host, port);
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
                    _ = cancel_token.cancelled() => return,
                    event = event_loop.poll() => event,
                };

                match event {
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        backoff = Duration::from_secs(1);
                        let payload = publish.payload.to_vec();
                        if let Err(error) = sender.try_send(payload) {
                            use tokio::sync::mpsc::error::TrySendError;
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
