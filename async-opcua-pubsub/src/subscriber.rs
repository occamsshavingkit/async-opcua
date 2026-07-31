//! UADP subscriber runtime for applying received DataSet fields to Variables.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use opcua_core::sync::RwLock;
use opcua_server::address_space::{AddressSpace, NodeType};
use opcua_types::{
    AttributeId, BinaryDecodable, Context, DataValue, PubSubState, StatusCode, UAString, Variant,
};

use crate::{
    codec::{
        json::{JsonDataSetMessage, JsonNetworkMessage},
        uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    },
    config::{DataSetReaderConfig, FieldTargetConfig, PubSubConnectionConfig, ReaderGroupConfig},
};

mod routing;

/// Subscriber-side processing error captured in reader diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriberError {
    /// Received field count does not match configured target mappings.
    FieldCountMismatch,
    /// A configured target node was not found.
    TargetNotFound,
    /// A configured target node is not a Variable.
    TargetNotVariable,
    /// The configured target mapping is unsupported by this runtime.
    UnsupportedTarget,
    /// The reader did not receive a new DataSetMessage within MessageReceiveTimeout.
    MessageReceiveTimeout,
    /// Received metadata major version is incompatible with configured metadata.
    MetadataMajorVersionMismatch,
}

/// Observable per-DataSetReader status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSetReaderStatus {
    /// Part 14 PubSub state for this DataSetReader.
    pub state: PubSubState,
    /// Last accepted DataSetMessage sequence number.
    pub last_sequence_number: Option<u64>,
    /// Last accepted receive timestamp.
    pub last_receive_time: Option<Instant>,
    /// Last structured subscriber error.
    pub last_error: Option<SubscriberError>,
    /// Accepted DataSetMessages.
    pub accepted_count: u64,
    /// Messages filtered by reader criteria.
    pub filtered_count: u64,
    /// Malformed or unsupported messages.
    pub dropped_count: u64,
    /// Observed sequence gaps.
    pub sequence_gap_count: u64,
    /// Observed duplicate sequences.
    pub duplicate_count: u64,
    /// Observed out-of-order sequences.
    pub out_of_order_count: u64,
    /// MessageReceiveTimeout expirations.
    pub timeout_count: u64,
    /// Security verification, token, nonce, or replay failures.
    pub security_failure_count: u64,
    pub(crate) metadata_mismatch_since: Option<Instant>,
}

impl Default for DataSetReaderStatus {
    fn default() -> Self {
        Self {
            state: PubSubState::PreOperational,
            last_sequence_number: None,
            last_receive_time: None,
            last_error: None,
            accepted_count: 0,
            filtered_count: 0,
            dropped_count: 0,
            sequence_gap_count: 0,
            duplicate_count: 0,
            out_of_order_count: 0,
            timeout_count: 0,
            security_failure_count: 0,
            metadata_mismatch_since: None,
        }
    }
}

/// Result summary for one subscriber datagram or NetworkMessage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubscriberApplyOutcome {
    /// Number of reader filters that matched DataSetMessages.
    pub matched_readers: usize,
    /// Number of readers whose targets were updated.
    pub applied_readers: usize,
    /// Number of reader filters that rejected DataSetMessages.
    pub filtered_readers: usize,
    /// Datagram-level drop reason, if any.
    pub dropped_reason: Option<SubscriberError>,
}

/// Runtime receiver, dispatcher, target applier, and status store for DataSetReaders.
pub struct SubscriberRuntime {
    address_space: Arc<RwLock<AddressSpace>>,
    reader_groups: Vec<ReaderGroupConfig>,
    statuses: HashMap<u16, DataSetReaderStatus>,
    timeouts: HashMap<u16, Duration>,
    metadata_major_versions: HashMap<u16, Option<u32>>,
}

impl SubscriberRuntime {
    /// Builds a subscriber runtime from connection configs.
    pub fn with_connections(
        address_space: Arc<RwLock<AddressSpace>>,
        connections: Vec<PubSubConnectionConfig>,
    ) -> Result<Self, StatusCode> {
        for connection in &connections {
            connection.validate_subscriber_config()?;
        }

        Ok(Self::from_connections(address_space, connections))
    }

