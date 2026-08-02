use opcua_types::ContextOwned;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    transport::mqtt::{
        quality_of_service, start_mqtt_subscriber_with_config, MqttBrokerAddress,
        MqttSubscriberConfig,
    },
    MqttDeliveryGuarantee, PubSubConnectionConfig,
};

use super::{subscriber::SubscriberDatagramProcessor, DatagramQueue, PubSubEngine};

struct MqttForwarder {
    processor: SubscriberDatagramProcessor,
    reader_id: u16,
    payload_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    cancel: CancellationToken,
    connection_id: String,
    topic: String,
}

impl MqttForwarder {
    fn spawn(mut self) -> JoinHandle<()> {
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
                        if let Err(status) = self.processor.process(&payload, &ctx) {
                            tracing::debug!(
                                ?status,
                                connection_id = %self.connection_id,
                                topic = %self.topic,
                                reader_id = self.reader_id,
                                "dropped PubSub subscriber MQTT payload"
                            );
                        }
                    }
                }
            }
        })
    }
}

impl PubSubEngine {
    /// Spawns MQTT broker subscriber tasks for each DataSetReader in the
    /// connection's ReaderGroups (OPC-10000-14 §6.4.2).
    ///
    /// Each DataSetReader maps to one MQTT topic subscription. The broker
    /// subscriber (`transport::mqtt::start_mqtt_subscriber`) forwards received
    /// payload bytes over an mpsc channel; a per-reader forwarder task drains
    /// that channel and hands each payload to the owning DataSetReader only.
    /// Broker connection failures are
    /// logged and retried with backoff inside the subscriber task, so they
    /// never crash the engine or abort the remaining readers.
    pub(super) fn spawn_mqtt_subscribers(
        &self,
        connection: PubSubConnectionConfig,
        broker_address: MqttBrokerAddress,
        processors: Vec<SubscriberDatagramProcessor>,
        cancel_token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        let connection_id = connection.connection_id.clone();
        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        let readers = connection.reader_groups.iter().flat_map(|reader_group| {
            reader_group
                .dataset_readers
                .iter()
                .cloned()
                .map(move |reader| (reader_group.reader_group_id, reader))
        });
        for ((reader_group_id, reader), processor) in readers.zip(processors) {
            let reader_id = reader.dataset_reader_id;
            let topic_filter = reader.mqtt_topic_filter(reader_group_id);
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
            // `try_send` and treats `Full` as `BadResourceUnavailable`
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

            handles.push(
                MqttForwarder {
                    processor,
                    reader_id,
                    payload_rx,
                    cancel: cancel_token.clone(),
                    connection_id: connection_id.clone(),
                    topic: topic_filter,
                }
                .spawn(),
            );
        }

        handles
    }
}

#[cfg(test)]
mod tests;
