use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures::{stream::FuturesUnordered, SinkExt};
use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, NodeType};
use opcua_types::{
    BinaryEncodable, ContextOwned, DataEncoding, NumericRange, StatusCode, TimestampsToReturn,
};
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

use crate::{
    codec::json::{opcua_to_json_value, JsonDataSetMessage, JsonNetworkMessage},
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    transport::{supervise_transport, wait_for_reconnect},
    MessageEncoding, PubSubConnectionConfig, PubSubPublisher,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSocketAddressSettings {
    url: Url,
}

struct SanitizedWebSocketEndpoint<'a> {
    url: &'a Url,
}

impl fmt::Display for SanitizedWebSocketEndpoint<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}://", self.url.scheme())?;
        match self.url.host() {
            Some(Host::Domain(domain)) => formatter.write_str(domain)?,
            Some(Host::Ipv4(address)) => write!(formatter, "{address}")?,
            Some(Host::Ipv6(address)) => write!(formatter, "[{address}]")?,
            None => formatter.write_str("<unknown>")?,
        }
        if let Some(port) = self.url.port_or_known_default() {
            write!(formatter, ":{port}")?;
        }
        Ok(())
    }
}

impl WebSocketAddressSettings {
    fn sanitized_endpoint(&self) -> impl fmt::Display + '_ {
        SanitizedWebSocketEndpoint { url: &self.url }
    }
}

/// WebSocket implementation of `PubSubPublisher`.
pub struct WebSocketPublisher {
    address_space: Arc<RwLock<AddressSpace>>,
}

impl WebSocketPublisher {
    /// Creates a new `WebSocketPublisher` with the given AddressSpace reference.
    pub fn new(address_space: Arc<RwLock<AddressSpace>>) -> Self {
        Self { address_space }
    }

    /// Sends a single payload to a WebSocket endpoint without starting a cyclic publisher loop.
    pub fn publish_immediate(
        &self,
        payload: Vec<u8>,
        destination_address: &str,
        encoding: &MessageEncoding,
    ) {
        let destination_address = destination_address.to_string();
        let encoding = encoding.clone();

        tokio::spawn(async move {
            let settings = match parse_websocket_address(&destination_address) {
                Ok(settings) => settings,
                Err(error) => {
                    tracing::warn!(error = %error, "invalid WebSocket PubSub destination");
                    return;
                }
            };

            let Some(frame) = frame_for_payload(&encoding, payload) else {
                tracing::warn!("failed to encode WebSocket PubSub frame");
                return;
            };

            match connect_async(settings.url.as_str()).await {
                Ok((mut websocket, _)) => {
                    if let Err(error) = websocket.send(frame).await {
                        tracing::warn!(
                            websocket_endpoint = %settings.sanitized_endpoint(),
                            error = %error,
                            "failed to publish WebSocket payload"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        websocket_endpoint = %settings.sanitized_endpoint(),
                        error = %error,
                        "failed to connect WebSocket publisher"
                    );
                }
            }
        });
    }
}

impl PubSubPublisher for WebSocketPublisher {
    fn start_publishing(
        &self,
        connection_config: PubSubConnectionConfig,
        cancel_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, StatusCode> {
        let settings = parse_websocket_address(&connection_config.address)?;
        let address_space = self.address_space.clone();
        let publisher_id = connection_config.connection_id.clone();

        let handle = tokio::spawn(async move {
            let writer_futures = FuturesUnordered::new();

            for writer_group in connection_config.writer_groups {
                let address_space = address_space.clone();
                let cancel_token = cancel_token.clone();
                let publisher_id = publisher_id.clone();
                let settings = settings.clone();

                writer_futures.push(async move {
                    let mut sequence_number: u16 = 0;
                    let mut backoff = Duration::from_secs(1);

                    loop {
                        if cancel_token.is_cancelled() {
                            break;
                        }

                        let (mut websocket, _) =
                            match connect_async(settings.url.as_str()).await {
                            Ok(connection) => connection,
                            Err(error) => {
                                tracing::warn!(
                                    websocket_endpoint = %settings.sanitized_endpoint(),
                                    error = %error,
                                    "failed to connect WebSocket publisher"
                                );
                                wait_for_reconnect(&cancel_token, &mut backoff).await;
                                continue;
                            }
                        };

                        backoff = Duration::from_secs(1);

                        loop {
                            tokio::select! {
                                _ = cancel_token.cancelled() => {
                                    return;
                                }
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

                            let Some(payload) = payload else {
                                continue;
                            };

                            let Some(frame) = frame_for_payload(&writer_group.encoding, payload)
                            else {
                                tracing::warn!(
                                    writer_group_id = writer_group.writer_group_id,
                                    "failed to encode WebSocket JSON text frame"
                                );
                                continue;
                            };

                            if let Err(error) = websocket.send(frame).await {
                                tracing::warn!(
                                    websocket_endpoint = %settings.sanitized_endpoint(),
                                    error = %error,
                                    "failed to publish WebSocket payload"
                                );
                                wait_for_reconnect(&cancel_token, &mut backoff).await;
                                break;
                            }
                        }
                    }
                });
            }

            supervise_transport(&cancel_token, std::future::pending(), writer_futures).await;
        });

        Ok(handle)
    }
}

pub(crate) fn parse_websocket_address(
    address: &str,
) -> Result<WebSocketAddressSettings, StatusCode> {
    let address = address.trim();
    if address.is_empty() {
        return Err(StatusCode::BadInvalidArgument);
    }

    let url = if address
        .get(..6)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("wss://"))
    {
        format!("wss://{}", &address[6..])
    } else if address
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("ws://"))
    {
        format!("ws://{}", &address[5..])
    } else {
        format!("ws://{address}")
    };

