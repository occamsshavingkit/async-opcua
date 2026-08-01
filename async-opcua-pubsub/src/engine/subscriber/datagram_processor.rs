use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_types::{Context, StatusCode};

use super::super::security::SubscriberSecurityProcessor;
use crate::{
    subscriber::{BoundDataSetReader, DataSetReaderKey, SubscriberApplyOutcome, SubscriberRuntime},
    DataSetReaderConfig,
};

/// Prepared subscriber datagram path owned by one live ingress task.
pub(in crate::engine) struct SubscriberDatagramProcessor {
    runtime: Arc<RwLock<SubscriberRuntime>>,
    readers: Vec<Arc<BoundDataSetReader>>,
    reader_keys: Vec<DataSetReaderKey>,
    security: Option<SubscriberSecurityProcessor>,
}

impl SubscriberDatagramProcessor {
    pub(in crate::engine) fn new(
        runtime: Arc<RwLock<SubscriberRuntime>>,
        connection_id: &str,
        readers: Vec<DataSetReaderConfig>,
        security: Option<SubscriberSecurityProcessor>,
    ) -> Self {
        let readers = readers
            .into_iter()
            .map(|reader| Arc::new(BoundDataSetReader::new(connection_id, reader)))
            .collect::<Vec<_>>();
        let reader_keys = readers.iter().map(|reader| reader.key.clone()).collect();
        Self {
            runtime,
            readers,
            reader_keys,
            security,
        }
    }

    pub(in crate::engine) fn process(
        &mut self,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode> {
        let Some(security) = &mut self.security else {
            return self
                .runtime
                .write()
                .process_datagram_for_readers(&self.readers, payload, ctx);
        };
        match security.decode(payload, ctx) {
            Ok(message) => self
                .runtime
                .write()
                .process_network_message_for_readers(&self.readers, &message),
            Err(status) => {
                self.runtime
                    .write()
                    .record_security_failure_for_readers(&self.reader_keys);
                Err(status)
            }
        }
    }
}
