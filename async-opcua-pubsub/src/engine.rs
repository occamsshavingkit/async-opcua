//! PubSub publishing coordinator.

use std::{collections::HashMap, sync::Arc};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_server::address_space::AddressSpace;
use opcua_types::{Context, ContextOwned, MessageSecurityMode, StatusCode};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    codec::uadp::UadpNetworkMessage,
    security::{ReplayWindow, SecurityGroup, SharedSecurityGroup, UadpSecurityCodec},
    subscriber::{
        effective_security_config, DataSetReaderStatus, SubscriberApplyOutcome, SubscriberRuntime,
        SubscriberSecurityConfig,
    },
    transport::{
        amqp::AmqpPublisher,
        mqtt::{
            quality_of_service, start_mqtt_subscriber_with_config, MqttPublisher,
            MqttSubscriberConfig,
        },
        udp::{bind_subscriber_socket, UdpPublisher, UdpSubscriberEndpoint},
        websocket::WebSocketPublisher,
    },
    MqttDeliveryGuarantee, PubSubConnectionConfig, PubSubPublisher,
};

/// Supported OPC UA PubSub transport mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// MQTT broker transport.
    Mqtt,
    /// UDP multicast or unicast transport.
    Udp,
    /// AMQP broker transport.
    Amqp,
    /// WebSocket transport.
    WebSocket,
    /// TSN transport. Experimental, requires the `tsn` feature.
    #[cfg(feature = "tsn")]
    Tsn,
}

impl TransportKind {
    /// Classifies a PubSub connection address by URI scheme.
    pub fn from_address(address: &str) -> Result<Self, StatusCode> {
        let address = address.trim();

        if address.starts_with("mqtt://") || address.starts_with("mqtts://") {
            return Ok(Self::Mqtt);
        }

        if address.starts_with("udp://") {
            return Ok(Self::Udp);
        }

        #[cfg(feature = "tsn")]
        if address.starts_with("tsn://") {
            return Ok(Self::Tsn);
        }

        if address.starts_with("amqp://") || address.starts_with("amqps://") {
            return Ok(Self::Amqp);
        }

        if address.starts_with("ws://") || address.starts_with("wss://") {
            return Ok(Self::WebSocket);
        }

        Err(StatusCode::BadInvalidArgument)
    }
}

/// Default PubSub datagram queue capacity.
///
/// `OperationalLimits` (Part 4) only covers service-call bounds, so the
/// PubSub datagram bound is a crate-level constant. Override per engine with
/// [`PubSubEngine::set_datagram_queue_capacity`].
pub const PUBSUB_DATAGRAM_QUEUE_CAPACITY: usize = 1024;

/// Bounded queue for incoming PubSub datagrams (OPC-10000-14 §9.1.10.1).
///
/// Enforces a processing limit on received PubSub NetworkMessages. When the
/// queue is full, [`DatagramQueue::try_enqueue`] returns
/// `StatusCode::BadTooManyPublishRequests` and the caller drops the datagram
/// rather than accumulating unbounded backpressure.
#[derive(Debug)]
pub struct DatagramQueue {
    tx: mpsc::Sender<Vec<u8>>,
    capacity: usize,
}

impl DatagramQueue {
    /// Creates a new bounded datagram queue with the requested capacity.
    ///
    /// The capacity is clamped to a minimum of 1 so a misconfigured zero
    /// capacity does not reject every datagram.
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<Vec<u8>>) {
        let capacity = capacity.max(1);
        let (tx, rx) = mpsc::channel(capacity);
        (Self { tx, capacity }, rx)
    }

    /// Attempts to enqueue a datagram without blocking.
    ///
    /// Returns:
    /// - `Ok(())` when the datagram was accepted.
    /// - `Err(StatusCode::BadTooManyPublishRequests)` when the queue is full
    ///   (OPC-10000-14 §9.1.10.1 processing-limit enforcement).
    /// - `Err(StatusCode::BadNoCommunication)` when the consumer has dropped
    ///   its receiver (e.g. the engine is shutting down).
    pub fn try_enqueue(&self, payload: Vec<u8>) -> Result<(), StatusCode> {
        self.tx.try_send(payload).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => StatusCode::BadTooManyPublishRequests,
            mpsc::error::TrySendError::Closed(_) => StatusCode::BadNoCommunication,
        })
    }

    /// Returns the configured queue capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns a clone of the underlying sender for transport tasks that own
    /// their own receive loops (e.g. the MQTT broker subscriber).
    ///
    /// Callers that use this raw sender must call `try_send` and treat
    /// `TrySendError::Full` as `StatusCode::BadTooManyPublishRequests` to
    /// honour OPC-10000-14 §9.1.10.1.
    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.tx.clone()
    }
}

