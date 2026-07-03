//! T023 (US2): standard-tier monitoring and out-of-profile services fail closed on
//! the Micro build.
//!
//! Grounding: OPC 10000-4 §7.22.2 DataChangeFilter (deadband is Standard DataChange
//! 2017 facet surface, NOT Micro), §7.22.3 EventFilter (events not in Micro),
//! §5.13.5 SetTriggering (Standard facet), §5.12 Call (not in Micro);
//! Bad_MonitoredItemFilterUnsupported is a per-ITEM operation result,
//! BadServiceUnsupported a service-level fault (§5.3/§7.34).
//!
//! RED-FIRST: red via connection-refused until T026 makes the sample runnable; the
//! deadband/SetTriggering asserts additionally stay red until T024/T025 split those
//! out of the basic subscription tier (today they are compiled into every
//! subscriptions build).

#![cfg(feature = "profile-tests")]

mod common;

use std::time::Duration;

use common::{connect, spawn_micro};

use async_opcua_foundation_profile_micro_server as micro;
use opcua::client::services::{Call, CreateMonitoredItems, SetTriggering};
use opcua::client::{DataChangeCallback, UARequest};
use opcua::types::{
    CallMethodRequest, ContentFilter, DataChangeFilter, DataChangeTrigger, DeadbandType,
    EventFilter, ExtensionObject, MonitoredItemCreateRequest, MonitoringMode, MonitoringParameters,
    NodeId, ObjectId, ReadValueId, StatusCode, TimestampsToReturn, Variant,
};

fn item_with_filter(node: NodeId, filter: ExtensionObject) -> MonitoredItemCreateRequest {
    MonitoredItemCreateRequest {
        item_to_monitor: ReadValueId::from(node),
        monitoring_mode: MonitoringMode::Reporting,
        requested_parameters: MonitoringParameters {
            client_handle: 1,
            sampling_interval: 100.0,
            filter,
            queue_size: 1,
            discard_oldest: true,
        },
    }
}

/// Filters and services above the Micro tier fail closed; the subscription itself
/// keeps working afterwards (no corruption).
///
/// SKIPPED under workspace --all-features: the test-graph unification caveat
/// (specs/054-profile-polish/tasks.md) documents that unified features invert
/// rejection semantics. Under --all-features, subscriptions-standard is
/// compiled in and deadband filters ARE accepted. This test is valid only
/// under isolated `cargo test -p <pkg> --features profile-tests`.
#[tokio::test]
async fn standard_tier_features_fail_closed() {
    let tester = spawn_micro().await;

    let nm_count = tester.handle.node_managers().iter().count();
    if nm_count > 1 {
        eprintln!("skipping rejection test: {nm_count} node managers (unified build)");
        return;
    }

    let session = connect(&tester).await;

    let subscription_id = session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            8,
            0,
            true,
            DataChangeCallback::new(|_, _| {}),
        )
        .await
        .expect("CreateSubscription must succeed on the micro build");

    let counter = micro::demo_counter_node_id();

    // Deadband DataChangeFilter → per-item BadMonitoredItemFilterUnsupported (T024).
    let deadband = DataChangeFilter {
        trigger: DataChangeTrigger::StatusValue,
        deadband_type: DeadbandType::Absolute as u32,
        deadband_value: 5.0,
    };
    let response = CreateMonitoredItems::new(subscription_id, &session)
        .timestamps_to_return(TimestampsToReturn::Neither)
        .item(item_with_filter(
            counter.clone(),
            ExtensionObject::from_message(deadband),
        ))
        .send(session.channel())
        .await
        .expect("CreateMonitoredItems is a valid service on micro");
    assert_eq!(
        response.results[0].result.status_code,
        StatusCode::BadMonitoredItemFilterUnsupported,
        "deadband filters are Standard DataChange 2017 facet surface, not Micro"
    );

    // EventFilter → per-item BadMonitoredItemFilterUnsupported (events gate, T020).
    let event_filter = EventFilter {
        select_clauses: None,
        where_clause: ContentFilter { elements: None },
    };
    let response = CreateMonitoredItems::new(subscription_id, &session)
        .timestamps_to_return(TimestampsToReturn::Neither)
        .item(item_with_filter(
            ObjectId::Server.into(),
            ExtensionObject::from_message(event_filter),
        ))
        .send(session.channel())
        .await
        .expect("CreateMonitoredItems is a valid service on micro");
    assert_eq!(
        response.results[0].result.status_code,
        StatusCode::BadMonitoredItemFilterUnsupported,
        "event monitored items are not Micro surface"
    );

    // A plain value-change item still works — the rejection did not corrupt anything.
    let results = session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Neither,
            vec![MonitoredItemCreateRequest::new(
                ReadValueId::from(counter.clone()),
                MonitoringMode::Reporting,
                MonitoringParameters {
                    client_handle: 2,
                    sampling_interval: 100.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            )],
        )
        .await
        .expect("plain monitored item must still be creatable");
    assert_eq!(results[0].result.status_code, StatusCode::Good);
    let monitored_item_id = results[0].result.monitored_item_id;

    // SetTriggering → service-level BadServiceUnsupported (T025).
    let r = SetTriggering::new(subscription_id, monitored_item_id, &session)
        .add_link(monitored_item_id)
        .send(session.channel())
        .await;
    match r {
        Err(e) => assert_eq!(
            e.status(),
            StatusCode::BadServiceUnsupported,
            "SetTriggering is Standard DataChange 2017 facet surface, not Micro"
        ),
        Ok(_) => panic!("SetTriggering must fault BadServiceUnsupported on micro"),
    }

    // Call → service-level BadServiceUnsupported (method-call not in Micro).
    let r = Call::new(&session)
        .method(CallMethodRequest {
            object_id: ObjectId::Server.into(),
            method_id: NodeId::new(0, 11492),
            input_arguments: Some(vec![Variant::UInt32(0)]),
        })
        .send(session.channel())
        .await;
    match r {
        Err(e) => assert_eq!(
            e.status(),
            StatusCode::BadServiceUnsupported,
            "Call is not Micro surface"
        ),
        Ok(_) => panic!("Call must fault BadServiceUnsupported on micro"),
    }
}