    /// Builds a subscriber runtime after validating only its DataSetReaders.
    ///
    /// Callers must validate connection-level invariants, including transport
    /// addresses and ReaderGroup settings, before calling this constructor.
    /// [`with_connections`] and [`PubSubConnectionConfig::validate_subscriber_config`]
    /// perform complete subscriber configuration validation.
    pub(crate) fn with_reader_validated_connections(
        address_space: Arc<RwLock<AddressSpace>>,
        connections: Vec<PubSubConnectionConfig>,
    ) -> Result<Self, StatusCode> {
        for connection in &connections {
            connection.validate_subscriber_readers()?;
        }

        Ok(Self::from_connections(address_space, connections))
    }

    pub(crate) fn from_connections(
        address_space: Arc<RwLock<AddressSpace>>,
        connections: Vec<PubSubConnectionConfig>,
    ) -> Self {
        let mut reader_groups = Vec::new();
        let mut statuses = HashMap::new();
        let mut timeouts = HashMap::new();
        let mut metadata_major_versions = HashMap::new();

        for connection in connections {
            for reader_group in connection.reader_groups {
                for reader in &reader_group.dataset_readers {
                    statuses
                        .entry(reader.dataset_reader_id)
                        .or_insert_with(DataSetReaderStatus::default);
                    if let Some(timeout) = reader.message_receive_timeout {
                        timeouts.insert(reader.dataset_reader_id, timeout);
                    }
                    metadata_major_versions
                        .insert(reader.dataset_reader_id, reader.metadata_major_version);
                }
                reader_groups.push(reader_group);
            }
        }

        Self {
            address_space,
            reader_groups,
            statuses,
            timeouts,
            metadata_major_versions,
        }
    }

    /// Returns a reader status snapshot.
    #[must_use]
    pub fn reader_status(&self, reader_id: u16) -> Option<DataSetReaderStatus> {
        self.statuses.get(&reader_id).cloned()
    }

    /// Records a security failure for specific reader ids.
    pub(crate) fn record_security_failure_for_readers(&mut self, reader_ids: &[u16]) {
        for reader_id in reader_ids {
            if let Some(status) = self.statuses.get_mut(reader_id) {
                status.security_failure_count += 1;
                status.last_error = None;
            }
        }
    }

    /// Checks MessageReceiveTimeout and pending metadata-version mismatch deadlines.
    pub fn check_timeouts_at(&mut self, now: Instant) {
        for (reader_id, status) in &mut self.statuses {
            let Some(timeout) = self.timeouts.get(reader_id).copied() else {
                continue;
            };

            if let Some(mismatch_since) = status.metadata_mismatch_since {
                if now.duration_since(mismatch_since) >= timeout {
                    status.state = PubSubState::Error;
                    status.last_error = Some(SubscriberError::MetadataMajorVersionMismatch);
                    continue;
                }
            }

            if status.state == PubSubState::Operational {
                if let Some(last_receive_time) = status.last_receive_time {
                    if now.duration_since(last_receive_time) >= timeout {
                        status.state = PubSubState::Error;
                        status.last_error = Some(SubscriberError::MessageReceiveTimeout);
                        status.timeout_count += 1;
                    }
                }
            }
        }
    }

    /// Observes a received metadata major version for one reader.
    pub fn observe_metadata_major_version_at(
        &mut self,
        reader_id: u16,
        observed_major_version: u32,
        now: Instant,
    ) -> Result<(), StatusCode> {
        let Some(configured_major_version) = self.metadata_major_versions.get(&reader_id).copied()
        else {
            return Err(StatusCode::BadNotFound);
        };
        let Some(status) = self.statuses.get_mut(&reader_id) else {
            return Err(StatusCode::BadNotFound);
        };

        if matches!(configured_major_version, Some(configured) if configured != observed_major_version)
        {
            status.metadata_mismatch_since.get_or_insert(now);
        } else {
            status.metadata_mismatch_since = None;
        }

        Ok(())
    }