/// Coordinates PubSub connection configurations and transport publishing loops.
pub struct PubSubEngine {
    address_space: Arc<RwLock<AddressSpace>>,
    connections: Vec<PubSubConnectionConfig>,
    security_groups: HashMap<String, SharedSecurityGroup>,
    replay_windows: RwLock<HashMap<String, ReplayWindow>>,
    cancel_token: Option<CancellationToken>,
    publisher_handles: Vec<JoinHandle<()>>,
    subscriber_runtime: Option<Arc<RwLock<SubscriberRuntime>>>,
    subscriber_cancel_token: Option<CancellationToken>,
    subscriber_handles: Vec<JoinHandle<()>>,
    datagram_queue_capacity: usize,
}

impl PubSubEngine {
    /// Creates an empty PubSub engine for the supplied OPC UA address space.
    pub fn new(address_space: Arc<RwLock<AddressSpace>>) -> Self {
        Self {
            address_space,
            connections: Vec::new(),
            security_groups: HashMap::new(),
            replay_windows: RwLock::new(HashMap::new()),
            cancel_token: None,
            publisher_handles: Vec::new(),
            subscriber_runtime: None,
            subscriber_cancel_token: None,
            subscriber_handles: Vec::new(),
            datagram_queue_capacity: PUBSUB_DATAGRAM_QUEUE_CAPACITY,
        }
    }

    /// Creates a PubSub engine preloaded with connection configurations.
    pub fn with_connections(
        address_space: Arc<RwLock<AddressSpace>>,
        connections: Vec<PubSubConnectionConfig>,
    ) -> Self {
        Self {
            address_space,
            connections,
            security_groups: HashMap::new(),
            replay_windows: RwLock::new(HashMap::new()),
            cancel_token: None,
            publisher_handles: Vec::new(),
            subscriber_runtime: None,
            subscriber_cancel_token: None,
            subscriber_handles: Vec::new(),
            datagram_queue_capacity: PUBSUB_DATAGRAM_QUEUE_CAPACITY,
        }
    }

    /// Adds a connection configuration to be started on the next engine start.
    pub fn add_connection(&mut self, connection: PubSubConnectionConfig) {
        self.connections.push(connection);
        if self.subscriber_cancel_token.is_none() {
            self.subscriber_runtime = None;
        }
    }

    /// Sets the bounded datagram queue capacity used by subscriber receive
    /// loops (OPC-10000-14 §9.1.10.1).
    ///
    /// Datagrams received while the queue is full are rejected with
    /// `StatusCode::BadTooManyPublishRequests`. Must be called before
    /// [`PubSubEngine::start_subscribers`]; later changes only apply to the
    /// next subscriber start. The capacity is clamped to a minimum of 1.
    pub fn set_datagram_queue_capacity(&mut self, capacity: usize) {
        self.datagram_queue_capacity = capacity.max(1);
    }

    /// Returns the configured datagram queue capacity.
    #[must_use]
    pub fn datagram_queue_capacity(&self) -> usize {
        self.datagram_queue_capacity
    }

    /// Removes a connection configuration by connection id.
    pub fn remove_connection(&mut self, connection_id: &str) -> Option<PubSubConnectionConfig> {
        let index = self
            .connections
            .iter()
            .position(|connection| connection.connection_id == connection_id)?;
        let removed = self.connections.remove(index);
        if self.subscriber_cancel_token.is_none() {
            self.subscriber_runtime = None;
        }
        Some(removed)
    }

