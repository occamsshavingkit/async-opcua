//! Coordinator ownership regressions for the WebSocket publisher.

use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    MessageEncoding, PubSubConnectionConfig, PubSubPublisher, WebSocketPublisher, WriterGroupConfig,
};
use opcua_server::address_space::AddressSpace;
use tokio_util::sync::CancellationToken;

const TEST_TIMEOUT: Duration = Duration::from_secs(1);

fn connection_config(connection_id: &str) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_owned(),
        name: connection_id.to_owned(),
        address: "ws://127.0.0.1:1/regression".to_owned(),
        writer_groups: vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 60_000,
            encoding: MessageEncoding::Json,
            dataset_writers: Vec::new(),
        }],
        reader_groups: Vec::new(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn aborting_coordinator_drops_writer_group_future_before_returning() {
    // Given: a coordinator that has started one writer-group future.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let publisher = WebSocketPublisher::new(Arc::clone(&address_space));
    let coordinator = publisher
        .start_publishing(
            connection_config("websocket-abort-drop-regression"),
            CancellationToken::new(),
        )
        .expect("WebSocket publisher should start");
    tokio::time::timeout(TEST_TIMEOUT, async {
        while Arc::strong_count(&address_space) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("WebSocket coordinator did not start the writer-group future");

    // When: the coordinator is aborted and awaited to completion.
    let observed_strong_count = tokio::spawn(async move {
        coordinator.abort();
        let join_error = coordinator
            .await
            .expect_err("coordinator completed instead of being aborted");
        assert!(join_error.is_cancelled(), "coordinator was not cancelled");
        Arc::strong_count(&address_space)
    })
    .await
    .expect("ownership observer task failed");

    // Then: no writer-group future retains the publisher's address space.
    assert_eq!(
        observed_strong_count, 2,
        "WebSocket writer-group future remained alive after the aborted coordinator returned"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn graceful_cancellation_exits_coordinator_cleanly() {
    // Given: a running WebSocket coordinator and its cancellation token.
    let publisher = WebSocketPublisher::new(Arc::new(RwLock::new(AddressSpace::new())));
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(
            connection_config("websocket-graceful-cancellation"),
            cancel_token.clone(),
        )
        .expect("WebSocket publisher should start");

    // When: graceful cancellation is requested.
    cancel_token.cancel();
    let result = tokio::time::timeout(TEST_TIMEOUT, coordinator)
        .await
        .expect("WebSocket coordinator did not stop before the deadline");

    // Then: the coordinator exits without a task failure.
    result.expect("WebSocket coordinator should shut down successfully");
}
