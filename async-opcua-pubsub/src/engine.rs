//! PubSub publishing coordinator.

use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use opcua_types::StatusCode;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod publisher;
mod queue;
mod security;
mod subscriber;
mod subscriber_mqtt;
mod subscriber_udp;
mod transport;

pub use queue::{DatagramQueue, PUBSUB_DATAGRAM_QUEUE_CAPACITY};
pub use transport::TransportKind;

use crate::{
    security::{ReplayWindow, SharedSecurityGroup},
    subscriber::SubscriberRuntime,
    PubSubConnectionConfig, PublisherId,
};

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

/// Coordinates PubSub connection configurations and transport publishing loops.
pub struct PubSubEngine {
    address_space: Arc<RwLock<AddressSpace>>,
    connections: Vec<PubSubConnectionConfig>,
    security_groups: HashMap<String, SharedSecurityGroup>,
    replay_windows: RwLock<HashMap<String, ReplayGroupState>>,
    cancel_token: Option<CancellationToken>,
    publisher_handles: Vec<JoinHandle<()>>,
    subscriber_runtime: Option<Arc<RwLock<SubscriberRuntime>>>,
    subscriber_runtime_dirty: bool,
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
            subscriber_runtime_dirty: false,
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
            subscriber_runtime_dirty: false,
            subscriber_cancel_token: None,
            subscriber_handles: Vec::new(),
            datagram_queue_capacity: PUBSUB_DATAGRAM_QUEUE_CAPACITY,
        }
    }

    /// Adds a connection configuration to be started on the next engine start.
    pub fn add_connection(&mut self, connection: PubSubConnectionConfig) {
        self.connections.push(connection);
        self.invalidate_subscriber_runtime();
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
        self.invalidate_subscriber_runtime();
        Some(removed)
    }

    /// Replaces all connection configurations with a fresh writable-config snapshot.
    ///
    /// Connection mutations made while subscribers are running do not affect
    /// active datagram processing or bound sockets. Call `stop_subscribers`
    /// followed by `start_subscribers` to apply them. The same restart
    /// requirement applies to mutations made with `add_connection` and
    /// `remove_connection`.
    pub fn replace_connections(&mut self, connections: Vec<PubSubConnectionConfig>) {
        self.connections = connections;
        self.invalidate_subscriber_runtime();
    }

    fn invalidate_subscriber_runtime(&mut self) {
        self.subscriber_runtime_dirty = true;
        if self.subscriber_cancel_token.is_none() {
            self.subscriber_runtime = None;
        }
    }

    /// Returns the configured PubSub connections.
    pub fn connection_configs(&self) -> &[PubSubConnectionConfig] {
        &self.connections
    }
}
