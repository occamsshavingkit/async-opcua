use futures::stream::FuturesUnordered;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use super::supervise_transport;
use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, NodeType};
use opcua_types::{
    BinaryEncodable, ContextOwned, DataEncoding, NumericRange, StatusCode, TimestampsToReturn,
};

mod publisher;
mod subscriber;
#[cfg(test)]
mod tests;
pub(crate) use subscriber::{
    parse_broker_address, start_mqtt_subscriber_with_config, MqttBrokerAddress,
    MqttSubscriberConfig,
};
pub use subscriber::{
    quality_of_service, start_mqtt_subscriber, start_mqtt_subscriber_with_cancel,
    MqttBrokerAddressError,
};

use crate::{
    codec::json::{opcua_to_json_value, JsonDataSetMessage, JsonNetworkMessage},
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    MessageEncoding, PubSubConnectionConfig, PubSubPublisher,
};

/// Maximum number of messages to keep in the local cache when disconnected.
const MAX_CACHE_SIZE: usize = 1000;

/// Cache of pending (topic, payload) messages awaiting (re)publication.
type MessageCache = Arc<Mutex<VecDeque<(String, Vec<u8>)>>>;

fn lock_cache(cache: &MessageCache) -> std::sync::MutexGuard<'_, VecDeque<(String, Vec<u8>)>> {
    match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn push_cached_message(cache: &MessageCache, topic: String, payload: Vec<u8>) {
    let mut cache = lock_cache(cache);
    if cache.len() >= MAX_CACHE_SIZE {
        let _ = cache.pop_front();
    }
    cache.push_back((topic, payload));
}

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
        push_cached_message(&self.cache, topic, payload);
    }
}

impl PubSubPublisher for MqttPublisher {
    fn start_publishing(
        &self,
        connection_config: PubSubConnectionConfig,
        cancel_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, StatusCode> {
        let broker_address = parse_broker_address(&connection_config.address)
            .map_err(|error| error.status_code())?;

        let address_space = self.address_space.clone();
        let cache = self.cache.clone();
        let publisher_id = connection_config.connection_id.clone();

        let writer_groups = connection_config.writer_groups;

        let handle = tokio::spawn(async move {
            let writer_futures = FuturesUnordered::new();
            for writer_group in writer_groups {
                let address_space = address_space.clone();
                let publisher = cache.clone();
                let cancel_token = cancel_token.clone();
                let publisher_id = publisher_id.clone();

                writer_futures.push(async move {
                    let mut sequence_number: u16 = 0;
                    loop {
                        if cancel_token.is_cancelled() {
                            break;
                        }

                        tokio::select! {
                            _ = cancel_token.cancelled() => break,
                            _ = sleep(Duration::from_millis(writer_group.publishing_interval)) => {}
                        }

                        // Keep the address-space read guard scoped to traversal only. This block
                        // ends before the next loop iteration can await; the message vectors remain
                        // owned and usable while payloads are formatted and cached below.
                        let (json_dataset_messages, uadp_dataset_messages) = {
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
                                                if let Ok(val) =
                                                    opcua_to_json_value(&data_value, &ctx)
                                                {
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

                            (json_dataset_messages, uadp_dataset_messages)
                        };

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
                                    push_cached_message(&publisher, topic, json_str.into_bytes());
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
                                push_cached_message(&publisher, topic, payload);
                            }
                        }
                    }
                });
            }

            let mut publish_state = publisher::PublishTaskState::new(cache);
            let transport_loop =
                publisher::run_transport_loop(&cancel_token, &broker_address, &mut publish_state);

            supervise_transport(&cancel_token, transport_loop, writer_futures).await;
            publish_state.restore();
        });

        Ok(handle)
    }
}
