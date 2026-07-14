use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, NodeType};
use opcua_types::{
    BinaryEncodable, ContextOwned, DataEncoding, NumericRange, StatusCode, TimestampsToReturn,
};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};

use crate::{
    codec::json::{opcua_to_json_value, JsonDataSetMessage, JsonNetworkMessage},
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    MessageEncoding, PubSubConnectionConfig, PubSubPublisher,
};

/// Maximum number of messages to keep in the local cache when disconnected.
const MAX_CACHE_SIZE: usize = 1000;

/// Cache of pending (topic, payload) messages awaiting (re)publication.
type MessageCache = Arc<Mutex<VecDeque<(String, Vec<u8>)>>>;

/// MQTT implementation of `PubSubPublisher` with reconnection, backoff, and local cache.
pub struct MqttPublisher {
    address_space: Arc<RwLock<AddressSpace>>,
    cache: MessageCache,
}

impl MqttPublisher {
    /// Creates a new `MqttPublisher` with the given AddressSpace reference.
    pub fn new(address_space: Arc<RwLock<AddressSpace>>) -> Self {
        Self {
            address_space,
            cache: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Instantly queues a message in the local bounded cache.
    pub fn publish_immediate(&self, topic: String, payload: Vec<u8>) {
        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= MAX_CACHE_SIZE {
            let _ = cache.pop_front();
        }
        cache.push_back((topic, payload));
    }
}

impl PubSubPublisher for MqttPublisher {
    fn start_publishing(
        &self,
        connection_config: PubSubConnectionConfig,
        cancel_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, StatusCode> {
        // Parse host and port from address
        let addr = connection_config
            .address
            .strip_prefix("mqtt://")
            .unwrap_or(&connection_config.address);
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0].to_string();
        let port = if parts.len() > 1 {
            parts[1].parse::<u16>().unwrap_or(1883)
        } else {
            1883
        };

        let address_space = self.address_space.clone();
        let cache = self.cache.clone();
        let publisher_id = connection_config.connection_id.clone();

        // 1. Spawn the cyclic publishing task(s)
        for writer_group in connection_config.writer_groups.clone() {
            let address_space = address_space.clone();
            let publisher = self.cache.clone();
            let cancel_token = cancel_token.clone();
            let publisher_id = publisher_id.clone();

            tokio::spawn(async move {
                let mut sequence_number: u16 = 0;
                loop {
                    if cancel_token.is_cancelled() {
                        break;
                    }

                    sleep(Duration::from_millis(writer_group.publishing_interval)).await;

                    // Query address space
                    let space = address_space.read();
                    let mut json_dataset_messages = Vec::new();
                    let mut uadp_dataset_messages = Vec::new();

                    for writer in &writer_group.dataset_writers {
                        let mut payload_map = std::collections::HashMap::new();
                        let mut uadp_fields = Vec::new();

                        for node_id in &writer.published_dataset.published_variables {
                            if let Some(node) = space.find(node_id) {
                                if let NodeType::Variable(ref var) = *node {
                                    // Use standard OPC UA getter
                                    let ctx_owned = ContextOwned::default();
                                    let ctx = ctx_owned.context();
                                    let data_value = var.value(
                                        TimestampsToReturn::Both,
                                        &NumericRange::None,
                                        &DataEncoding::Binary,
                                        0.0,
                                    );

                                    // For JSON
                                    if writer_group.encoding == MessageEncoding::Json {
                                        if let Ok(val) = opcua_to_json_value(&data_value, &ctx) {
                                            payload_map.insert(node_id.to_string(), val);
                                        }
                                    } else if let Some(ref val) = data_value.value {
                                        // For UADP
                                        uadp_fields.push(val.clone());
                                    }
                                }
                            }
                        }

                        sequence_number = sequence_number.wrapping_add(1);

                        match writer_group.encoding {
                            MessageEncoding::Json => {
                                json_dataset_messages.push(JsonDataSetMessage {
                                    dataset_writer_id: writer.dataset_writer_id,
                                    sequence_number,
                                    payload: payload_map,
                                });
                            }
                            MessageEncoding::Uadp => {
                                uadp_dataset_messages.push(UadpDataSetMessage {
                                    dataset_writer_id: writer.dataset_writer_id,
                                    sequence_number,
                                    timestamp: Some(opcua_types::DateTime::now()),
                                    status: Some(StatusCode::Good),
                                    fields: uadp_fields,
                                });
                            }
                        }
                    }

                    // Format and queue payload
                    let topic = format!("opcua/telemetry/{}", writer_group.writer_group_id);
                    match writer_group.encoding {
                        MessageEncoding::Json => {
                            let msg = JsonNetworkMessage {
                                message_id: uuid::Uuid::new_v4().to_string(),
                                message_type: "ua-data".to_string(),
                                publisher_id: publisher_id.clone(),
                                writer_group_id: writer_group.writer_group_id,
                                messages: json_dataset_messages,
                            };
                            if let Ok(json_str) = msg.to_json_string() {
                                let mut cache = publisher.lock().unwrap();
                                if cache.len() >= MAX_CACHE_SIZE {
                                    let _ = cache.pop_front();
                                }
                                cache.push_back((topic, json_str.into_bytes()));
                            }
                        }
                        MessageEncoding::Uadp => {
                            let msg = UadpNetworkMessage {
                                publisher_id: PublisherId::String(publisher_id.clone()),
                                writer_group_id: writer_group.writer_group_id,
                                network_message_number: 0,
                                sequence_number,
                                dataset_messages: uadp_dataset_messages,
                            };
                            let ctx_owned = ContextOwned::default();
                            let ctx = ctx_owned.context();
                            let payload = msg.encode_to_vec(&ctx);
                            let mut cache = publisher.lock().unwrap();
                            if cache.len() >= MAX_CACHE_SIZE {
                                let _ = cache.pop_front();
                            }
                            cache.push_back((topic, payload));
                        }
                    }
                }
            });
        }

        // 2. Spawn the MQTT connection and sender loop with backoff
        let handle = tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);

            loop {
                if cancel_token.is_cancelled() {
                    break;
                }

                let client_id = format!("opcua-publisher-{}", uuid::Uuid::new_v4());
                let mut options = MqttOptions::new(client_id, host.clone(), port);
                options.set_keep_alive(Duration::from_secs(5));

                let (client, mut event_loop) = AsyncClient::new(options, 50);

                // Background loop draining the cache and polling MQTT
                loop {
                    if cancel_token.is_cancelled() {
                        return;
                    }

                    // Attempt to publish one item from cache
                    let mut next_item = None;
                    {
                        let mut cache_lock = cache.lock().unwrap();
                        if let Some((topic, payload)) = cache_lock.pop_front() {
                            next_item = Some((topic, payload));
                        }
                    }

                    if let Some((topic, payload)) = next_item {
                        if client
                            .publish(topic.clone(), QoS::AtLeastOnce, false, payload.clone())
                            .await
                            .is_err()
                        {
                            // Put it back at the front and break to reconnect
                            {
                                let mut cache_lock = cache.lock().unwrap();
                                cache_lock.push_front((topic, payload));
                            }
                            sleep(backoff).await;
                            backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                            break;
                        }
                        // Success: continue draining cache immediately without polling event loop
                        continue;
                    }

                    // Cache is empty, poll the event loop to keep connection alive
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            return;
                        }
                        res = event_loop.poll() => {
                            match res {
                                Ok(_) => {
                                    // Successful communication, reset backoff
                                    backoff = Duration::from_secs(1);
                                }
                                Err(_) => {
                                    // Connection lost, sleep and reconnect
                                    sleep(backoff).await;
                                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                                    break;
                                }
                            }
                        }
                        _ = sleep(Duration::from_millis(20)) => {
                            // Wake up to check cache again
                        }
                    }
                }
            }
        });

        Ok(handle)
    }
}