    fn apply_reader(
        &mut self,
        reader: &DataSetReaderConfig,
        dataset_message: &UadpDataSetMessage,
        now: Instant,
    ) -> Result<(), SubscriberError> {
        let targets = reader.effective_target_variables();
        if targets.len() != dataset_message.fields.len() {
            return Err(SubscriberError::FieldCountMismatch);
        }

        let mut writes = Vec::with_capacity(targets.len());
        for target in &targets {
            validate_target_config(target)?;
            let Some(field) = dataset_message.fields.get(target.dataset_field_index) else {
                return Err(SubscriberError::FieldCountMismatch);
            };
            writes.push((target.target_node_id.clone(), field.clone()));
        }

        {
            let space = self.address_space.read();
            for (target_node_id, _) in &writes {
                let Some(node) = space.find(target_node_id) else {
                    return Err(SubscriberError::TargetNotFound);
                };
                if !matches!(&*node, NodeType::Variable(_)) {
                    return Err(SubscriberError::TargetNotVariable);
                }
            }
        }

        {
            let space = self.address_space.write();
            for (target_node_id, value) in writes {
                let Some(mut node) = space.find_mut(&target_node_id) else {
                    return Err(SubscriberError::TargetNotFound);
                };
                let NodeType::Variable(variable) = &mut *node else {
                    return Err(SubscriberError::TargetNotVariable);
                };
                variable.set_data_value(DataValue::value_only(value));
            }
        }

        if let Some(status) = self.statuses.get_mut(&reader.dataset_reader_id) {
            let was_operational = status.state == PubSubState::Operational;
            let is_new = update_sequence_status(status, dataset_message.sequence_number);
            status.state = PubSubState::Operational;
            if !was_operational || is_new {
                status.last_receive_time = Some(now);
            }
            status.last_error = None;
            status.metadata_mismatch_since = None;
            status.accepted_count += 1;
        }

        Ok(())
    }

    fn apply_json_reader(
        &mut self,
        reader: &DataSetReaderConfig,
        dataset_msg: &JsonDataSetMessage,
        now: Instant,
    ) -> Result<(), SubscriberError> {
        // JSON payload is a map of field names to values. Deterministic field
        // ordering is established by sorting keys so dataset_field_index maps
        // consistently to the same position.
        let mut keys: Vec<&String> = dataset_msg.payload.keys().collect();
        keys.sort();
        let fields: Vec<Variant> = keys
            .iter()
            .map(|k| json_value_to_variant(&dataset_msg.payload[*k]))
            .collect();

        let synthetic = UadpDataSetMessage {
            dataset_writer_id: dataset_msg.dataset_writer_id,
            sequence_number: dataset_msg.sequence_number,
            timestamp: None,
            status: None,
            fields,
        };

        self.apply_reader(reader, &synthetic, now)
    }
}

/// Bind a decoded NetworkMessage's DataSets into the address space via matching DataSetReaders.
///
/// Returns the number of DataSetMessages applied.
pub fn apply_network_message(
    address_space: &AddressSpace,
    message: &UadpNetworkMessage,
    reader_groups: &[ReaderGroupConfig],
) -> usize {
    let mut applied = 0;

    for dataset_message in &message.dataset_messages {
        let Some(reader) = find_reader(reader_groups, message, dataset_message) else {
            continue;
        };

        let targets = reader.effective_target_variables();
        if targets.len() != dataset_message.fields.len() {
            continue;
        }

        for target in targets {
            let Some(field) = dataset_message.fields.get(target.dataset_field_index) else {
                continue;
            };
            if let Some(mut node) = address_space.find_mut(&target.target_node_id) {
                if let NodeType::Variable(variable) = &mut *node {
                    variable.set_data_value(DataValue::value_only(field.clone()));
                }
            }
        }

        applied += 1;
    }

    applied
}

/// Decode a UADP NetworkMessage and apply matching DataSets to target Variables.
pub fn decode_and_apply(
    address_space: &AddressSpace,
    payload: &[u8],
    ctx: &Context<'_>,
    reader_groups: &[ReaderGroupConfig],
) -> Result<usize, StatusCode> {
    let message =
        UadpNetworkMessage::decode(&mut &payload[..], ctx).map_err(|error| error.status())?;
    Ok(apply_network_message(
        address_space,
        &message,
        reader_groups,
    ))
}

fn find_reader<'a>(
    reader_groups: &'a [ReaderGroupConfig],
    message: &UadpNetworkMessage,
    dataset_message: &UadpDataSetMessage,
) -> Option<&'a DataSetReaderConfig> {
    reader_groups
        .iter()
        .flat_map(|reader_group| reader_group.dataset_readers.iter())
        .find(|reader| reader_matches(reader, message, dataset_message))
}

fn reader_matches(
    reader: &DataSetReaderConfig,
    message: &UadpNetworkMessage,
    dataset_message: &UadpDataSetMessage,
) -> bool {
    publisher_matches(reader.publisher_id.as_ref(), &message.publisher_id)
        && optional_u16_matches(reader.writer_group_id, message.writer_group_id)
        && optional_u16_matches(
            reader.network_message_number,
            message.network_message_number,
        )
        && dataset_writer_matches(reader.dataset_writer_id, dataset_message.dataset_writer_id)
}