    /// Replaces all connection configurations with a fresh writable-config snapshot.
    pub fn replace_connections(&mut self, connections: Vec<PubSubConnectionConfig>) {
        self.connections = connections;
        self.subscriber_runtime = None;
    }

    /// Returns the configured PubSub connections.
    pub fn connection_configs(&self) -> &[PubSubConnectionConfig] {
        &self.connections
    }

    /// Registers a PubSub security group for publisher message signing.
    pub fn register_security_group(
        &mut self,
        security_group: SecurityGroup,
    ) -> SharedSecurityGroup {
        let group_id = security_group.group_id().to_string();
        let shared_group = Arc::new(RwLock::new(security_group));
        self.replay_windows.write().remove(&group_id);
        self.security_groups.insert(group_id, shared_group.clone());
        shared_group
    }

    /// Registers shared PubSub security group state for publisher message signing.
    pub fn register_shared_security_group(&mut self, security_group: SharedSecurityGroup) {
        let group_id = security_group.read().group_id().to_string();
        self.replay_windows.write().remove(&group_id);
        self.security_groups.insert(group_id, security_group);
    }

    /// Removes a registered PubSub security group.
    pub fn remove_security_group(&mut self, group_id: &str) -> Option<SharedSecurityGroup> {
        self.replay_windows.write().remove(group_id);
        self.security_groups.remove(group_id)
    }

    /// Returns a registered PubSub security group.
    pub fn security_group(&self, group_id: &str) -> Option<SharedSecurityGroup> {
        self.security_groups.get(group_id).cloned()
    }

    /// Encodes a publisher UADP NetworkMessage using the current key for a security group.
    pub fn encode_publisher_uadp_message(
        &self,
        security_group_id: &str,
        security_mode: MessageSecurityMode,
        security_policy: SecurityPolicy,
        message: &UadpNetworkMessage,
        ctx: &Context<'_>,
    ) -> Result<Vec<u8>, StatusCode> {
        let security_group = self
            .security_groups
            .get(security_group_id)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let key_set = security_group.read().current_key_set().clone();
        UadpSecurityCodec::new(security_mode, security_policy, key_set)
            .encode_network_message(message, ctx)
            .map_err(|error| error.status())
    }

    /// Signs a publisher UADP NetworkMessage using the current key for a security group.
    pub fn sign_publisher_uadp_message(
        &self,
        security_group_id: &str,
        security_policy: SecurityPolicy,
        message: &UadpNetworkMessage,
        ctx: &Context<'_>,
    ) -> Result<Vec<u8>, StatusCode> {
        self.encode_publisher_uadp_message(
            security_group_id,
            MessageSecurityMode::Sign,
            security_policy,
            message,
            ctx,
        )
    }

