use futures::stream::FuturesUnordered;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, NodeType};
use opcua_types::{
    BinaryEncodable, ContextOwned, DataEncoding, NumericRange, StatusCode, TimestampsToReturn,
};

use crate::{
    codec::json::{JsonDataSetMessage, JsonNetworkMessage},
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    transport::supervise_transport,
    MessageEncoding, PubSubConnectionConfig, PubSubPublisher,
};

/// Maximum transmission unit for a single UDP packet to avoid IP-level fragmentation.
const MTU: usize = 1400;
const UDP_SCHEMES: [&str; 2] = ["opc.udp://", "udp://"];

/// Strips the OPC-10000-14 §§7.3.2.2-7.3.2.3 UDP scheme or its legacy alias.
pub(crate) fn strip_udp_scheme(address: &str) -> Option<&str> {
    let address = address.trim();
    UDP_SCHEMES.into_iter().find_map(|scheme| {
        address
            .get(..scheme.len())
            .filter(|prefix| prefix.eq_ignore_ascii_case(scheme))
            .map(|_| &address[scheme.len()..])
    })
}

pub(crate) fn parse_udp_destination(address: &str) -> Result<SocketAddr, StatusCode> {
    let address = address.trim();
    strip_udp_scheme(address)
        .unwrap_or(address)
        .parse::<SocketAddr>()
        .map_err(|_| StatusCode::BadConfigurationError)
}

fn bind_publisher_socket(bind_addr: SocketAddr) -> Result<UdpSocket, StatusCode> {
    let socket =
        std::net::UdpSocket::bind(bind_addr).map_err(|_| StatusCode::BadCommunicationError)?;
    socket
        .set_nonblocking(true)
        .map_err(|_| StatusCode::BadCommunicationError)?;
    UdpSocket::from_std(socket).map_err(|_| StatusCode::BadCommunicationError)
}

/// Parsed UDP subscriber bind endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpSubscriberEndpoint {
    /// Socket address used to bind the subscriber.
    pub bind_addr: SocketAddr,
    /// Multicast group to join when the target address is multicast.
    pub multicast_addr: Option<Ipv4Addr>,
}

impl UdpSubscriberEndpoint {
    /// Parses an OPC UA PubSub UDP URL such as `opc.udp://239.0.0.1:4840`.
    pub fn parse(address: &str) -> Result<Self, StatusCode> {
        let socket_addr = parse_udp_destination(address)?;

        match socket_addr.ip() {
            IpAddr::V4(ip) if ip.is_multicast() => Ok(Self {
                bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), socket_addr.port()),
                multicast_addr: Some(ip),
            }),
            IpAddr::V4(_) => Ok(Self {
                bind_addr: socket_addr,
                multicast_addr: None,
            }),
            IpAddr::V6(_) => Err(StatusCode::BadNotSupported),
        }
    }
}

/// Binds a UDP socket for subscriber receive.
pub async fn bind_subscriber_socket(
    endpoint: UdpSubscriberEndpoint,
) -> Result<UdpSocket, StatusCode> {
    let socket = UdpSocket::bind(endpoint.bind_addr)
        .await
        .map_err(|_| StatusCode::BadCommunicationError)?;

    if let Some(multicast_addr) = endpoint.multicast_addr {
        socket
            .join_multicast_v4(multicast_addr, Ipv4Addr::UNSPECIFIED)
            .map_err(|_| StatusCode::BadCommunicationError)?;
    }

    Ok(socket)
}

/// Returns true for the crate's legacy custom UDP fragmentation header.
#[must_use]
pub fn is_custom_fragment_datagram(payload: &[u8]) -> bool {
    payload.len() >= 7 && (payload[0] & 0x0f) != 1 && (payload[6] & 0x0f) == 1
}

/// UDP Multicast implementation of `PubSubPublisher` with datagram fragmentation.
pub struct UdpPublisher {
    address_space: Arc<RwLock<AddressSpace>>,
}

impl UdpPublisher {
    /// Creates a new `UdpPublisher` with the given AddressSpace reference.
    pub fn new(address_space: Arc<RwLock<AddressSpace>>) -> Self {
        Self { address_space }
    }

    /// Instantly sends a payload to the destination address.
    pub async fn publish_immediate(&self, payload: Vec<u8>, destination_address: &str) {
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
            let _ = socket.send_to(&payload, destination_address).await;
        }
    }
}

