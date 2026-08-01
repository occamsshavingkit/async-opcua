use std::{sync::Arc, time::Instant};

use opcua_types::{BinaryDecodable, Context, PubSubState, StatusCode};

use crate::{
    codec::{
        json::{decode_network_message, JsonDataSetMessage, JsonNetworkMessage},
        uadp::{UadpDataSetMessage, UadpNetworkMessage},
    },
    config::MessageEncoding,
    transport::udp::is_custom_fragment_datagram,
};

use super::{
    json_reader_matches, reader_matches, BoundDataSetReader, DataSetReaderKey,
    SubscriberApplyOutcome, SubscriberError, SubscriberRuntime,
};

#[path = "routing_batch.rs"]
mod batch;

struct UadpReaderMessage<'a> {
    reader: &'a BoundDataSetReader,
    network_message: &'a UadpNetworkMessage,
    dataset_message: &'a UadpDataSetMessage,
    received_at: Instant,
}

struct JsonReaderMessage<'a> {
    reader: &'a BoundDataSetReader,
    network_message: &'a JsonNetworkMessage,
    dataset_message: &'a JsonDataSetMessage,
    received_at: Instant,
}

impl SubscriberApplyOutcome {
    fn accumulate(&mut self, next: Self) {
        self.matched_readers += next.matched_readers;
        self.applied_readers += next.applied_readers;
        self.filtered_readers += next.filtered_readers;
        if self.dropped_reason.is_none() {
            self.dropped_reason = next.dropped_reason;
        }
    }
}

impl SubscriberRuntime {
    /// Processes a raw UADP datagram.
    ///
    /// Secured byte ingress must use [`PubSubEngine::process_subscriber_datagram`].
    pub fn process_datagram(
        &mut self,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        self.ensure_unscoped_ingress_is_unambiguous()?;
        let secured_connection_id = self
            .connection_ids
            .iter()
            .next()
            .filter(|connection_id| self.secured_connection_ids.contains(*connection_id))
            .cloned();
        if let Some(connection_id) = secured_connection_id {
            self.record_security_failure_for_connection(&connection_id);
            return Err(StatusCode::BadSecurityChecksFailed);
        }

        let is_json = payload
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b'{');

        if is_json {
            return self.process_json_datagram(payload);
        }

        if is_custom_fragment_datagram(payload) {
            self.record_drop_for_all(SubscriberError::UnsupportedTarget);
            return Err(StatusCode::BadNotSupported);
        }

