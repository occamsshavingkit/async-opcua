use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_types::{Context, StatusCode};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

use super::PubSubEngine;
use crate::{
    config::ParsedSubscriberTransport,
    subscriber::{
        DataSetReaderKey, DataSetReaderStatus, SubscriberApplyOutcome, SubscriberRuntime,
    },
    transport::{
        mqtt::MqttBrokerAddress,
        udp::{bind_subscriber_socket, UdpSubscriberEndpoint},
    },
    DataSetReaderConfig, PubSubConnectionConfig,
};

mod datagram_processor;

pub(super) use datagram_processor::SubscriberDatagramProcessor;

enum SubscriberConnectionPlan {
    Udp(PubSubConnectionConfig, UdpSubscriberEndpoint),
    Mqtt(PubSubConnectionConfig, MqttBrokerAddress),
}

enum PreparedSubscriberConnection {
    Udp(
        PubSubConnectionConfig,
        UdpSocket,
        SubscriberDatagramProcessor,
    ),
    Mqtt(
        PubSubConnectionConfig,
        MqttBrokerAddress,
        Vec<SubscriberDatagramProcessor>,
    ),
}

enum PreparedSubscriberRuntime {
    Existing(Arc<RwLock<SubscriberRuntime>>),
    New(Arc<RwLock<SubscriberRuntime>>),
}

impl PreparedSubscriberRuntime {
    fn runtime(&self) -> Arc<RwLock<SubscriberRuntime>> {
        match self {
            Self::Existing(runtime) | Self::New(runtime) => runtime.clone(),
        }
    }

    fn commit(self, runtime: &mut Option<Arc<RwLock<SubscriberRuntime>>>) {
        match self {
            Self::Existing(_) => {}
            Self::New(prepared) => *runtime = Some(prepared),
        }
    }
}

impl PubSubEngine {
    /// Processes one subscriber datagram for the named connection.
    pub fn process_subscriber_datagram(
        &mut self,
        connection_id: &str,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        if self.subscribers_are_running() && self.subscriber_runtime_dirty {
            return Err(StatusCode::BadInvalidState);
        }

        let security = self
            .connections
            .iter()
            .find(|connection| connection.connection_id == connection_id)
            .ok_or(StatusCode::BadNotFound)?
            .validated_subscriber_security()?;
        let runtime = self.ensure_subscriber_runtime()?;

        if let Some(security) = security {
            let security_policy = SecurityPolicy::from_uri(&security.security_policy_uri);
            let decoded = if security_policy == SecurityPolicy::Unknown {
                Err(StatusCode::BadSecurityChecksFailed)
            } else {
                self.decode_connection_subscriber_uadp_message(
                    connection_id,
                    &security,
                    security_policy,
                    payload,
                    ctx,
                )
            };

            return match decoded {
                Ok(message) => runtime
                    .write()
                    .process_network_message_for_connection(connection_id, &message),
                Err(status) => {
                    runtime
                        .write()
                        .record_security_failure_for_connection(connection_id);
                    Err(status)
                }
            };
        }

        let outcome = runtime
            .write()
            .process_datagram_for_connection(connection_id, payload, ctx);
        outcome
    }

    /// Returns a subscriber DataSetReader status snapshot.
    ///
    /// This compatibility lookup returns `None` when multiple connections
    /// contain the same numeric DataSetReader id. Use
    /// [`Self::subscriber_status_by_key`] when ids may repeat.
    #[must_use]
    pub fn subscriber_status(&self, reader_id: u16) -> Option<DataSetReaderStatus> {
        self.subscriber_runtime
            .as_ref()
            .and_then(|runtime| runtime.read().reader_status(reader_id))
    }

    /// Returns a subscriber status by connection-scoped DataSetReader key.
    ///
    /// The key pairs connection_id with dataset_reader_id to prevent
    /// cross-connection collisions when numeric reader ids repeat.
    #[must_use]
    pub fn subscriber_status_by_key(&self, key: &DataSetReaderKey) -> Option<DataSetReaderStatus> {
        self.subscriber_runtime
            .as_ref()
            .and_then(|runtime| runtime.read().reader_status_by_key(key))
    }

    /// Returns true when subscriber receive loops are running.
    pub fn subscribers_are_running(&self) -> bool {
        self.subscriber_cancel_token.is_some()
    }

    /// Returns the number of active subscriber receive task handles.
    pub fn active_subscriber_handle_count(&self) -> usize {
        self.subscriber_handles.len()
    }