fn publisher_matches(expected: Option<&PublisherId>, actual: &PublisherId) -> bool {
    match expected {
        None | Some(PublisherId::None) => true,
        Some(PublisherId::Byte(0))
        | Some(PublisherId::UInt16(0))
        | Some(PublisherId::UInt32(0))
        | Some(PublisherId::UInt64(0)) => true,
        Some(PublisherId::String(value)) if value.is_empty() => true,
        Some(expected) => expected == actual,
    }
}

/// Matches a configured reader against a decoded JSON NetworkMessage.
///
/// JSON messages (OPC-10000-14 §7.2.5.4) carry `PublisherId` as a string and
/// `WriterGroupId`/`DataSetWriterId` as integers, so the filter logic differs
/// slightly from the UADP binary path.
fn json_reader_matches(
    reader: &DataSetReaderConfig,
    message: &JsonNetworkMessage,
    dataset_msg: &JsonDataSetMessage,
) -> bool {
    json_publisher_matches(reader.publisher_id.as_ref(), &message.publisher_id)
        && optional_u16_matches(reader.writer_group_id, message.writer_group_id)
        && dataset_writer_matches(reader.dataset_writer_id, dataset_msg.dataset_writer_id)
}

fn json_publisher_matches(expected: Option<&PublisherId>, actual: &str) -> bool {
    match expected {
        None | Some(PublisherId::None) => true,
        Some(PublisherId::Byte(0))
        | Some(PublisherId::UInt16(0))
        | Some(PublisherId::UInt32(0))
        | Some(PublisherId::UInt64(0)) => true,
        Some(PublisherId::String(value)) if value.is_empty() => true,
        Some(expected) => publisher_id_to_json_string(expected) == actual,
    }
}

fn publisher_id_to_json_string(id: &PublisherId) -> String {
    match id {
        PublisherId::None => String::new(),
        PublisherId::Byte(v) => v.to_string(),
        PublisherId::UInt16(v) => v.to_string(),
        PublisherId::UInt32(v) => v.to_string(),
        PublisherId::UInt64(v) => v.to_string(),
        PublisherId::String(v) => v.clone(),
    }
}

/// Converts a raw JSON value into a best-effort OPC-UA `Variant`.
///
/// JSON DataSetMessage payloads (OPC-10000-14 §7.2.5.4.3) may contain plain
/// JSON primitives. Numeric values are mapped to `Double` to match the
/// loose typing of JSON numbers.
fn json_value_to_variant(value: &serde_json::Value) -> Variant {
    match value {
        serde_json::Value::Null => Variant::Empty,
        serde_json::Value::Bool(b) => Variant::Boolean(*b),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(f) => Variant::Double(f),
            None => Variant::Empty,
        },
        serde_json::Value::String(s) => Variant::String(UAString::from(s.as_str())),
        _ => Variant::Empty,
    }
}

fn optional_u16_matches(expected: Option<u16>, actual: u16) -> bool {
    match expected {
        Some(expected) => expected == actual,
        None => true,
    }
}

fn dataset_writer_matches(expected: u16, actual: u16) -> bool {
    expected == 0 || expected == actual
}

fn validate_target_config(target: &FieldTargetConfig) -> Result<(), SubscriberError> {
    if target.attribute_id != AttributeId::Value
        || matches!(target.index_range.as_deref(), Some(range) if !range.is_empty())
    {
        return Err(SubscriberError::UnsupportedTarget);
    }
    Ok(())
}

fn update_sequence_status(status: &mut DataSetReaderStatus, sequence_number: u16) -> bool {
    let is_new = match status.last_sequence_number {
        None => true,
        Some(last) => {
            let last = last as u16;
            if sequence_number == last {
                status.duplicate_count += 1;
                false
            } else {
                let expected = last.wrapping_add(1);
                if sequence_number != expected {
                    let forward_distance = sequence_number.wrapping_sub(last);
                    if forward_distance < (u16::MAX / 2) {
                        status.sequence_gap_count += 1;
                    } else {
                        status.out_of_order_count += 1;
                    }
                }
                true
            }
        }
    };

    status.last_sequence_number = Some(sequence_number as u64);
    is_new
}
