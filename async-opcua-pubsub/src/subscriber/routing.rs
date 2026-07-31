use std::time::Instant;

use opcua_types::{BinaryDecodable, Context, PubSubState, StatusCode};

use crate::{
    codec::{
        json::{decode_network_message, JsonDataSetMessage, JsonNetworkMessage},
        uadp::{UadpDataSetMessage, UadpNetworkMessage},
    },
    config::{DataSetReaderConfig, MessageEncoding},
    transport::udp::is_custom_fragment_datagram,
};

use super::{
    json_reader_matches, reader_matches, SubscriberApplyOutcome, SubscriberError, SubscriberRuntime,
};

struct UadpReaderMessage<'a> {
    reader: &'a DataSetReaderConfig,
    network_message: &'a UadpNetworkMessage,
    dataset_message: &'a UadpDataSetMessage,
    received_at: Instant,
}

struct JsonReaderMessage<'a> {
    reader: &'a DataSetReaderConfig,
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
    /// Processes a datagram against one validated DataSetReader only.
    ///
    /// The returned outcome and all diagnostic or target mutations belong to
    /// `reader`; unrelated runtime readers are not consulted.
    pub(crate) fn process_datagram_for_reader(
        &mut self,
        reader: &DataSetReaderConfig,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        match &reader.message_encoding {
            MessageEncoding::Json => self.process_json_datagram_for_reader(reader, payload),
            MessageEncoding::Uadp => {
                if is_custom_fragment_datagram(payload) {
                    self.record_drop_for_reader(
                        reader.dataset_reader_id,
                        SubscriberError::UnsupportedTarget,
                    );
                    return Err(StatusCode::BadNotSupported);
                }

                let message = UadpNetworkMessage::decode(&mut &payload[..], ctx)
                    .map_err(|error| error.status())?;
                Ok(self.process_network_message_for_reader_at(reader, &message, Instant::now()))
            }
        }
    }

    /// Processes a plain UADP datagram.
    pub fn process_datagram(
        &mut self,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let any_json_reader = self
            .reader_groups
            .iter()
            .flat_map(|reader_group| reader_group.dataset_readers.iter())
            .any(|reader| reader.message_encoding == MessageEncoding::Json);

        if any_json_reader {
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

    fn process_json_datagram(
        &mut self,
        payload: &[u8],
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let message = decode_network_message(payload)?;
        let readers = self
            .reader_groups
            .iter()
            .flat_map(|reader_group| reader_group.dataset_readers.iter())
            .cloned()
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

    fn process_json_datagram_for_reader(
        &mut self,
        reader: &DataSetReaderConfig,
        payload: &[u8],
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let message = decode_network_message(payload)?;
        let mut outcome = SubscriberApplyOutcome::default();
        let received_at = Instant::now();

        for dataset_message in &message.messages {
            outcome.accumulate(self.process_json_reader_message(JsonReaderMessage {
                reader,
                network_message: &message,
                dataset_message,
                received_at,
            }));
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

    /// Processes an already decoded and verified UADP NetworkMessage at a supplied time.
    pub fn process_network_message_at(
        &mut self,
        message: &UadpNetworkMessage,
        now: Instant,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let readers = self
            .reader_groups
            .iter()
            .flat_map(|reader_group| reader_group.dataset_readers.iter())
            .cloned()
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

    fn process_network_message_for_reader_at(
        &mut self,
        reader: &DataSetReaderConfig,
        message: &UadpNetworkMessage,
        received_at: Instant,
    ) -> SubscriberApplyOutcome {
        let mut outcome = SubscriberApplyOutcome::default();

        for dataset_message in &message.dataset_messages {
            outcome.accumulate(self.process_uadp_reader_message(UadpReaderMessage {
                reader,
                network_message: message,
                dataset_message,
                received_at,
            }));
        }

        outcome
    }

    fn process_uadp_reader_message(
        &mut self,
        input: UadpReaderMessage<'_>,
    ) -> SubscriberApplyOutcome {
        if !reader_matches(input.reader, input.network_message, input.dataset_message) {
            if let Some(status) = self.statuses.get_mut(&input.reader.dataset_reader_id) {
                status.filtered_count += 1;
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
            Err(error) => self.record_application_error(input.reader.dataset_reader_id, error),
        }
        outcome
    }

    fn process_json_reader_message(
        &mut self,
        input: JsonReaderMessage<'_>,
    ) -> SubscriberApplyOutcome {
        if !json_reader_matches(input.reader, input.network_message, input.dataset_message) {
            if let Some(status) = self.statuses.get_mut(&input.reader.dataset_reader_id) {
                status.filtered_count += 1;
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
            Err(error) => self.record_application_error(input.reader.dataset_reader_id, error),
        }
        outcome
    }

    fn record_application_error(&mut self, reader_id: u16, error: SubscriberError) {
        if let Some(status) = self.statuses.get_mut(&reader_id) {
            status.last_error = Some(error);
            status.dropped_count += 1;
            status.state = PubSubState::Error;
        }
    }

    fn record_drop_for_reader(&mut self, reader_id: u16, error: SubscriberError) {
        if let Some(status) = self.statuses.get_mut(&reader_id) {
            status.dropped_count += 1;
            status.last_error = Some(error);
        }
    }

    fn record_drop_for_all(&mut self, error: SubscriberError) {
        for status in self.statuses.values_mut() {
            status.dropped_count += 1;
            status.last_error = Some(error);
        }
    }
}