impl PubSubPublisher for UdpPublisher {
    fn start_publishing(
        &self,
        connection_config: PubSubConnectionConfig,
        cancel_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, StatusCode> {
        let destination_address = parse_udp_destination(&connection_config.address)?;

        let address_space = self.address_space.clone();
        let publisher_id = connection_config.connection_id.clone();
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        let prepared_sockets = connection_config
            .writer_groups
            .iter()
            .map(|_| {
                let socket = bind_publisher_socket(bind_addr)?;
                socket
                    .set_multicast_loop_v4(true)
                    .map_err(|_| StatusCode::BadCommunicationError)?;
                socket
                    .set_multicast_ttl_v4(32)
                    .map_err(|_| StatusCode::BadCommunicationError)?;
                Ok(Arc::new(socket))
            })
            .collect::<Result<Vec<_>, StatusCode>>()?;

        // Spawn a coordinator task that manages the individual writer group loops
        let handle = tokio::spawn(async move {
            let writer_futures = FuturesUnordered::new();
            for (writer_group, socket) in connection_config
                .writer_groups
                .into_iter()
                .zip(prepared_sockets)
            {
                let address_space = address_space.clone();
                let cancel_token = cancel_token.clone();
                let destination_address = destination_address;
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

                        // Query address space inside a block: the guard
                        // must not be in scope across an await.
                        let mut json_dataset_messages = Vec::new();
                        let mut uadp_dataset_messages = Vec::new();
                        {
                            let space = address_space.read();

                            for writer in &writer_group.dataset_writers {
                                let mut payload_map = std::collections::HashMap::new();
                                let mut uadp_fields = Vec::new();

                                for node_id in &writer.published_dataset.published_variables {
                                    if let Some(node) = space.find(node_id) {
                                        if let NodeType::Variable(ref var) = *node {
                                            let ctx_owned = ContextOwned::default();
                                            let ctx = ctx_owned.context();
                                            let data_value = var.value(
                                                TimestampsToReturn::Both,
                                                &NumericRange::None,
                                                &DataEncoding::Binary,
                                                0.0,
                                            );

                                            if writer_group.encoding == MessageEncoding::Json {
                                                if let Ok(val) =
                                                    opcua_to_json_value(&data_value, &ctx)
                                                {
                                                    payload_map.insert(node_id.to_string(), val);
                                                }
                                            } else if let Some(ref val) = data_value.value {
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
                        }

                        // Format payload
                        let payload = match writer_group.encoding {
                            MessageEncoding::Json => {
                                let msg = JsonNetworkMessage {
                                    message_id: uuid::Uuid::new_v4().to_string(),
                                    message_type: "ua-data".to_string(),
                                    publisher_id: publisher_id.clone(),
                                    writer_group_id: writer_group.writer_group_id,
                                    messages: json_dataset_messages,
                                };
                                msg.to_json_string().ok().map(|s| s.into_bytes())
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
                                Some(msg.encode_to_vec(&ctx))
                            }
                        };

                        if let Some(payload) = payload {
                            // Send payload with datagram fragmentation if size > MTU
                            if payload.len() <= MTU {
                                let _ = socket.send_to(&payload, destination_address).await;
                            } else {
                                let total_fragments = payload.len().div_ceil(MTU) as u8;
                                for fragment_index in 0..total_fragments {
                                    let start = fragment_index as usize * MTU;
                                    let end = std::cmp::min(start + MTU, payload.len());
                                    let chunk = &payload[start..end];

                                    // Fragment header: sequence_number (2b), total_fragments (1b), fragment_index (1b), chunk_size (2b)
                                    let mut packet = Vec::with_capacity(6 + chunk.len());
                                    packet.extend_from_slice(&sequence_number.to_be_bytes());
                                    packet.push(total_fragments);
                                    packet.push(fragment_index);
                                    packet.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
                                    packet.extend_from_slice(chunk);

                                    let _ = socket.send_to(&packet, destination_address).await;
                                }
                            }
                        }
                    }
                });
            }

            supervise_transport(&cancel_token, std::future::pending::<()>(), writer_futures).await;
        });

        Ok(handle)
    }
}

/// Helper function to convert an OPC-UA `JsonEncodable` type to a `serde_json::Value`.
fn opcua_to_json_value<T: opcua_types::json::JsonEncodable>(
    value: &T,
    ctx: &opcua_types::Context<'_>,
) -> Result<serde_json::Value, opcua_types::Error> {
    let json_str = opcua_types::json::to_string(value, ctx)?;
    let val =
        serde_json::from_str(&json_str).map_err(|e| opcua_types::Error::decoding(e.to_string()))?;
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DataSetWriterConfig, PublishedDataSetConfig, WriterGroupConfig};

    #[tokio::test]
    async fn start_publishing_rejects_malformed_destination_before_spawn() {
        // Given: a UDP publisher configured with a malformed socket address.
        let publisher = UdpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
        let config = PubSubConnectionConfig {
            connection_id: "malformed-udp".to_string(),
            name: "malformed-udp".to_string(),
            address: "udp://not-a-socket-address".to_string(),
            writer_groups: Vec::new(),
            reader_groups: Vec::new(),
        };

        // When: publishing is started directly.
        let result = publisher.start_publishing(config, CancellationToken::new());

        // Then: malformed configuration is rejected instead of returning a spawned handle.
        match result {
            Err(status) => assert_eq!(status, StatusCode::BadConfigurationError),
            Ok(handle) => {
                handle.abort();
                panic!("malformed UDP destination returned a publisher handle");
            }
        }
    }

    #[tokio::test]
    async fn aborting_coordinator_stops_writer_group_future() {
        let receiver = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("loopback receiver binds");
        let destination = receiver
            .local_addr()
            .expect("loopback receiver has an address");
        let publisher = UdpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
        let cancel_token = CancellationToken::new();
        let config = PubSubConnectionConfig {
            connection_id: "udp-regression".to_string(),
            name: "udp-regression".to_string(),
            address: format!("udp://{destination}"),
            writer_groups: vec![WriterGroupConfig {
                writer_group_id: 1,
                publishing_interval: 10,
                encoding: MessageEncoding::Json,
                dataset_writers: vec![DataSetWriterConfig {
                    dataset_writer_id: 1,
                    dataset_name: "empty".to_string(),
                    published_dataset: PublishedDataSetConfig {
                        published_variables: Vec::new(),
                        configuration_version: Default::default(),
                    },
                }],
            }],
            reader_groups: Vec::new(),
        };

        let coordinator = publisher
            .start_publishing(config, cancel_token.clone())
            .expect("publisher starts");
        let mut packet = [0u8; 2048];
        tokio::time::timeout(Duration::from_secs(1), receiver.recv_from(&mut packet))
            .await
            .expect("writer emits a datagram before the deadline")
            .expect("writer datagram is received");

        coordinator.abort();
        assert!(coordinator
            .await
            .expect_err("coordinator was aborted")
            .is_cancelled());
        sleep(Duration::from_millis(30)).await;

        while receiver.try_recv_from(&mut packet).is_ok() {}

        let continued_packet =
            tokio::time::timeout(Duration::from_millis(50), receiver.recv_from(&mut packet)).await;
        assert!(
            continued_packet.is_err(),
            "writer group delivered a datagram after coordinator abort"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn aborting_coordinator_drops_writer_future_before_returning() {
        // Given: a coordinator that has taken direct ownership of one writer future.
        let publisher = UdpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
        let config = PubSubConnectionConfig {
            connection_id: "udp-abort-drop-regression".to_string(),
            name: "UDP abort drop regression".to_string(),
            address: "udp://127.0.0.1:9".to_string(),
            writer_groups: vec![WriterGroupConfig {
                writer_group_id: 1,
                publishing_interval: 60_000,
                encoding: MessageEncoding::Json,
                dataset_writers: Vec::new(),
            }],
            reader_groups: Vec::new(),
        };
        let coordinator = publisher
            .start_publishing(config, CancellationToken::new())
            .expect("UDP publisher should start");
        tokio::time::timeout(Duration::from_secs(1), async {
            while Arc::strong_count(&publisher.address_space) < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("UDP coordinator did not take ownership of the writer future");

        // When: the coordinator is aborted and awaited to completion.
        let observed_address_space = Arc::clone(&publisher.address_space);
        let observed_strong_count = tokio::spawn(async move {
            coordinator.abort();
            let join_error = coordinator
                .await
                .expect_err("coordinator completed instead of being aborted");
            assert!(join_error.is_cancelled(), "coordinator was not cancelled");
            Arc::strong_count(&observed_address_space)
        })
        .await
        .expect("ownership observer task failed");

        // Then: no writer future retains the publisher's address space.
        assert_eq!(
            observed_strong_count, 2,
            "UDP writer future remained alive after the aborted coordinator returned"
        );
    }

    #[test]
    fn subscriber_endpoint_parse_strips_mixed_case_udp_scheme() {
        let endpoint = UdpSubscriberEndpoint::parse("  UdP://127.0.0.1:4840  ")
            .expect("mixed-case UDP scheme should parse");

        assert_eq!(endpoint.bind_addr, "127.0.0.1:4840".parse().unwrap());
        assert_eq!(endpoint.multicast_addr, None);
    }

    #[test]
    fn publisher_destination_parse_strips_mixed_case_udp_scheme() {
        let destination = parse_udp_destination("UdP://239.0.0.1:4840")
            .expect("mixed-case UDP scheme should parse");

        assert_eq!(destination, "239.0.0.1:4840".parse().unwrap());
    }

    #[test]
    fn subscriber_endpoint_parse_accepts_standard_mixed_case_opc_udp_scheme() {
        // Given: the UDP URI scheme required by OPC-10000-14 §§7.3.2.2-7.3.2.3.
        let address = "  OpC.UdP://127.0.0.1:4840  ";

        // When: the subscriber endpoint is parsed.
        let endpoint = UdpSubscriberEndpoint::parse(address)
            .expect("standards-compliant OPC UDP scheme should parse");

        // Then: the socket address is normalized without changing unicast behavior.
        assert_eq!(endpoint.bind_addr, "127.0.0.1:4840".parse().unwrap());
        assert_eq!(endpoint.multicast_addr, None);
    }

    #[test]
    fn publisher_destination_parse_accepts_standard_mixed_case_opc_udp_scheme() {
        // Given: a standards-compliant mixed-case OPC UDP destination.
        let address = "OpC.UdP://239.0.0.1:4840";

        // When: publisher preflight parses the destination.
        let destination = parse_udp_destination(address)
            .expect("standards-compliant OPC UDP scheme should parse");

        // Then: the multicast socket address is preserved.
        assert_eq!(destination, "239.0.0.1:4840".parse().unwrap());
    }

    #[test]
    fn subscriber_endpoint_maps_malformed_socket_address_to_configuration_error() {
        // Given: a recognized UDP scheme with an invalid socket address.
        let address = "opc.udp://127.0.0.1:not-a-port";

        // When: subscriber preflight parses the endpoint.
        let result = UdpSubscriberEndpoint::parse(address);

        // Then: malformed configuration uses the common transport configuration status.
        assert_eq!(result, Err(StatusCode::BadConfigurationError));
    }

    #[test]
    fn subscriber_endpoint_keeps_ipv6_unsupported_for_standard_opc_udp_scheme() {
        // Given: a syntactically valid IPv6 OPC UDP endpoint.
        let address = "opc.udp://[::1]:4840";

        // When: subscriber preflight parses the endpoint.
        let result = UdpSubscriberEndpoint::parse(address);

        // Then: the existing IPv6 support boundary remains unchanged.
        assert_eq!(result, Err(StatusCode::BadNotSupported));
    }

    #[test]
    fn publisher_socket_preparation_reports_bind_failure_synchronously() {
        // OPC-10000-14 transport startup must surface publisher bind failures before spawning.
        let occupied_socket = std::net::UdpSocket::bind("127.0.0.1:0")
            .expect("occupied publisher address should bind");
        let occupied_addr = occupied_socket
            .local_addr()
            .expect("occupied publisher socket should have an address");

        let result = bind_publisher_socket(occupied_addr);

        assert!(matches!(result, Err(StatusCode::BadCommunicationError)));
    }

    #[tokio::test]
    async fn graceful_cancellation_stops_datagram_production() {
        let receiver = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("loopback receiver binds");
        let destination = receiver
            .local_addr()
            .expect("loopback receiver has an address");
        let publisher = UdpPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
        let cancel_token = CancellationToken::new();
        let config = PubSubConnectionConfig {
            connection_id: "udp-graceful-shutdown".to_string(),
            name: "udp-graceful-shutdown".to_string(),
            address: format!("udp://{destination}"),
            writer_groups: vec![WriterGroupConfig {
                writer_group_id: 1,
                publishing_interval: 10,
                encoding: MessageEncoding::Json,
                dataset_writers: vec![DataSetWriterConfig {
                    dataset_writer_id: 1,
                    dataset_name: "empty".to_string(),
                    published_dataset: PublishedDataSetConfig {
                        published_variables: Vec::new(),
                        configuration_version: Default::default(),
                    },
                }],
            }],
            reader_groups: Vec::new(),
        };

        let coordinator = publisher
            .start_publishing(config, cancel_token.clone())
            .expect("publisher starts");
        let mut packet = [0u8; 2048];
        tokio::time::timeout(Duration::from_secs(1), receiver.recv_from(&mut packet))
            .await
            .expect("writer emits a datagram before the deadline")
            .expect("writer datagram is received");

        cancel_token.cancel();
        tokio::time::timeout(Duration::from_secs(2), coordinator)
            .await
            .expect("coordinator should stop before the deadline")
            .expect("coordinator should shut down successfully");

        while receiver.try_recv_from(&mut packet).is_ok() {}
        let continued_packet =
            tokio::time::timeout(Duration::from_millis(50), receiver.recv_from(&mut packet)).await;
        assert!(
            continued_packet.is_err(),
            "writer group delivered a datagram after graceful cancellation"
        );
    }
}