    let url = Url::parse(&url).map_err(|_| StatusCode::BadInvalidArgument)?;

    Ok(WebSocketAddressSettings { url })
}

fn frame_for_payload(encoding: &MessageEncoding, payload: Vec<u8>) -> Option<Message> {
    match encoding {
        MessageEncoding::Json => String::from_utf8(payload).ok().map(Message::Text),
        MessageEncoding::Uadp => Some(Message::Binary(payload)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_websocket_address_with_prefix() {
        let settings = parse_websocket_address("ws://broker.local:9001/opcua").unwrap();

        assert_eq!(settings.url.as_str(), "ws://broker.local:9001/opcua");
    }

    #[test]
    fn parses_mixed_case_ws_scheme_as_lowercase() {
        let settings = parse_websocket_address("Ws://broker.local:9001/opcua").unwrap();

        assert_eq!(settings.url.as_str(), "ws://broker.local:9001/opcua");
    }

    #[test]
    fn parses_mixed_case_wss_scheme_as_lowercase() {
        let settings = parse_websocket_address("WSS://broker.local:9001/opcua").unwrap();

        assert_eq!(settings.url.as_str(), "wss://broker.local:9001/opcua");
    }

    #[test]
    fn parses_websocket_address_without_prefix_as_ws_url() {
        let settings = parse_websocket_address("broker.local:9001/opcua").unwrap();

        assert_eq!(settings.url.as_str(), "ws://broker.local:9001/opcua");
    }

    #[test]
    fn rejects_empty_websocket_address() {
        let error = parse_websocket_address("  ").unwrap_err();

        assert_eq!(error, StatusCode::BadInvalidArgument);
    }

    #[test]
    fn sends_json_payload_as_text_frame() {
        let frame = frame_for_payload(
            &MessageEncoding::Json,
            br#"{"MessageType":"ua-data"}"#.to_vec(),
        )
        .unwrap();

        assert_eq!(
            frame,
            Message::Text(r#"{"MessageType":"ua-data"}"#.to_string())
        );
    }

    #[test]
    fn sends_uadp_payload_as_binary_frame() {
        let frame = frame_for_payload(&MessageEncoding::Uadp, vec![1, 2, 3]).unwrap();

        assert_eq!(frame, Message::Binary(vec![1, 2, 3]));
    }
}
