//! T004 (US1): excluded services fail closed on the Nano build.
//!
//! Grounding: OPC 10000-4 §5.3 (Service results — a service-level failure is a
//! ServiceFault) + §7.34; constitution IV (network-facing paths reject, never panic).
//! Raw `UARequest` builders are used instead of `Session` helpers so no client-side
//! state machine or validation masks the server's answer (error-mode convention).
//!
//! RED-FIRST: today this is red via connection-refused (the 041 sample cannot run —
//! no node manager; see profile_smoke.rs). Once T021 makes the sample runnable, the
//! interesting red remains wherever gating (T006–T018) has not landed: these services
//! are compiled in and answer normally instead of faulting. After US1 completes, the
//! nano build must answer every one of these with `BadServiceUnsupported` and keep
//! the session healthy.

mod common;

use common::{connect, spawn_nano};

use opcua::client::services::{
    AddNodes, Call, CreateSubscription, HistoryRead, Publish, QueryFirst,
};
use opcua::client::HistoryReadAction;
use opcua::client::{IdentityToken, UARequest};
use opcua::types::{
    AddNodesItem, CallMethodRequest, ExpandedNodeId, ExtensionObject, NodeAttributes, NodeClass,
    NodeId, NodeTypeDescription, ObjectId, QualifiedName, ReadRawModifiedDetails, StatusCode,
    TimestampsToReturn, VariableId,
};

/// Every service outside the Nano 2017 surface answers with a service-level
/// `BadServiceUnsupported` fault, and the session keeps working afterwards.
#[tokio::test]
async fn excluded_services_fault_and_session_survives() {
    let tester = spawn_nano().await;
    let session = connect(&tester, IdentityToken::Anonymous).await;

    // Subscription service set (OPC 10000-4 §5.14) — outside Nano.
    let r = CreateSubscription::new(&session)
        .publishing_interval(std::time::Duration::from_millis(100))
        .send(session.channel())
        .await;
    assert_service_unsupported("CreateSubscription", r.map(|_| ()));

    let r = Publish::new(&session).send(session.channel()).await;
    assert_service_unsupported("Publish", r.map(|_| ()));

    // Method service set (OPC 10000-4 §5.12) — outside Nano.
    let r = Call::new(&session)
        .method(CallMethodRequest {
            object_id: ObjectId::Server.into(),
            method_id: NodeId::new(0, 11492), // Server_GetMonitoredItems
            input_arguments: Some(vec![opcua::types::Variant::UInt32(0)]),
        })
        .send(session.channel())
        .await;
    assert_service_unsupported("Call", r.map(|_| ()));

    // History access (OPC 10000-4 §5.11, OPC 10000-11 §6.1) — outside Nano.
    let r = HistoryRead::new(
        HistoryReadAction::ReadRawModifiedDetails(ReadRawModifiedDetails {
            is_read_modified: false,
            start_time: opcua::types::DateTime::epoch(),
            end_time: opcua::types::DateTime::now(),
            num_values_per_node: 10,
            return_bounds: false,
        }),
        &session,
    )
    .node(opcua::types::HistoryReadValueId {
        node_id: VariableId::Server_ServerStatus_State.into(),
        index_range: Default::default(),
        data_encoding: Default::default(),
        continuation_point: Default::default(),
    })
    .send(session.channel())
    .await;
    assert_service_unsupported("HistoryRead", r.map(|_| ()));

    // Query service set (OPC 10000-4 Annex B) — outside Nano.
    let r = QueryFirst::new(&session)
        .node_type(NodeTypeDescription {
            type_definition_node: ExpandedNodeId::new(opcua::types::ObjectTypeId::BaseObjectType),
            include_sub_types: true,
            data_to_return: None,
        })
        .send(session.channel())
        .await;
    assert_service_unsupported("QueryFirst", r.map(|_| ()));

    // NodeManagement service set (OPC 10000-4 §5.8) — outside Nano.
    let r = AddNodes::new(&session)
        .node(AddNodesItem {
            parent_node_id: ExpandedNodeId::new(ObjectId::ObjectsFolder),
            reference_type_id: opcua::types::ReferenceTypeId::Organizes.into(),
            requested_new_node_id: ExpandedNodeId::null(),
            browse_name: QualifiedName::new(0, "NanoShouldNotAdd"),
            node_class: NodeClass::Object,
            node_attributes: ExtensionObject::from_message(NodeAttributes::default()),
            type_definition: ExpandedNodeId::new(opcua::types::ObjectTypeId::BaseObjectType),
        })
        .send(session.channel())
        .await;
    assert_service_unsupported("AddNodes", r.map(|_| ()));

    // The session must remain healthy: a follow-up Read succeeds at service level.
    session
        .read(
            &[opcua::types::ReadValueId::from(NodeId::from(
                VariableId::Server_ServerStatus_State,
            ))],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("the session must survive rejected requests (no corruption)");
}

fn assert_service_unsupported(service: &str, result: Result<(), opcua::types::Error>) {
    match result {
        Err(e) => assert_eq!(
            e.status(),
            StatusCode::BadServiceUnsupported,
            "{service} must fault with BadServiceUnsupported on the nano build, got {e:?}"
        ),
        Ok(()) => panic!(
            "{service} must fault with BadServiceUnsupported on the nano build, but succeeded"
        ),
    }
}
