//! UADP subscriber runtime for applying received DataSet fields to Variables.

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    sync::Arc,
    time::Instant,
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
    config::{
        reader_groups_require_security, DataSetReaderConfig, FieldTargetConfig,
        PubSubConnectionConfig, ReaderGroupConfig,
    },
};

mod reader;
mod routing;

pub(crate) use reader::{update_sequence_status, BoundDataSetReader, DataSetReaderRuntimeRecord};
pub use reader::{DataSetReaderKey, DataSetReaderStatus};

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
    connection_ids: HashSet<String>,
    secured_connection_ids: HashSet<String>,
    readers: Vec<Arc<BoundDataSetReader>>,
    reader_records: HashMap<DataSetReaderKey, DataSetReaderRuntimeRecord>,
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

        Self::from_connections(address_space, connections)
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

        Self::from_connections(address_space, connections)
    }

    pub(crate) fn from_connections(
        address_space: Arc<RwLock<AddressSpace>>,
        connections: Vec<PubSubConnectionConfig>,
    ) -> Result<Self, StatusCode> {
        let mut readers = Vec::new();
        let mut reader_records = HashMap::new();
        let mut connection_ids = HashSet::new();
        let mut secured_connection_ids = HashSet::new();

        for connection in connections {
            let is_secured = connection.validated_subscriber_security()?.is_some();
            let connection_id = connection.connection_id;
            if !connection_ids.insert(connection_id.clone()) {
                return Err(StatusCode::BadConfigurationError);
            }
            if is_secured {
                secured_connection_ids.insert(connection_id.clone());
            }
            for reader_group in connection.reader_groups {
                for reader in reader_group.dataset_readers {
                    let bound_reader = Arc::new(BoundDataSetReader::new(&connection_id, reader));
                    match reader_records.entry(bound_reader.key.clone()) {
                        Entry::Occupied(_) => return Err(StatusCode::BadConfigurationError),
                        Entry::Vacant(entry) => {
                            entry.insert(DataSetReaderRuntimeRecord::new(&bound_reader.config));
                        }
                    }
                    readers.push(bound_reader);
                }
            }
        }

        Ok(Self {
            address_space,
            connection_ids,
            secured_connection_ids,
            readers,
            reader_records,
        })
    }

    /// Returns a reader status snapshot by its connection-scoped identity.
    ///
    /// The key pairs connection_id with dataset_reader_id to prevent
    /// cross-connection collisions when numeric reader ids repeat.
    #[must_use]
    pub fn reader_status_by_key(&self, key: &DataSetReaderKey) -> Option<DataSetReaderStatus> {
        self.reader_records
            .get(key)
            .map(|record| record.status.clone())
    }

    /// Returns a status by numeric id only when exactly one configured reader has that id.
    ///
    /// Numeric ids reused by multiple connections are ambiguous and return
    /// `None`; callers should use [`Self::reader_status_by_key`].
    #[must_use]
    pub fn reader_status(&self, reader_id: u16) -> Option<DataSetReaderStatus> {
        let key = self.unique_key_for_reader_id(reader_id)?;
        self.reader_status_by_key(key)
    }

    pub(crate) fn record_security_failure_for_readers(&mut self, reader_keys: &[DataSetReaderKey]) {
        for reader_key in reader_keys {
            if let Some(record) = self.reader_records.get_mut(reader_key) {
                record.status.security_failure_count += 1;
            }
        }
    }

    /// Checks MessageReceiveTimeout and pending metadata-version mismatch deadlines.
    pub fn check_timeouts_at(&mut self, now: Instant) {
        for record in self.reader_records.values_mut() {
            let Some(timeout) = record.message_receive_timeout else {
                continue;
            };
            let status = &mut record.status;

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
        let key = self
            .unique_key_for_reader_id(reader_id)
            .cloned()
            .ok_or(StatusCode::BadNotFound)?;
        self.observe_metadata_major_version_for_key_at(&key, observed_major_version, now)
    }

    /// Observes a received metadata major version for one connection-scoped reader.
    pub fn observe_metadata_major_version_for_key_at(
        &mut self,
        key: &DataSetReaderKey,
        observed_major_version: u32,
        now: Instant,
    ) -> Result<(), StatusCode> {
        let record = self
            .reader_records
            .get_mut(key)
            .ok_or(StatusCode::BadNotFound)?;

        if matches!(record.metadata_major_version, Some(configured) if configured != observed_major_version)
        {
            record.status.metadata_mismatch_since.get_or_insert(now);
        } else {
            record.status.metadata_mismatch_since = None;
        }

        Ok(())
    }

    fn unique_key_for_reader_id(&self, reader_id: u16) -> Option<&DataSetReaderKey> {
        let mut matches = self
            .reader_records
            .keys()
            .filter(|key| key.dataset_reader_id == reader_id);
        let key = matches.next()?;
        matches.next().is_none().then_some(key)
    }

    fn apply_reader(
        &mut self,
        reader: &BoundDataSetReader,
        dataset_message: &UadpDataSetMessage,
        now: Instant,
    ) -> Result<(), SubscriberError> {
        let targets = reader.config.effective_target_variables();
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

        if let Some(record) = self.reader_records.get_mut(&reader.key) {
            let status = &mut record.status;
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
        reader: &BoundDataSetReader,
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
/// Callers are responsible for verifying, decrypting, and replay-checking the
/// message before calling this trusted decoded-message boundary.
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
    if reader_groups_require_security(reader_groups)? {
        return Err(StatusCode::BadSecurityChecksFailed);
    }

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
