use std::time::{Duration, Instant};

use opcua_types::PubSubState;

use crate::config::DataSetReaderConfig;

use super::SubscriberError;

/// Connection-scoped identity of one configured DataSetReader.
///
/// The (connection_id, dataset_reader_id) pair is a runtime identity design
/// that prevents cross-connection collisions: the same numeric DataSetReader id
/// on two distinct connections cannot share runtime state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataSetReaderKey {
    /// Id of the connection that owns the DataSetReader.
    pub connection_id: String,
    /// Numeric id of the DataSetReader within its connection.
    pub dataset_reader_id: u16,
}

impl DataSetReaderKey {
    /// Creates a connection-scoped DataSetReader key.
    #[must_use]
    pub fn new(connection_id: impl Into<String>, dataset_reader_id: u16) -> Self {
        Self {
            connection_id: connection_id.into(),
            dataset_reader_id,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BoundDataSetReader {
    pub(crate) key: DataSetReaderKey,
    pub(crate) config: DataSetReaderConfig,
}

impl BoundDataSetReader {
    pub(crate) fn new(connection_id: &str, config: DataSetReaderConfig) -> Self {
        let key = DataSetReaderKey::new(connection_id, config.dataset_reader_id);
        Self { key, config }
    }
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

pub(crate) struct DataSetReaderRuntimeRecord {
    pub(crate) status: DataSetReaderStatus,
    pub(crate) message_receive_timeout: Option<Duration>,
    pub(crate) metadata_major_version: Option<u32>,
}

impl DataSetReaderRuntimeRecord {
    pub(crate) fn new(config: &DataSetReaderConfig) -> Self {
        Self {
            status: DataSetReaderStatus::default(),
            message_receive_timeout: config.message_receive_timeout,
            metadata_major_version: config.metadata_major_version,
        }
    }
}

pub(crate) fn update_sequence_status(
    status: &mut DataSetReaderStatus,
    sequence_number: u16,
) -> bool {
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
