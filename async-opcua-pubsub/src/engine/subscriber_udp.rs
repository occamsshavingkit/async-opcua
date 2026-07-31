use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_types::{ContextOwned, StatusCode};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{DatagramQueue, PubSubEngine};
use crate::{subscriber::SubscriberRuntime, PubSubConnectionConfig};

impl PubSubEngine {
    /// Spawns a single UDP datagram receive loop for a broker-less PubSub
    /// connection (OPC-10000-14 §6.4.1).
    ///
    /// Received payloads are forwarded across a bounded
    /// [`DatagramQueue`] (OPC-10000-14 §9.1.10.1) to a consumer task that
    /// hands them to `SubscriberRuntime::process_datagram`. When the queue is
    /// full (processing can't keep up), the producer rejects the datagram with
    /// `StatusCode::BadTooManyPublishRequests` and drops it rather than
    /// blocking the receive loop or growing memory without bound.
    ///
    /// Returns the producer and consumer task handles so the engine can await
    /// both on shutdown.
    pub(super) fn spawn_udp_subscriber(
        &self,
        connection: PubSubConnectionConfig,
        socket: UdpSocket,
        runtime: Arc<RwLock<SubscriberRuntime>>,
        cancel_token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        let connection_id = connection.connection_id;
        let (queue, payload_rx) = DatagramQueue::new(self.datagram_queue_capacity);
        let mut handles = Vec::with_capacity(2);

        // Consumer task: drains the bounded queue and runs the (synchronous)
        // subscriber runtime processing. Exits on cancellation or once the
        // producer drops its sender and the queue drains.
        let consumer_runtime = runtime.clone();
        let consumer_cancel = cancel_token.clone();
        let consumer_connection_id = connection_id.clone();
        handles.push(tokio::spawn(async move {
            let mut payload_rx = payload_rx;
            loop {
                tokio::select! {
                    _ = consumer_cancel.cancelled() => break,
                    payload = payload_rx.recv() => {
                        let Some(payload) = payload else { break };
                        let ctx_owned = ContextOwned::default();
                        let ctx = ctx_owned.context();
                        if let Err(status) = consumer_runtime
                            .write()
                            .process_datagram(&payload, &ctx)
                        {
                            tracing::debug!(
                                ?status,
                                %consumer_connection_id,
                                "dropped PubSub subscriber UDP datagram"
                            );
                        }
                    }
                }
            }
        }));

        // Producer task: receives UDP datagrams and enqueues them on the
        // bounded queue. On `BadTooManyPublishRequests` the datagram is
        // dropped (logged) so the receive loop never blocks on a full queue.
        handles.push(tokio::spawn(async move {
            let mut buf = vec![0_u8; 65_535];

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    received = socket.recv_from(&mut buf) => {
                        match received {
                            Ok((len, _peer)) => {
                                if let Err(status) = queue.try_enqueue(buf[..len].to_vec()) {
                                    if status == StatusCode::BadTooManyPublishRequests {
                                        tracing::warn!(
                                            ?status,
                                            %connection_id,
                                            "PubSub subscriber UDP datagram rejected; \
                                             datagram queue full"
                                        );
                                    } else {
                                        tracing::debug!(
                                            ?status,
                                            %connection_id,
                                            "PubSub subscriber UDP datagram not enqueued; \
                                             queue closed"
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                tracing::warn!(
                                    ?error,
                                    %connection_id,
                                    "failed to receive PubSub subscriber UDP datagram"
                                );
                            }
                        }
                    }
                }
            }
        }));

        handles
    }
}
