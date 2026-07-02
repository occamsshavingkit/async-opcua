//! T022 (US2): Micro Embedded Device 2017 Server Profile — mandated-operation smoke.
//!
//! Grounding: Embedded DataChange Subscription Server Facet CUs (Monitor Basic,
//! Monitor Items 2, Monitor QueueSize_1, Monitor Value Change, Subscription Basic,
//! Subscription Minimum 1, Subscription Publish Min 02, PublishRequest Queue
//! Overflow) + "Session Minimum 2 Parallel". OPC 10000-4 §5.13/§5.14. See
//! specs/054-profile-polish/research-assets/PROFILES-2017.md.
//!
//! RED-FIRST: today the micro benchmark sample cannot run (no node manager, same 041
//! gap as nano) — red via connection-refused until T026 reworks the sample onto the
//! `micro` alias with the shared minimal ns0 + ticking demo counter.

#![cfg(feature = "profile-tests")]

mod common;

use std::{
    sync::{
        mpsc::{channel, RecvTimeoutError},
        Arc,
    },
    time::Duration,
};

use common::{connect, spawn_micro};

use async_opcua_foundation_profile_micro_server as micro;
use opcua::client::DataChangeCallback;
use opcua::types::{
    ExtensionObject, MonitoredItemCreateRequest, MonitoringMode, MonitoringParameters, NodeId,
    ReadValueId, StatusCode, TimestampsToReturn, Variant,
};

/// Monitor Basic / Monitor Value Change / Subscription Basic / Publish flow:
/// a data-change subscription on the ticking demo counter delivers notifications.
#[tokio::test]
async fn data_change_subscription_delivers_notifications() {
    let tester = spawn_micro().await;
    let session = connect(&tester).await;

    let (tx, rx) = channel::<(NodeId, Variant)>();
    let subscription_id = session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            8,
            0,
            true,
            DataChangeCallback::new(move |dv, item| {
                let _ = tx.send((
                    item.item_to_monitor().node_id.clone(),
                    dv.value.clone().unwrap_or_default(),
                ));
            }),
        )
        .await
        .expect("CreateSubscription must succeed on the micro build");

    let counter = micro::demo_counter_node_id();
    let results = session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Neither,
            vec![MonitoredItemCreateRequest::new(
                ReadValueId::from(counter.clone()),
                MonitoringMode::Reporting,
                MonitoringParameters {
                    client_handle: 1,
                    sampling_interval: 100.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            )],
        )
        .await
        .expect("CreateMonitoredItems must succeed");
    assert_eq!(results[0].result.status_code, StatusCode::Good);

    // The sample ticks the counter; at least two distinct notifications must flow.
    let mut seen = Vec::new();
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok((node, value)) => {
                assert_eq!(node, counter, "notification must be for the demo counter");
                seen.push(value);
            }
            Err(RecvTimeoutError::Timeout) => panic!(
                "expected data-change notifications from the ticking demo counter, got {seen:?}"
            ),
            Err(e) => panic!("notification channel failed: {e}"),
        }
    }
}

/// Session Minimum 2 Parallel: two concurrent sessions both operate.
#[tokio::test]
async fn two_parallel_sessions() {
    let tester = spawn_micro().await;
    let session_a = connect(&tester).await;
    let session_b = connect(&tester).await;

    for session in [&session_a, &session_b] {
        let read = read_counter(session, &micro::demo_counter_node_id()).await;
        assert_eq!(
            read,
            StatusCode::Good,
            "both parallel sessions must be able to read"
        );
    }
}

async fn read_counter(session: &Arc<opcua::client::Session>, node: &NodeId) -> StatusCode {
    let values = session
        .read(
            &[ReadValueId::from(node.clone())],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("Read must succeed at service level");
    values[0].status()
}
