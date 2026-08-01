use std::{sync::Arc, time::Instant};

use opcua_types::{BinaryDecodable, Context, StatusCode};

use crate::{
    codec::{
        json::{decode_network_message, JsonNetworkMessage},
        uadp::UadpNetworkMessage,
    },
    config::MessageEncoding,
    transport::udp::is_custom_fragment_datagram,
};

use super::{
    BoundDataSetReader, JsonReaderMessage, SubscriberApplyOutcome, SubscriberError,
    SubscriberRuntime, UadpReaderMessage,
};

impl SubscriberRuntime {
    pub(crate) fn process_datagram_for_readers(
        &mut self,
        readers: &[Arc<BoundDataSetReader>],
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let mut outcome = SubscriberApplyOutcome::default();
        let mut decoded = false;
        let mut decode_error = None;

        if readers
            .iter()
            .any(|reader| reader.config.message_encoding == MessageEncoding::Uadp)
        {
            if is_custom_fragment_datagram(payload) {
                for reader in readers
                    .iter()
                    .filter(|reader| reader.config.message_encoding == MessageEncoding::Uadp)
                {
                    self.record_drop_for_reader(&reader.key, SubscriberError::UnsupportedTarget);
                }
                decode_error = Some(StatusCode::BadNotSupported);
            } else {
                match UadpNetworkMessage::decode(&mut &payload[..], ctx) {
                    Ok(message) => {
                        decoded = true;
                        outcome.accumulate(
                            self.process_network_message_for_readers(readers, &message)?,
                        );
                    }
                    Err(error) => decode_error = Some(error.status()),
                }
            }
        }

        if readers
            .iter()
            .any(|reader| reader.config.message_encoding == MessageEncoding::Json)
        {
            match decode_network_message(payload) {
                Ok(message) => {
                    decoded = true;
                    outcome.accumulate(self.process_json_message_for_readers(readers, &message));
                }
                Err(error) => decode_error = Some(error.into()),
            }
        }

        if decoded || readers.is_empty() {
            Ok(outcome)
        } else {
            Err(decode_error.unwrap_or(StatusCode::BadDecodingError))
        }
    }

    pub(crate) fn process_network_message_for_readers(
        &mut self,
        readers: &[Arc<BoundDataSetReader>],
        message: &UadpNetworkMessage,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        self.process_network_message_for_readers_at(readers, message, Instant::now())
    }

    pub(crate) fn process_network_message_for_readers_at(
        &mut self,
        readers: &[Arc<BoundDataSetReader>],
        message: &UadpNetworkMessage,
        received_at: Instant,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let mut outcome = SubscriberApplyOutcome::default();
        for dataset_message in &message.dataset_messages {
            for reader in readers
                .iter()
                .filter(|reader| reader.config.message_encoding == MessageEncoding::Uadp)
            {
                outcome.accumulate(self.process_uadp_reader_message(UadpReaderMessage {
                    reader,
                    network_message: message,
                    dataset_message,
                    received_at,
                }));
            }
        }
        Ok(outcome)
    }

    fn process_json_message_for_readers(
        &mut self,
        readers: &[Arc<BoundDataSetReader>],
        message: &JsonNetworkMessage,
    ) -> SubscriberApplyOutcome {
        let mut outcome = SubscriberApplyOutcome::default();
        let received_at = Instant::now();
        for dataset_message in &message.messages {
            for reader in readers
                .iter()
                .filter(|reader| reader.config.message_encoding == MessageEncoding::Json)
            {
                outcome.accumulate(self.process_json_reader_message(JsonReaderMessage {
                    reader,
                    network_message: message,
                    dataset_message,
                    received_at,
                }));
            }
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use opcua_core::sync::RwLock;
    use opcua_server::address_space::AddressSpace;

    use crate::config::{DataSetReaderConfig, PubSubConnectionConfig, ReaderGroupConfig};

    use super::*;

    #[test]
    fn reader_selection_shares_bound_reader_configuration() {
        // Given: a runtime that owns one connection-scoped reader configuration.
        let connection = PubSubConnectionConfig {
            connection_id: "connection".to_string(),
            name: "connection".to_string(),
            address: "udp://127.0.0.1:4840".to_string(),
            writer_groups: Vec::new(),
            reader_groups: vec![ReaderGroupConfig {
                dataset_readers: vec![DataSetReaderConfig {
                    name: Some("reader".to_string()),
                    dataset_reader_id: 1,
                    ..DataSetReaderConfig::default()
                }],
                ..ReaderGroupConfig::default()
            }],
        };
        let runtime = SubscriberRuntime::with_reader_validated_connections(
            Arc::new(RwLock::new(AddressSpace::new())),
            vec![connection],
        )
        .unwrap();

        // When: routing selects readers for that connection.
        let selected = runtime.readers_for_connection("connection").unwrap();

        // Then: the selected reader shares the runtime-owned configuration allocation.
        assert!(std::ptr::eq(
            &runtime.readers[0].config,
            &selected[0].config
        ));
    }
}
