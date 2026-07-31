use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_types::ContextOwned;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{config::DataSetReaderConfig, subscriber::SubscriberRuntime};

pub(super) struct MqttForwarder {
    pub(super) runtime: Arc<RwLock<SubscriberRuntime>>,
    pub(super) reader: DataSetReaderConfig,
    pub(super) payload_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pub(super) cancel: CancellationToken,
    pub(super) connection_id: String,
    pub(super) topic: String,
}

impl MqttForwarder {
    pub(super) fn spawn(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let ctx_owned = ContextOwned::default();
            let ctx = ctx_owned.context();
            // inv: every previously dequeued payload was dispatched at most once to only the
            // captured reader; once cancellation is ready, no further payload is dequeued.
            loop {
                tokio::select! {
                    biased;
                    _ = self.cancel.cancelled() => break,
                    payload = self.payload_rx.recv() => {
                        let Some(payload) = payload else { break };
                        if let Err(status) = self
                            .runtime
                            .write()
                            .process_datagram_for_reader(&self.reader, &payload, &ctx)
                        {
                            tracing::debug!(
                                ?status,
                                connection_id = %self.connection_id,
                                topic = %self.topic,
                                reader_id = self.reader.dataset_reader_id,
                                "dropped PubSub subscriber MQTT payload"
                            );
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