/// Starts an MQTT subscriber implementing the Broker DataSetReader transport
/// (OPC-10000-14 §6.4.2.6).
///
/// `broker_address` is the `mqtt://host:port` (or bare `host:port`) of the
/// broker, `topic_filter` is the broker QueueName (§6.4.2.6.1) to subscribe to,
/// and received payload bytes are forwarded to `sender`.
///
/// Forwarding uses a non-blocking `try_send`; when `sender` is a bounded
/// channel that is full, the payload is rejected and logged as
/// `BadTooManyPublishRequests` (OPC-10000-14 §9.1.10.1) rather than blocking
/// the broker poll loop. A closed receiver stops the subscriber.
///
/// The subscriber runs as a background tokio task; the returned `JoinHandle`
/// lets the caller await completion or abort. Reconnects with exponential
/// backoff (capped at 60s) on connection loss or subscribe failure. The MQTT
/// QoS defaults to `AtLeastOnce`, matching `MqttPublisher`'s delivery guarantee
/// and corresponding to the DataSetReader's RequestedDeliveryGuarantee
/// (§6.4.2.6.4).
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
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            // Parse host and port from address (mirrors MqttPublisher).
            let addr = broker_address
                .strip_prefix("mqtt://")
                .unwrap_or(&broker_address);
            let parts: Vec<&str> = addr.split(':').collect();
            let host = parts[0].to_string();
            let port = if parts.len() > 1 {
                parts[1].parse::<u16>().unwrap_or(1883)
            } else {
                1883
            };

            let client_id = format!("opcua-subscriber-{}", uuid::Uuid::new_v4());
            let mut options = MqttOptions::new(client_id, host, port);
            options.set_keep_alive(Duration::from_secs(5));

            let (client, mut event_loop) = AsyncClient::new(options, 50);

            // Subscribe to the broker QueueName (§6.4.2.6.1). QoS maps to the
            // DataSetReader's RequestedDeliveryGuarantee (§6.4.2.6.4).
            if let Err(error) = client
                .subscribe(topic_filter.clone(), QoS::AtLeastOnce)
                .await
            {
                tracing::warn!(
                    topic = %topic_filter,
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

            tracing::info!(topic = %topic_filter, "MQTT subscriber connected and subscribed");

            loop {
                let event = tokio::select! {
                    _ = cancel_token.cancelled() => return,
                    event = event_loop.poll() => event,
                };

                match event {
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        backoff = Duration::from_secs(1);
                        let payload = publish.payload.to_vec();
                        // Bounded `try_send` honours OPC-10000-14 §9.1.10.1:
                        // a full datagram queue rejects the payload with the
                        // equivalent of `BadTooManyPublishRequests` rather
                        // than blocking the broker poll loop or growing
                        // memory without bound.
                        if let Err(err) = sender.try_send(payload) {
                            use tokio::sync::mpsc::error::TrySendError;
                            match err {
                                TrySendError::Full(_) => {
                                    tracing::warn!(
                                        topic = %topic_filter,
                                        "MQTT subscriber datagram rejected; PubSub \
                                         datagram queue full (BadTooManyPublishRequests)"
                                    );
                                }
                                TrySendError::Closed(_) => {
                                    tracing::debug!(
                                        topic = %topic_filter,
                                        "subscriber channel closed; stopping"
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    Ok(_) => {
                        // Other MQTT packets (CONNACK, PINGRESP, etc.); connection is healthy.
                        backoff = Duration::from_secs(1);
                    }
                    Err(error) => {
                        tracing::warn!(
                            topic = %topic_filter,
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
