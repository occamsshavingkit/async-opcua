use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use opcua_types::{BinaryEncodable, ContextOwned, NodeId, Variant};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::{connection, reader};
use crate::{
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    engine::subscriber_mqtt::MqttForwarder,
    MessageEncoding, SubscriberError, SubscriberRuntime,
};

#[tokio::test]
async fn mqtt_forwarder_records_custom_fragment_drop_only_for_owner() {
    // Given two UADP readers and a legacy custom-fragment payload.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let owner = reader(1, MessageEncoding::Uadp, NodeId::new(1, "OwnerTarget"));
    let unrelated = reader(2, MessageEncoding::Uadp, NodeId::new(1, "UnrelatedTarget"));
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(
            address_space,
            vec![connection(vec![owner.clone(), unrelated])],
        )
        .expect("reader fixture should be valid"),
    ));
    let (payload_tx, payload_rx) = tokio::sync::mpsc::channel(1);
    let forwarder = MqttForwarder {
        runtime: runtime.clone(),
        reader: owner,
        payload_rx,
        cancel: CancellationToken::new(),
        connection_id: "mqtt-conn".to_string(),
        topic: "opcua/telemetry/7".to_string(),
    }
    .spawn();

    // When the owning forwarder receives the fragment and its channel closes.
    payload_tx
        .send(vec![0, 0, 0, 0, 0, 0, 1])
        .await
        .expect("bounded test channel should remain open");
    drop(payload_tx);
    timeout(Duration::from_secs(1), forwarder)
        .await
        .expect("forwarder should stop when its receiver closes")
        .expect("forwarder should not panic");

    // Then only the owning reader records the unsupported fragment drop.
    let runtime = runtime.read();
    let owner_status = runtime.reader_status(1).expect("owner status should exist");
    assert_eq!(owner_status.dropped_count, 1);
    assert_eq!(
        owner_status.last_error,
        Some(SubscriberError::UnsupportedTarget)
    );
    let unrelated_status = runtime
        .reader_status(2)
        .expect("unrelated status should exist");
    assert_eq!(unrelated_status.dropped_count, 0);
    assert_eq!(unrelated_status.last_error, None);
}

#[tokio::test]
async fn mqtt_forwarder_stops_when_cancelled_without_payload() {
    // Given an idle forwarder with an open bounded channel.
    let address_space = Arc::new(RwLock::new(AddressSpace::new()));
    let owner = reader(1, MessageEncoding::Uadp, NodeId::new(1, "OwnerTarget"));
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(address_space, vec![connection(vec![owner.clone()])])
            .expect("reader fixture should be valid"),
    ));
    let (_payload_tx, payload_rx) = tokio::sync::mpsc::channel(1);
    let cancel = CancellationToken::new();
    let forwarder = MqttForwarder {
        runtime,
        reader: owner,
        payload_rx,
        cancel: cancel.clone(),
        connection_id: "mqtt-conn".to_string(),
        topic: "opcua/telemetry/7".to_string(),
    }
    .spawn();

    // When cancellation fires before a payload arrives.
    cancel.cancel();

    // Then the forwarder terminates without polling or a detached task.
    timeout(Duration::from_secs(1), forwarder)
        .await
        .expect("forwarder should stop when cancelled")
        .expect("forwarder should not panic");
}

#[tokio::test]
async fn mqtt_forwarder_prioritizes_ready_cancellation_over_queued_payload() {
    // Given an owning reader whose bounded channel and cancellation are ready before spawning.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = super::insert_target(&space, "CancelledOwnerTarget");
    let address_space = Arc::new(RwLock::new(space));
    let owner = reader(1, MessageEncoding::Uadp, target.clone());
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(
            address_space.clone(),
            vec![connection(vec![owner.clone()])],
        )
        .expect("reader fixture should be valid"),
    ));
    let initial_status = runtime
        .read()
        .reader_status(1)
        .expect("owner status should exist");
    let message = UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(91.5)],
        }],
    };
    let payload = message.encode_to_vec(&ContextOwned::default().context());
    let (payload_tx, payload_rx) = tokio::sync::mpsc::channel(1);
    payload_tx
        .try_send(payload)
        .expect("bounded test channel should accept one payload");
    let cancel = CancellationToken::new();
    cancel.cancel();

    // When the forwarder starts with both select branches ready.
    let forwarder = MqttForwarder {
        runtime: runtime.clone(),
        reader: owner,
        payload_rx,
        cancel,
        connection_id: "mqtt-conn".to_string(),
        topic: "opcua/telemetry/7".to_string(),
    }
    .spawn();
    timeout(Duration::from_secs(1), forwarder)
        .await
        .expect("forwarder should stop on ready cancellation")
        .expect("forwarder should not panic");

    // Then cancellation wins without applying or diagnosing the queued payload.
    assert_eq!(
        super::target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .read()
            .reader_status(1)
            .expect("owner status should exist"),
        initial_status
    );
}