    /// Starts subscriber receive loops for configured ReaderGroups.
    ///
    /// Dispatches by transport mapping (OPC-10000-14 §6.4): UDP connections
    /// spawn datagram receive loops (§6.4.1), while MQTT (`mqtt://`)
    /// connections spawn one broker subscriber task per DataSetReader (§6.4.2).
    /// Unsupported subscriber transports return `StatusCode::BadNotSupported`.
    /// All configuration is validated and every UDP socket is bound before any
    /// subscriber task or engine running state is committed.
    ///
    /// # Errors
    ///
    /// Returns the configuration, UDP bind, or multicast join error without
    /// starting any subscriber when preparation fails.
    pub async fn start_subscribers(&mut self) -> Result<(), StatusCode> {
        if self.subscribers_are_running() {
            return Ok(());
        }

        let connection_plans = self
            .connections
            .iter()
            .filter(|connection| !connection.reader_groups.is_empty())
            .cloned()
            .map(|connection| match connection.subscriber_preflight()? {
                ParsedSubscriberTransport::Udp(endpoint) => {
                    Ok(SubscriberConnectionPlan::Udp(connection, endpoint))
                }
                ParsedSubscriberTransport::Mqtt(broker_address) => {
                    Ok(SubscriberConnectionPlan::Mqtt(connection, broker_address))
                }
            })
            .collect::<Result<Vec<_>, StatusCode>>()?;
        if connection_plans.is_empty() {
            self.subscriber_runtime = None;
            self.subscriber_runtime_dirty = false;
            return Ok(());
        }

        let prepared_runtime = match (&self.subscriber_runtime, self.subscriber_runtime_dirty) {
            (Some(runtime), false) => PreparedSubscriberRuntime::Existing(runtime.clone()),
            (None, false) | (Some(_), true) | (None, true) => PreparedSubscriberRuntime::New(
                Arc::new(RwLock::new(SubscriberRuntime::from_connections(
                    self.address_space.clone(),
                    self.connections.clone(),
                )?)),
            ),
        };

        let runtime = prepared_runtime.runtime();
        let mut prepared_connections = Vec::with_capacity(connection_plans.len());
        for connection in connection_plans {
            match connection {
                SubscriberConnectionPlan::Udp(connection, endpoint) => {
                    let processor = SubscriberDatagramProcessor::new(
                        runtime.clone(),
                        &connection.connection_id,
                        connection_readers(&connection),
                        self.prepare_subscriber_security_processor(&connection)?,
                    );
                    let socket = bind_subscriber_socket(endpoint).await?;
                    prepared_connections.push(PreparedSubscriberConnection::Udp(
                        connection, socket, processor,
                    ));
                }
                SubscriberConnectionPlan::Mqtt(connection, broker_address) => {
                    let processors = connection_readers(&connection)
                        .into_iter()
                        .map(|reader| {
                            Ok(SubscriberDatagramProcessor::new(
                                runtime.clone(),
                                &connection.connection_id,
                                vec![reader],
                                self.prepare_subscriber_security_processor(&connection)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, StatusCode>>()?;
                    prepared_connections.push(PreparedSubscriberConnection::Mqtt(
                        connection,
                        broker_address,
                        processors,
                    ));
                }
            }
        }

        let cancel_token = CancellationToken::new();
        let mut handles = Vec::with_capacity(prepared_connections.len());

        for connection in prepared_connections {
            match connection {
                PreparedSubscriberConnection::Udp(connection, socket, processor) => {
                    handles.extend(self.spawn_udp_subscriber(
                        connection,
                        socket,
                        processor,
                        cancel_token.clone(),
                    ));
                }
                PreparedSubscriberConnection::Mqtt(connection, broker_address, processors) => {
                    handles.extend(self.spawn_mqtt_subscribers(
                        connection,
                        broker_address,
                        processors,
                        cancel_token.clone(),
                    ));
                }
            }
        }

        prepared_runtime.commit(&mut self.subscriber_runtime);
        self.subscriber_runtime_dirty = false;
        self.subscriber_cancel_token = Some(cancel_token);
        self.subscriber_handles = handles;
        Ok(())
    }

    /// Stops all subscriber receive loops and waits for them to finish.
    pub async fn stop_subscribers(&mut self) {
        if let Some(cancel_token) = self.subscriber_cancel_token.take() {
            cancel_token.cancel();
        }

        while let Some(handle) = self.subscriber_handles.pop() {
            let _ = handle.await;
        }
    }

    fn ensure_subscriber_runtime(&mut self) -> Result<Arc<RwLock<SubscriberRuntime>>, StatusCode> {
        if let Some(runtime) = &self.subscriber_runtime {
            if self.subscribers_are_running() || !self.subscriber_runtime_dirty {
                return Ok(runtime.clone());
            }
        }

        let runtime = SubscriberRuntime::with_reader_validated_connections(
            self.address_space.clone(),
            self.connections.clone(),
        )?;
        let runtime = Arc::new(RwLock::new(runtime));
        self.subscriber_runtime = Some(runtime.clone());
        self.subscriber_runtime_dirty = false;
        Ok(runtime)
    }
}

fn connection_readers(connection: &PubSubConnectionConfig) -> Vec<DataSetReaderConfig> {
    connection
        .reader_groups
        .iter()
        .flat_map(|reader_group| reader_group.dataset_readers.iter())
        .cloned()
        .collect()
}
