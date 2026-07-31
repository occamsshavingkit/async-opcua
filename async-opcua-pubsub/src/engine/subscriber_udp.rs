use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_types::{ContextOwned, StatusCode};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{DatagramQueue, PubSubEngine};
use crate::{subscriber::SubscriberRuntime, PubSubConnectionConfig};

const RECEIVE_RETRY_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Default)]
struct ReceiveErrorLogState {
    has_consecutive_error: bool,
}

impl ReceiveErrorLogState {
    fn on_receive_error(&mut self) -> tracing::Level {
        if self.has_consecutive_error {
            tracing::Level::DEBUG
        } else {
            self.has_consecutive_error = true;
            tracing::Level::WARN
        }
    }

    fn on_receive_success(&mut self) {
        self.has_consecutive_error = false;
    }
}

async fn wait_for_receive_retry(cancel_token: CancellationToken, retry_interval: Duration) {
    tokio::select! {
        _ = cancel_token.cancelled() => {}
        _ = tokio::time::sleep(retry_interval) => {}
    }
}

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
            let mut receive_error_state = ReceiveErrorLogState::default();

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    received = socket.recv_from(&mut buf) => {
                        match received {
                            Ok((len, _peer)) => {
                                receive_error_state.on_receive_success();
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
                                if receive_error_state.on_receive_error() == tracing::Level::WARN {
                                    tracing::warn!(
                                        ?error,
                                        %connection_id,
                                        "failed to receive PubSub subscriber UDP datagram"
                                    );
                                } else {
                                    tracing::debug!(
                                        ?error,
                                        %connection_id,
                                        "failed to receive PubSub subscriber UDP datagram"
                                    );
                                }
                                wait_for_receive_retry(
                                    cancel_token.clone(),
                                    RECEIVE_RETRY_INTERVAL,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        }));

        handles
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_for_receive_retry, ReceiveErrorLogState};
    use tokio::time::{timeout, Duration};
    use tokio_util::sync::CancellationToken;
    use tracing::Level;

    #[test]
    fn receive_error_warning_is_suppressed_until_success_resets_state() {
        // Given a receive-error state with no prior consecutive failures.
        let mut state = ReceiveErrorLogState::default();

        // When the first receive error is recorded.
        let first_error_level = state.on_receive_error();

        // Then it selects a warning.
        assert_eq!(first_error_level, Level::WARN);

        // When another receive error is recorded without a successful receive.
        let repeated_error_level = state.on_receive_error();

        // Then the repeated warning is suppressed at debug level.
        assert_eq!(repeated_error_level, Level::DEBUG);

        // When a receive succeeds and the next receive error is recorded.
        state.on_receive_success();
        let error_after_success_level = state.on_receive_error();

        // Then warning selection is reset for the new failure run.
        assert_eq!(error_after_success_level, Level::WARN);
    }

    #[tokio::test]
    async fn wait_for_receive_retry_paces_and_honors_cancellation() {
        // Given a retry interval and an uncancelled receive-loop token.
        let retry_interval = Duration::from_millis(50);
        let retry_token = CancellationToken::new();
        let mut retry = tokio::spawn(wait_for_receive_retry(retry_token.clone(), retry_interval));

        // When less than the retry interval has elapsed.
        let before_interval = timeout(Duration::from_millis(5), &mut retry).await;

        // Then the retry wait remains pending.
        assert!(before_interval.is_err());

        // Given another retry wait with a live cancellation token.
        let cancel_token = CancellationToken::new();
        let cancel_wait =
            tokio::spawn(wait_for_receive_retry(cancel_token.clone(), retry_interval));
        tokio::task::yield_now().await;

        // When cancellation is requested before the interval elapses.
        cancel_token.cancel();
        let completed = timeout(Duration::from_millis(100), cancel_wait).await;

        // Then the retry wait completes immediately.
        assert!(completed.is_ok());
    }
}