    /// Decodes and verifies a subscriber UADP NetworkMessage using a security group's current key.
    pub fn decode_subscriber_uadp_message(
        &self,
        security_group_id: &str,
        security_mode: MessageSecurityMode,
        security_policy: SecurityPolicy,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<UadpNetworkMessage, StatusCode> {
        let security_group = self
            .security_groups
            .get(security_group_id)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let (token_id, key_sets) = {
            let security_group = security_group.read();
            (
                security_group.current_key_set().token_id(),
                vec![
                    security_group.current_key_set().clone(),
                    security_group.next_key_set().clone(),
                ],
            )
        };
        let message = UadpSecurityCodec::with_candidates(security_mode, security_policy, key_sets)
            .decode_network_message(payload, ctx)
            .map_err(|error| error.status())?;

        if security_mode != MessageSecurityMode::None {
            self.replay_windows
                .write()
                .entry(security_group_id.to_string())
                .or_default()
                .check(token_id, message.sequence_number)
                .map_err(|error| error.status())?;
        }

        Ok(message)
    }

    /// Processes one subscriber datagram for the named connection.
    pub fn process_subscriber_datagram(
        &mut self,
        connection_id: &str,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let connection = self
            .connections
            .iter()
            .find(|connection| connection.connection_id == connection_id)
            .cloned()
            .ok_or(StatusCode::BadNotFound)?;
        connection.validate_subscriber_config()?;

        let reader_ids = connection_reader_ids(&connection);
        let security = first_effective_security(&connection);
        let runtime = self.ensure_subscriber_runtime()?;

        if let Some(security) = security {
            let security_policy = SecurityPolicy::from_uri(&security.security_policy_uri);
            let decoded = if security_policy == SecurityPolicy::Unknown {
                Err(StatusCode::BadSecurityChecksFailed)
            } else {
                self.decode_subscriber_uadp_message(
                    &security.security_group_id,
                    security.security_mode,
                    security_policy,
                    payload,
                    ctx,
                )
            };

            return match decoded {
                Ok(message) => runtime.write().process_network_message(&message),
                Err(status) => {
                    runtime
                        .write()
                        .record_security_failure_for_readers(&reader_ids);
                    Err(status)
                }
            };
        }

        let result = runtime.write().process_datagram(payload, ctx);
        result
    }

    /// Returns a subscriber DataSetReader status snapshot.
    #[must_use]
    pub fn subscriber_status(&self, reader_id: u16) -> Option<DataSetReaderStatus> {
        self.subscriber_runtime
            .as_ref()
            .and_then(|runtime| runtime.read().reader_status(reader_id))
    }

    /// Returns true when the engine has started publisher loops.
    pub fn is_running(&self) -> bool {
        self.cancel_token.is_some()
    }

    /// Returns the number of active publisher coordinator handles.
    pub fn active_handle_count(&self) -> usize {
        self.publisher_handles.len()
    }

    /// Returns true when subscriber receive loops are running.
    pub fn subscribers_are_running(&self) -> bool {
        self.subscriber_cancel_token.is_some()
    }

    /// Returns the number of active subscriber receive task handles.
    pub fn active_subscriber_handle_count(&self) -> usize {
        self.subscriber_handles.len()
    }

    /// Starts transport publisher loops for all configured connections.
    pub fn start(&mut self) -> Result<(), StatusCode> {
        if self.is_running() {
            return Ok(());
        }

        let cancel_token = CancellationToken::new();
        let mut handles = Vec::with_capacity(self.connections.len());

        for connection in &self.connections {
            match self.start_connection(connection.clone(), cancel_token.clone()) {
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

    /// Starts subscriber receive loops for configured ReaderGroups.
    ///
    /// Dispatches by transport mapping (OPC-10000-14 §6.4): UDP connections
    /// spawn datagram receive loops (§6.4.1), while MQTT (`mqtt://`/`mqtts://`)
    /// connections spawn one broker subscriber task per DataSetReader (§6.4.2).
    /// Other broker transports are logged and skipped so a single unsupported
    /// connection does not abort subscriber startup.
    pub fn start_subscribers(&mut self) -> Result<(), StatusCode> {
        if self.subscribers_are_running() {
            return Ok(());
        }

        let connections = self
            .connections
            .iter()
            .filter(|connection| !connection.reader_groups.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        if connections.is_empty() {
            return Ok(());
        }

        for connection in &connections {
            connection.validate_subscriber_config()?;
        }

        let runtime = self.ensure_subscriber_runtime()?;
        let cancel_token = CancellationToken::new();
        let mut handles = Vec::with_capacity(connections.len());

        for connection in connections {
            let kind = TransportKind::from_address(&connection.address)?;
            let connection_id = connection.connection_id.clone();
            match kind {
                TransportKind::Udp => {
                    handles.extend(self.spawn_udp_subscriber(
                        connection,
                        runtime.clone(),
                        cancel_token.clone(),
                    )?);
                }
                TransportKind::Mqtt => {
                    handles.extend(self.spawn_mqtt_subscribers(
                        connection,
                        runtime.clone(),
                        cancel_token.clone(),
                    ));
                }
                TransportKind::Amqp | TransportKind::WebSocket => {
                    tracing::warn!(
                        %connection_id,
                        "subscriber dispatch not yet supported for this PubSub transport; ignoring connection"
                    );
                }
                #[cfg(feature = "tsn")]
                TransportKind::Tsn => {
                    tracing::warn!(
                        %connection_id,
                        "TSN subscriber dispatch not yet supported; ignoring connection"
                    );
                }
            }
        }

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

    /// Spawns a single UDP datagram receive loop for a broker-less PubSub
    /// connection (OPC-10000-14 §6.4.1).
    ///
    /// Received payloads are forwarded across a bounded
    /// [`DatagramQueue`] (OPC-10000-14 §9.1.10.1) to a consumer task that
    /// hands them to `SubscriberRuntime::process_datagram`. When the queue is
    /// full (processing can't keep up), the producer rejects the datagram with
    /// `StatusCode::BadTooManyPublishRequests` and drops it rather than
    /// blocking the receive loop or growing memory without bound.
    ///
    /// Returns the producer and consumer task handles so the engine can await
    /// both on shutdown.
    fn spawn_udp_subscriber(
        &self,
        connection: PubSubConnectionConfig,
        runtime: Arc<RwLock<SubscriberRuntime>>,
        cancel_token: CancellationToken,
    ) -> Result<Vec<JoinHandle<()>>, StatusCode> {
        let endpoint = UdpSubscriberEndpoint::parse(&connection.address)?;
        let connection_id = connection.connection_id;
        let (queue, payload_rx) = DatagramQueue::new(self.datagram_queue_capacity);
        let mut handles = Vec::with_capacity(2);

        // Consumer task: drains the bounded queue and runs the (synchronous)
        // subscriber runtime processing. Exits on cancellation or once the
        // producer drops its sender and the queue drains.
        let consumer_runtime = runtime.clone();
        let consumer_cancel = cancel_token.clone();
        let consumer_connection_id = connection_id.clone();
        handles.push(tokio::spawn(async move {
            let mut payload_rx = payload_rx;
            loop {
                tokio::select! {
                    _ = consumer_cancel.cancelled() => break,
                    payload = payload_rx.recv() => {
                        let Some(payload) = payload else { break };
                        let ctx_owned = ContextOwned::default();
                        let ctx = ctx_owned.context();
                        if let Err(status) = consumer_runtime
                            .write()
                            .process_datagram(&payload, &ctx)
                        {
                            tracing::debug!(
                                ?status,
                                %consumer_connection_id,
                                "dropped PubSub subscriber UDP datagram"
                            );
                        }
                    }
                }
            }
        }));

        // Producer task: receives UDP datagrams and enqueues them on the
        // bounded queue. On `BadTooManyPublishRequests` the datagram is
        // dropped (logged) so the receive loop never blocks on a full queue.
        handles.push(tokio::spawn(async move {
            let socket = match bind_subscriber_socket(endpoint).await {
                Ok(socket) => socket,
                Err(status) => {
                    tracing::error!(
                        ?status,
                        %connection_id,
                        "failed to bind PubSub subscriber UDP socket"
                    );
                    return;
                }
            };
            let mut buf = vec![0_u8; 65_535];

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    received = socket.recv_from(&mut buf) => {
                        match received {
                            Ok((len, _peer)) => {
                                if let Err(status) = queue.try_enqueue(buf[..len].to_vec()) {
                                    if status == StatusCode::BadTooManyPublishRequests {
                                        tracing::warn!(
                                            ?status,
                                            %connection_id,
                                            "PubSub subscriber UDP datagram rejected; \
                                             datagram queue full"
                                        );
                                    } else {
                                        tracing::debug!(
                                            ?status,
                                            %connection_id,
                                            "PubSub subscriber UDP datagram not enqueued; \
                                             queue closed"
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                tracing::warn!(
                                    ?error,
                                    %connection_id,
                                    "failed to receive PubSub subscriber UDP datagram"
                                );
                            }
                        }
                    }
                }
            }
        }));

        Ok(handles)
    }

    /// Spawns MQTT broker subscriber tasks for each DataSetReader in the
    /// connection's ReaderGroups (OPC-10000-14 §6.4.2).
    ///
    /// Each DataSetReader maps to one MQTT topic subscription. The broker
    /// subscriber (`transport::mqtt::start_mqtt_subscriber`) forwards received
    /// payload bytes over an mpsc channel; a per-reader forwarder task drains
    /// that channel and hands each payload to
    /// `SubscriberRuntime::process_datagram`. Broker connection failures are
    /// logged and retried with backoff inside the subscriber task, so they
    /// never crash the engine or abort the remaining readers.
    ///
    fn spawn_mqtt_subscribers(
        &self,
        connection: PubSubConnectionConfig,
        runtime: Arc<RwLock<SubscriberRuntime>>,
        cancel_token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        let connection_id = connection.connection_id.clone();
        let broker_address = connection.address.clone();
        let mut handles = Vec::new();

        for reader_group in &connection.reader_groups {
            for reader in &reader_group.dataset_readers {
                let reader_id = reader.dataset_reader_id;
                let topic_filter = reader.mqtt_topic_filter(reader_group.reader_group_id);
                let delivery_guarantee = reader
                    .mqtt_transport
                    .as_ref()
                    .map_or(MqttDeliveryGuarantee::AtLeastOnce, |transport| {
                        transport.delivery_guarantee
                    });
                let subscriber_config = MqttSubscriberConfig::new(
                    broker_address.clone(),
                    topic_filter.clone(),
                    quality_of_service(delivery_guarantee),
                );

                let (queue, mut payload_rx) = DatagramQueue::new(self.datagram_queue_capacity);
                // Raw sender for the broker subscriber task, which uses
                // `try_send` and treats `Full` as `BadTooManyPublishRequests`
                // (see `transport::mqtt::start_mqtt_subscriber`).
                let payload_tx = queue.sender();

                // Subscriber task: connects to the broker (with reconnect
                // backoff) and forwards published payloads to the channel.
                let subscriber_handle = start_mqtt_subscriber_with_config(
                    subscriber_config,
                    payload_tx,
                    cancel_token.clone(),
                );
                handles.push(subscriber_handle);

                // Forwarder task: drains received payloads into the subscriber
                // runtime. Exits on cancellation or when the subscriber drops
                // its sender (e.g. unrecoverable broker loss), which in turn
                // lets the subscriber task wind down.
                let runtime = runtime.clone();
                let cancel = cancel_token.clone();
                let connection_id = connection_id.clone();
                let topic = topic_filter.clone();
                handles.push(tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            payload = payload_rx.recv() => {
                                let Some(payload) = payload else { break };
                                let ctx_owned = ContextOwned::default();
                                let ctx = ctx_owned.context();
                                if let Err(status) = runtime
                                    .write()
                                    .process_datagram(&payload, &ctx)
                                {
                                    tracing::debug!(
                                        ?status,
                                        %connection_id,
                                        %topic,
                                        reader_id,
                                        "dropped PubSub subscriber MQTT payload"
                                    );
                                }
                            }
                        }
                    }
                }));
            }
        }

        handles
    }

    fn start_connection(
        &self,
        connection: PubSubConnectionConfig,
        cancel_token: CancellationToken,
    ) -> Result<JoinHandle<()>, StatusCode> {
        match TransportKind::from_address(&connection.address)? {
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

    fn ensure_subscriber_runtime(&mut self) -> Result<Arc<RwLock<SubscriberRuntime>>, StatusCode> {
        if let Some(runtime) = &self.subscriber_runtime {
            return Ok(runtime.clone());
        }

        let runtime = SubscriberRuntime::with_connections(
            self.address_space.clone(),
            self.connections.clone(),
        )?;
        let runtime = Arc::new(RwLock::new(runtime));
        self.subscriber_runtime = Some(runtime.clone());
        Ok(runtime)
    }
}

fn first_effective_security(
    connection: &PubSubConnectionConfig,
) -> Option<SubscriberSecurityConfig> {
    connection
        .reader_groups
        .iter()
        .flat_map(|reader_group| {
            reader_group
                .dataset_readers
                .iter()
                .filter_map(move |reader| effective_security_config(reader_group, reader))
        })
        .next()
}

fn connection_reader_ids(connection: &PubSubConnectionConfig) -> Vec<u16> {
    connection
        .reader_groups
        .iter()
        .flat_map(|reader_group| reader_group.dataset_readers.iter())
        .map(|reader| reader.dataset_reader_id)
        .collect()
}
