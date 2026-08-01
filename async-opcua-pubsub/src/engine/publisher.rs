use opcua_types::StatusCode;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::TransportKind;
#[cfg(feature = "tsn")]
use crate::transport::tsn::publisher::parse_tsn_interface;
use crate::{
    transport::{
        amqp::{parse_amqp_address, AmqpPublisher},
        mqtt::{parse_broker_address, MqttPublisher},
        udp::{parse_udp_destination, UdpPublisher},
        websocket::{parse_websocket_address, WebSocketPublisher},
    },
    PubSubConnectionConfig, PubSubPublisher,
};

use super::PubSubEngine;

impl PubSubEngine {
    /// Returns true when the engine has started publisher loops.
    pub fn is_running(&self) -> bool {
        self.cancel_token.is_some()
    }

    /// Returns the number of active publisher coordinator handles.
    pub fn active_handle_count(&self) -> usize {
        self.publisher_handles.len()
    }

    /// Starts transport publisher loops for all configured connections.
    pub fn start(&mut self) -> Result<(), StatusCode> {
        if self.is_running() {
            return Ok(());
        }

        let cancel_token = CancellationToken::new();
        let connections = self
            .connections
            .iter()
            .cloned()
            .map(|connection| {
                let transport = self.preflight_connection(&connection)?;
                Ok((connection, transport))
            })
            .collect::<Result<Vec<_>, StatusCode>>()?;
        let mut handles = Vec::with_capacity(self.connections.len());

        for (connection, transport) in connections {
            match self.start_connection(connection, transport, cancel_token.clone()) {
                Ok(handle) => handles.push(handle),
                Err(status) => {
                    cancel_token.cancel();
                    for handle in handles {
                        handle.abort();
                    }
                    return Err(status);
                }
            }
        }

        self.cancel_token = Some(cancel_token);
        self.publisher_handles = handles;
        Ok(())
    }

    /// Stops all active publisher loops and waits for their coordinator tasks to finish.
    pub async fn stop(&mut self) {
        self.stop_subscribers().await;

        if let Some(cancel_token) = self.cancel_token.take() {
            cancel_token.cancel();
        }

        while let Some(handle) = self.publisher_handles.pop() {
            let _ = handle.await;
        }
    }

    fn start_connection(
        &self,
        connection: PubSubConnectionConfig,
        transport: TransportKind,
        cancel_token: CancellationToken,
    ) -> Result<JoinHandle<()>, StatusCode> {
        match transport {
            TransportKind::Mqtt => MqttPublisher::new(self.address_space.clone())
                .start_publishing(connection, cancel_token),
            TransportKind::Udp => UdpPublisher::new(self.address_space.clone())
                .start_publishing(connection, cancel_token),
            #[cfg(feature = "tsn")]
            TransportKind::Tsn => {
                crate::transport::tsn::publisher::TsnPublisher::new(self.address_space.clone())
                    .start_publishing(connection, cancel_token)
            }
            TransportKind::Amqp => AmqpPublisher::new(self.address_space.clone())
                .start_publishing(connection, cancel_token),
            TransportKind::WebSocket => WebSocketPublisher::new(self.address_space.clone())
                .start_publishing(connection, cancel_token),
        }
    }

    fn preflight_connection(
        &self,
        connection: &PubSubConnectionConfig,
    ) -> Result<TransportKind, StatusCode> {
        let transport = TransportKind::from_address(&connection.address)?;
        match transport {
            TransportKind::Mqtt => {
                parse_broker_address(&connection.address).map_err(|error| error.status_code())?;
            }
            TransportKind::WebSocket => {
                parse_websocket_address(&connection.address)?;
            }
            TransportKind::Udp => {
                parse_udp_destination(&connection.address)?;
            }
            TransportKind::Amqp => {
                parse_amqp_address(&connection.address)?;
            }
            #[cfg(feature = "tsn")]
            TransportKind::Tsn => {
                parse_tsn_interface(&connection.address)?;
            }
        }
        Ok(transport)
    }
}

#[cfg(all(test, feature = "tsn"))]
mod tests {
    use std::sync::Arc;

    use opcua_core::sync::RwLock;
    use opcua_server::address_space::AddressSpace;

    use super::*;

    #[test]
    fn preflight_rejects_invalid_tsn_interface() {
        // Given: a TSN connection whose interface name is invalid for Linux.
        let connection = PubSubConnectionConfig {
            connection_id: "tsn".to_string(),
            name: "tsn".to_string(),
            address: "tsn://eth:0".to_string(),
            writer_groups: Vec::new(),
            reader_groups: Vec::new(),
        };
        let engine =
            PubSubEngine::with_connections(Arc::new(RwLock::new(AddressSpace::new())), Vec::new());

        // When: engine preflight validates the connection without starting it.
        let result = engine.preflight_connection(&connection);

        // Then: invalid configuration is rejected before transport side effects.
        assert!(matches!(result, Err(StatusCode::BadConfigurationError)));
    }
}