        let message =
            UadpNetworkMessage::decode(&mut &payload[..], ctx).map_err(|error| error.status())?;
        self.process_network_message(&message)
    }

    /// Processes a raw encoded datagram for one configured connection.
    ///
    /// Secured byte ingress must use [`PubSubEngine::process_subscriber_datagram`].
    pub fn process_datagram_for_connection(
        &mut self,
        connection_id: &str,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let readers = self.readers_for_connection(connection_id)?;
        if self.secured_connection_ids.contains(connection_id) {
            self.record_security_failure_for_connection(connection_id);
            return Err(StatusCode::BadSecurityChecksFailed);
        }
        self.process_datagram_for_readers(&readers, payload, ctx)
    }

    fn process_json_datagram(
        &mut self,
        payload: &[u8],
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let message = decode_network_message(payload)?;
        let readers = self
            .readers
            .iter()
            .filter(|reader| reader.config.message_encoding == MessageEncoding::Json)
            .map(Arc::clone)
            .collect::<Vec<_>>();
        let mut outcome = SubscriberApplyOutcome::default();
        let received_at = Instant::now();

        for dataset_message in &message.messages {
            for reader in &readers {
                outcome.accumulate(self.process_json_reader_message(JsonReaderMessage {
                    reader,
                    network_message: &message,
                    dataset_message,
                    received_at,
                }));
            }
        }

        Ok(outcome)
    }

    /// Processes an already decoded and verified UADP NetworkMessage.
    pub fn process_network_message(
        &mut self,
        message: &UadpNetworkMessage,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        self.process_network_message_at(message, Instant::now())
    }

    /// Processes an already decoded and verified UADP NetworkMessage for one connection.
    pub fn process_network_message_for_connection(
        &mut self,
        connection_id: &str,
        message: &UadpNetworkMessage,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        self.process_network_message_for_connection_at(connection_id, message, Instant::now())
    }

    /// Processes an already decoded and verified UADP NetworkMessage at a supplied time.
    pub fn process_network_message_at(
        &mut self,
        message: &UadpNetworkMessage,
        now: Instant,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        self.ensure_unscoped_ingress_is_unambiguous()?;

        let readers = self
            .readers
            .iter()
            .filter(|reader| reader.config.message_encoding == MessageEncoding::Uadp)
            .map(Arc::clone)
            .collect::<Vec<_>>();
        let mut outcome = SubscriberApplyOutcome::default();

        for dataset_message in &message.dataset_messages {
            for reader in &readers {
                outcome.accumulate(self.process_uadp_reader_message(UadpReaderMessage {
                    reader,
                    network_message: message,
                    dataset_message,
                    received_at: now,
                }));
            }
        }

        Ok(outcome)
    }

    /// Processes an already decoded and verified UADP NetworkMessage for one connection at a supplied time.
    pub fn process_network_message_for_connection_at(
        &mut self,
        connection_id: &str,
        message: &UadpNetworkMessage,
        now: Instant,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let readers = self.readers_for_connection(connection_id)?;
        self.process_network_message_for_readers_at(&readers, message, now)
    }

    fn ensure_unscoped_ingress_is_unambiguous(&self) -> Result<(), StatusCode> {
        if self.connection_ids.len() > 1 {
            return Err(StatusCode::BadInvalidArgument);
        }
        Ok(())
    }

    pub(crate) fn record_security_failure_for_connection(&mut self, connection_id: &str) {
        for (key, record) in &mut self.reader_records {
            if key.connection_id == connection_id {
                record.status.security_failure_count += 1;
            }
        }
    }

    fn readers_for_connection(
        &self,
        connection_id: &str,
    ) -> Result<Vec<Arc<BoundDataSetReader>>, StatusCode> {
        if !self.connection_ids.contains(connection_id) {
            return Err(StatusCode::BadNotFound);
        }
        Ok(self
            .readers
            .iter()
            .filter(|reader| reader.key.connection_id == connection_id)
            .map(Arc::clone)
            .collect())
    }

    fn process_uadp_reader_message(
        &mut self,
        input: UadpReaderMessage<'_>,
    ) -> SubscriberApplyOutcome {
        if !reader_matches(
            &input.reader.config,
            input.network_message,
            input.dataset_message,
        ) {
            if let Some(record) = self.reader_records.get_mut(&input.reader.key) {
                record.status.filtered_count += 1;
            }
            return SubscriberApplyOutcome {
                filtered_readers: 1,
                ..SubscriberApplyOutcome::default()
            };
        }

        let mut outcome = SubscriberApplyOutcome {
            matched_readers: 1,
            ..SubscriberApplyOutcome::default()
        };
        match self.apply_reader(input.reader, input.dataset_message, input.received_at) {
            Ok(()) => outcome.applied_readers = 1,
            Err(error) => self.record_application_error(&input.reader.key, error),
        }
        outcome
    }

    fn process_json_reader_message(
        &mut self,
        input: JsonReaderMessage<'_>,
    ) -> SubscriberApplyOutcome {
        if !json_reader_matches(
            &input.reader.config,
            input.network_message,
            input.dataset_message,
        ) {
            if let Some(record) = self.reader_records.get_mut(&input.reader.key) {
                record.status.filtered_count += 1;
            }
            return SubscriberApplyOutcome {
                filtered_readers: 1,
                ..SubscriberApplyOutcome::default()
            };
        }

        let mut outcome = SubscriberApplyOutcome {
            matched_readers: 1,
            ..SubscriberApplyOutcome::default()
        };
        match self.apply_json_reader(input.reader, input.dataset_message, input.received_at) {
            Ok(()) => outcome.applied_readers = 1,
            Err(error) => self.record_application_error(&input.reader.key, error),
        }
        outcome
    }

    fn record_application_error(&mut self, key: &DataSetReaderKey, error: SubscriberError) {
        if let Some(record) = self.reader_records.get_mut(key) {
            record.status.last_error = Some(error);
            record.status.dropped_count += 1;
            record.status.state = PubSubState::Error;
        }
    }

    fn record_drop_for_reader(&mut self, key: &DataSetReaderKey, error: SubscriberError) {
        if let Some(record) = self.reader_records.get_mut(key) {
            record.status.dropped_count += 1;
            record.status.last_error = Some(error);
        }
    }

    fn record_drop_for_all(&mut self, error: SubscriberError) {
        for record in self.reader_records.values_mut() {
            record.status.dropped_count += 1;
            record.status.last_error = Some(error);
        }
    }
}
