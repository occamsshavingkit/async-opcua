use std::{future::Future, panic::AssertUnwindSafe, time::Duration};

use futures::{stream::FuturesUnordered, FutureExt, StreamExt};
use tokio_util::sync::CancellationToken;

/// MQTT transport driver implementation.
pub mod mqtt;

/// AMQP transport driver implementation.
pub mod amqp;

/// UDP multicast transport driver implementation.
pub mod udp;

/// WebSocket transport driver implementation.
pub mod websocket;

/// TSN transport driver implementation.
/// Experimental TSN transport. The AF_XDP socket is a simulated loopback
/// stub and scheduling shells out to `tc taprio`; gated behind the `tsn`
/// feature and not suitable for production use.
#[cfg(feature = "tsn")]
pub mod tsn;

pub(crate) async fn supervise_transport<F, W>(
    cancel_token: &CancellationToken,
    transport_loop: F,
    mut writer_futures: FuturesUnordered<W>,
) where
    F: Future<Output = ()>,
    W: Future<Output = ()>,
{
    tokio::pin!(transport_loop);
    let has_writer_futures = !writer_futures.is_empty();

    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {}
        _ = &mut transport_loop => {}
        writer_result = AssertUnwindSafe(writer_futures.next()).catch_unwind(), if has_writer_futures => {
            match writer_result {
                Ok(Some(())) => {
                    tracing::error!("transport writer future completed unexpectedly");
                }
                Ok(None) => {
                    tracing::error!("transport writer future set became empty unexpectedly");
                }
                Err(_) => {
                    tracing::error!("transport writer future panicked");
                }
            }
        }
    }
}

pub(crate) async fn wait_for_reconnect(cancel_token: &CancellationToken, backoff: &mut Duration) {
    tokio::select! {
        _ = cancel_token.cancelled() => {}
        _ = tokio::time::sleep(*backoff) => {
            *backoff = std::cmp::min(*backoff * 2, Duration::from_secs(60));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::{pending, Future},
        pin::Pin,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        task::Poll,
        time::Duration,
    };

    use futures::{
        future::{AbortHandle, Abortable},
        stream::FuturesUnordered,
    };
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use super::supervise_transport;

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);
    type WriterFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

    struct MarkDropped(Arc<AtomicBool>);

    impl Drop for MarkDropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    fn never_completing_writer(dropped: Arc<AtomicBool>) -> WriterFuture {
        let mark_dropped = MarkDropped(dropped);
        Box::pin(async move {
            let _mark_dropped = mark_dropped;
            pending::<()>().await;
        })
    }

    #[tokio::test]
    async fn cancellation_drops_pending_writer_future() {
        // Given
        let cancel_token = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let writer_futures = FuturesUnordered::<WriterFuture>::new();
        writer_futures.push(never_completing_writer(dropped.clone()));
        cancel_token.cancel();

        // When
        tokio::time::timeout(
            TEST_TIMEOUT,
            supervise_transport(&cancel_token, pending(), writer_futures),
        )
        .await
        .expect("transport supervisor timed out");

        // Then
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn empty_writer_set_waits_for_cancellation() {
        // Given
        let cancel_token = CancellationToken::new();
        let transport_started = Arc::new(Notify::new());
        let coordinator_cancel_token = cancel_token.clone();
        let coordinator_transport_started = transport_started.clone();
        let coordinator = tokio::spawn(async move {
            let transport_loop = async move {
                coordinator_transport_started.notify_one();
                pending::<()>().await;
            };
            supervise_transport(
                &coordinator_cancel_token,
                transport_loop,
                FuturesUnordered::<WriterFuture>::new(),
            )
            .await;
        });
        transport_started.notified().await;
        assert!(
            !coordinator.is_finished(),
            "empty writer set stopped the supervisor prematurely"
        );

        // When
        cancel_token.cancel();
        let result = tokio::time::timeout(TEST_TIMEOUT, coordinator)
            .await
            .expect("transport supervisor timed out");

        // Then
        result.expect("transport supervisor task failed");
    }

    #[tokio::test]
    async fn empty_writer_set_waits_for_transport_completion() {
        // Given
        let cancel_token = CancellationToken::new();
        let writer_futures = FuturesUnordered::<WriterFuture>::new();

        // When
        let result = tokio::time::timeout(
            TEST_TIMEOUT,
            supervise_transport(&cancel_token, async {}, writer_futures),
        )
        .await;

        // Then
        result.expect("transport supervisor ignored transport completion");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn externally_aborted_supervisor_drops_writer_future_before_returning() {
        // Given
        let cancel_token = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let writer_futures = FuturesUnordered::<WriterFuture>::new();
        writer_futures.push(never_completing_writer(dropped.clone()));

        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let mut supervisor = Box::pin(Abortable::new(
            supervise_transport(&cancel_token, pending(), writer_futures),
            abort_registration,
        ));
        assert_eq!(futures::poll!(supervisor.as_mut()), Poll::Pending);

        // When
        abort_handle.abort();
        let abort_result = supervisor.await;

        // Then
        assert!(abort_result.is_err(), "supervisor was not aborted");
        assert!(
            dropped.load(Ordering::Acquire),
            "writer future remained alive after the aborted supervisor returned"
        );
    }

    #[tokio::test]
    async fn writer_panic_drops_remaining_writer_futures() {
        // Given
        let cancel_token = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let writer_futures = FuturesUnordered::<WriterFuture>::new();
        writer_futures.push(never_completing_writer(dropped.clone()));
        writer_futures.push(Box::pin(async { panic!("writer panic") }));

        // When
        tokio::time::timeout(
            TEST_TIMEOUT,
            supervise_transport(&cancel_token, pending(), writer_futures),
        )
        .await
        .expect("transport supervisor timed out");

        // Then
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn writer_completion_drops_remaining_writer_futures() {
        // Given
        let cancel_token = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let writer_futures = FuturesUnordered::<WriterFuture>::new();
        writer_futures.push(never_completing_writer(dropped.clone()));
        writer_futures.push(Box::pin(async {}));

        // When
        tokio::time::timeout(
            TEST_TIMEOUT,
            supervise_transport(&cancel_token, pending(), writer_futures),
        )
        .await
        .expect("transport supervisor timed out");

        // Then
        assert!(dropped.load(Ordering::Acquire));
    }
}
