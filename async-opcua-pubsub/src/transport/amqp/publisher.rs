use std::time::Duration;

use futures::stream::FuturesUnordered;
use lapin::{
    options::{BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use opcua_server::address_space::{AddressSpace, NodeType};
use opcua_types::{
    BinaryEncodable, ContextOwned, DataEncoding, NumericRange, StatusCode, TimestampsToReturn,
};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber;

use crate::{
    codec::json::{opcua_to_json_value, JsonDataSetMessage, JsonNetworkMessage},
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    transport::{supervise_transport, wait_for_reconnect},
    MessageEncoding, PubSubConnectionConfig, PubSubPublisher,
};

use super::{lock_cache, parse_amqp_address, push_cached_message, AmqpPublisher, DEFAULT_EXCHANGE};

impl PubSubPublisher for AmqpPublisher {
    fn start_publishing(
        &self,
        connection_config: PubSubConnectionConfig,
        cancel_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, StatusCode> {
        let settings = parse_amqp_address(&connection_config.address)?;
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
                let routing_key = settings.routing_key.clone();

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

                        let payload = {
                            let space = address_space.read();
                            build_writer_group_payload(
                                &space,
                                &writer_group,
                                &publisher_id,
                                &mut sequence_number,
                            )
                        };

                        if let Some(payload) = payload {
                            push_cached_message(&publisher, routing_key.clone(), payload);
                        }
                    }
                });
            }

            let transport_loop = async {
                let mut backoff = Duration::from_secs(1);
                loop {
                    if cancel_token.is_cancelled() {
                        break;
                    }

                    let connection = match Connection::connect(
                        &settings.broker_url,
                        ConnectionProperties::default(),
                    )
                    .await
                    {
                        Ok(connection) => connection,
                        Err(error) => {
                            tracing::warn!(
                                broker_endpoint = %settings.sanitized_endpoint(),
                                error = %error,
                                "failed to connect AMQP publisher"
                            );
                            wait_for_reconnect(&cancel_token, &mut backoff).await;
                            continue;
                        }
                    };

                    let channel = match connection.create_channel().await {
                        Ok(channel) => channel,
                        Err(error) => {
                            tracing::warn!(?error, "failed to create AMQP channel");
                            wait_for_reconnect(&cancel_token, &mut backoff).await;
                            continue;
                        }
                    };

                    if let Err(error) = channel
                        .queue_declare(
                            &settings.routing_key,
                            QueueDeclareOptions::default(),
                            FieldTable::default(),
                        )
                        .await
                    {
                        tracing::warn!(
                            routing_key = %settings.routing_key,
                            ?error,
                            "failed to declare AMQP queue"
                        );
                        wait_for_reconnect(&cancel_token, &mut backoff).await;
                        continue;
                    }

                    backoff = Duration::from_secs(1);

                    loop {
                        if cancel_token.is_cancelled() {
                            return;
                        }

                        let next_item = lock_cache(&cache).pop_front();

                        if let Some((routing_key, payload)) = next_item {
                            if let Err(error) =
                                publish_payload(&channel, &routing_key, &payload).await
                            {
                                tracing::warn!(routing_key = %routing_key, ?error, "failed to publish AMQP payload");
                                lock_cache(&cache).push_front((routing_key, payload));
                                wait_for_reconnect(&cancel_token, &mut backoff).await;
                                break;
                            }
                            continue;
                        }

                        tokio::select! {
                            _ = cancel_token.cancelled() => {
                                return;
                            }
                            _ = sleep(Duration::from_millis(20)) => {}
                        }
                    }
                }
            };

            supervise_transport(&cancel_token, transport_loop, writer_futures).await;
        }
        .with_current_subscriber());

        Ok(handle)
    }
}

fn build_writer_group_payload(
    space: &AddressSpace,
    writer_group: &crate::WriterGroupConfig,
    publisher_id: &str,
    sequence_number: &mut u16,
) -> Option<Vec<u8>> {
    let mut json_dataset_messages = Vec::new();
    let mut uadp_dataset_messages = Vec::new();

    for writer in &writer_group.dataset_writers {
        let mut payload_map = std::collections::HashMap::new();
        let mut uadp_fields = Vec::new();

        for node_id in &writer.published_dataset.published_variables {
            if let Some(NodeType::Variable(var)) = space.find(node_id).as_deref() {
                let ctx_owned = ContextOwned::default();
                let ctx = ctx_owned.context();
                let data_value = var.value(
                    TimestampsToReturn::Both,
                    &NumericRange::None,
                    &DataEncoding::Binary,
                    0.0,
                );

                if writer_group.encoding == MessageEncoding::Json {
                    if let Ok(val) = opcua_to_json_value(&data_value, &ctx) {
                        payload_map.insert(node_id.to_string(), val);
                    }
                } else if let Some(ref val) = data_value.value {
                    uadp_fields.push(val.clone());
                }
            }
        }

        *sequence_number = sequence_number.wrapping_add(1);

        match writer_group.encoding {
            MessageEncoding::Json => {
                json_dataset_messages.push(JsonDataSetMessage {
                    dataset_writer_id: writer.dataset_writer_id,
                    sequence_number: *sequence_number,
                    payload: payload_map,
                });
            }
            MessageEncoding::Uadp => {
                uadp_dataset_messages.push(UadpDataSetMessage {
                    dataset_writer_id: writer.dataset_writer_id,
                    sequence_number: *sequence_number,
                    timestamp: Some(opcua_types::DateTime::now()),
                    status: Some(StatusCode::Good),
                    fields: uadp_fields,
                });
            }
        }
    }

    match writer_group.encoding {
        MessageEncoding::Json => {
            let msg = JsonNetworkMessage {
                message_id: uuid::Uuid::new_v4().to_string(),
                message_type: "ua-data".to_string(),
                publisher_id: publisher_id.to_string(),
                writer_group_id: writer_group.writer_group_id,
                messages: json_dataset_messages,
            };
            msg.to_json_string().ok().map(String::into_bytes)
        }
        MessageEncoding::Uadp => {
            let msg = UadpNetworkMessage {
                publisher_id: PublisherId::String(publisher_id.to_string()),
                writer_group_id: writer_group.writer_group_id,
                network_message_number: 0,
                sequence_number: *sequence_number,
                dataset_messages: uadp_dataset_messages,
            };
            let ctx_owned = ContextOwned::default();
            let ctx = ctx_owned.context();
            Some(msg.encode_to_vec(&ctx))
        }
    }
}

async fn publish_payload(
    channel: &Channel,
    routing_key: &str,
    payload: &[u8],
) -> lapin::Result<()> {
    let confirmation = channel
        .basic_publish(
            DEFAULT_EXCHANGE,
            routing_key,
            BasicPublishOptions::default(),
            payload,
            BasicProperties::default(),
        )
        .await?;
    let _ = confirmation.await?;
    Ok(())
}
