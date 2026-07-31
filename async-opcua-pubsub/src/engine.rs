//! PubSub publishing coordinator.

use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_server::address_space::AddressSpace;
use opcua_types::{Context, ContextOwned, StatusCode};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    security::{ReplayWindow, SharedSecurityGroup},
    subscriber::{
        effective_security_config, DataSetReaderStatus, SubscriberApplyOutcome, SubscriberRuntime,
        SubscriberSecurityConfig,
    },
    transport::{
        mqtt::{quality_of_service, start_mqtt_subscriber_with_config, MqttSubscriberConfig},
        udp::{bind_subscriber_socket, UdpSubscriberEndpoint},
    },
    MqttDeliveryGuarantee, PubSubConnectionConfig, PublisherId,
};

mod publisher;
mod security;
mod subscriber_mqtt;
mod transport;

pub use transport::TransportKind;

// Bounds retained authenticated identities to resist key-holder memory exhaustion.
const MAX_REPLAY_STREAMS_PER_SECURITY_GROUP: usize = 1024;

#[derive(Eq, Hash, PartialEq)]
enum ReplayPublisherId {
    None,
    Byte(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    String(String),
}

impl From<&PublisherId> for ReplayPublisherId {
    fn from(publisher_id: &PublisherId) -> Self {
        match publisher_id {
            PublisherId::None => Self::None,
            PublisherId::Byte(value) => Self::Byte(*value),
            PublisherId::UInt16(value) => Self::UInt16(*value),
            PublisherId::UInt32(value) => Self::UInt32(*value),
            PublisherId::UInt64(value) => Self::UInt64(*value),
            PublisherId::String(value) => Self::String(value.clone()),
        }
    }
}

#[derive(Eq, Hash, PartialEq)]
struct ReplayStreamIdentity {
    publisher_id: ReplayPublisherId,
    writer_group_id: u16,
}

impl ReplayStreamIdentity {
    fn new(publisher_id: &PublisherId, writer_group_id: u16) -> Self {
        Self {
            publisher_id: publisher_id.into(),
            writer_group_id,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct CandidateTokenSnapshot {
    current: u32,
    next: u32,
}

impl CandidateTokenSnapshot {
    fn new(current: u32, next: u32) -> Self {
        Self { current, next }
    }

    fn contains(self, token_id: u32) -> bool {
        token_id == self.current || token_id == self.next
    }
}

#[derive(Default)]
struct ReplayGroupState {
    candidate_tokens: Option<CandidateTokenSnapshot>,
    streams: HashMap<ReplayStreamIdentity, HashMap<u32, ReplayWindow>>,
}

impl ReplayGroupState {
    fn reconcile_candidate_tokens(&mut self, candidate_tokens: CandidateTokenSnapshot) {
        if self.candidate_tokens == Some(candidate_tokens) {
            return;
        }

        self.streams.retain(|_, token_windows| {
            token_windows.retain(|token_id, _| candidate_tokens.contains(*token_id));
            !token_windows.is_empty()
        });
        self.candidate_tokens = Some(candidate_tokens);
    }

    fn stream_windows_mut_or_insert(
        &mut self,
        stream_identity: ReplayStreamIdentity,
    ) -> Result<&mut HashMap<u32, ReplayWindow>, StatusCode> {
        let has_capacity = self.streams.len() < MAX_REPLAY_STREAMS_PER_SECURITY_GROUP;
        match self.streams.entry(stream_identity) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) if has_capacity => Ok(entry.insert(HashMap::new())),
            Entry::Vacant(_) => Err(StatusCode::BadResourceUnavailable),
        }
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
    replay_windows: RwLock<HashMap<String, ReplayGroupState>>,
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

        let reader_ids = connection_reader_ids(&connection);
        let readers = connection
            .reader_groups
            .iter()
            .flat_map(|reader_group| reader_group.dataset_readers.iter())
            .cloned()
            .collect::<Vec<_>>();
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

        let mut outcome = SubscriberApplyOutcome::default();
        let mut runtime = runtime.write();
        for reader in &readers {
            outcome.accumulate(runtime.process_datagram_for_reader(reader, payload, ctx)?);
        }
        Ok(outcome)
    }

    /// Returns a subscriber DataSetReader status snapshot.
    #[must_use]
    pub fn subscriber_status(&self, reader_id: u16) -> Option<DataSetReaderStatus> {
        self.subscriber_runtime
            .as_ref()
            .and_then(|runtime| runtime.read().reader_status(reader_id))
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
                let reader = reader.clone();
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

                let (queue, payload_rx) = DatagramQueue::new(self.datagram_queue_capacity);
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

                // Forwarder task: drains received payloads into only the owning reader.
                handles.push(
                    subscriber_mqtt::MqttForwarder {
                        runtime: runtime.clone(),
                        reader,
                        payload_rx,
                        cancel: cancel_token.clone(),
                        connection_id: connection_id.clone(),
                        topic: topic_filter,
                    }
                    .spawn(),
                );
            }
        }

        handles
    }

    fn ensure_subscriber_runtime(&mut self) -> Result<Arc<RwLock<SubscriberRuntime>>, StatusCode> {
        if let Some(runtime) = &self.subscriber_runtime {
            return Ok(runtime.clone());
        }

        let runtime = SubscriberRuntime::with_reader_validated_connections(
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
