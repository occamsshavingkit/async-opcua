//! T003 (US1): Nano Embedded Device 2017 Server Profile — mandated-operation smoke.
//!
//! Grounding: Core 2017 Server Facet mandatory CUs (Session Base/Minimum 1, Attribute
//! Read, View Basic / Minimum Continuation Point 01 / RegisterNodes /
//! TranslateBrowsePath, Discovery Get Endpoints / Find Servers Self, Security User
//! Name Password) + UA-TCP UA-SC UA-Binary. OPC 10000-4 §5.5 Discovery, §5.7 Session,
//! §5.9 View, §5.11 Attribute/Read. See
//! specs/054-profile-polish/research-assets/PROFILES-2017.md.
//!
//! RED-FIRST: written before the nano gating/sample rework (T006–T021). Today the
//! benchmark sample cannot even RUN: base-server with no node manager fails at
//! startup ("No node managers defined") — the 041 benchmark only ever measured build
//! size. Every case here is red via connection-refused until T021 gives the sample a
//! minimal hand-rolled node manager/address space (plus the demo user-name token).

#![cfg(feature = "profile-tests")]

mod common;

use common::{connect, spawn_nano};

use async_opcua_foundation_profile_nano_server as nano;
use opcua::client::services::{Browse, BrowseNext, RegisterNodes, TranslateBrowsePaths};
use opcua::client::{IdentityToken, Password, UARequest};
use opcua::types::{
    BrowseDescription, BrowseDirection, BrowsePath, NodeClassMask, NodeId, ObjectId,
    ReferenceTypeId, RelativePath, RelativePathElement, StatusCode, TimestampsToReturn, VariableId,
};

fn browse_all(node: impl Into<NodeId>) -> BrowseDescription {
    BrowseDescription {
        node_id: node.into(),
        browse_direction: BrowseDirection::Forward,
        reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
        include_subtypes: true,
        node_class_mask: NodeClassMask::all().bits(),
        result_mask: 0x3f,
    }
}

/// Discovery Find Servers Self + Get Endpoints: the server describes itself without a
/// session, and the policy-None endpoint is advertised.
#[tokio::test]
async fn discovery_self_and_endpoints() {
    let tester = spawn_nano().await;

    let client = opcua::client::ClientBuilder::new()
        .application_name("nano-discovery-probe")
        .application_uri("urn:nano-discovery-probe")
        .create_sample_keypair(false)
        .session_retry_limit(1)
        .client()
        .expect("client should build");

    let servers = client
        .find_servers(tester.url.as_str(), None, None)
        .await
        .expect("FindServers against the nano server");
    assert!(
        servers
            .iter()
            .any(|s| s.application_uri.as_ref().contains("foundation-profile")),
        "FindServers must return the server itself, got {servers:?}"
    );

    let endpoints = client
        .get_server_endpoints_from_url(tester.url.as_str())
        .await
        .expect("GetEndpoints against the nano server");
    assert!(
        endpoints
            .iter()
            .any(|e| e.security_policy_uri.as_ref().ends_with("#None")),
        "the policy-None endpoint must be advertised, got {endpoints:?}"
    );
}

/// Session Base / Session Minimum 1 / Attribute Read: an anonymous session reads
/// ServerStatus.State from the minimal core structure (Base Info Core Structure CU).
#[tokio::test]
async fn anonymous_session_reads_server_status() {
    let tester = spawn_nano().await;
    let session = connect(&tester, IdentityToken::Anonymous).await;

    let state = session
        .read(
            &[opcua::types::ReadValueId::from(NodeId::from(
                VariableId::Server_ServerStatus_State,
            ))],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("Read service must succeed");
    assert_eq!(
        state[0].status(),
        StatusCode::Good,
        "ServerStatus.State must be served (Base Info Core Structure), got {state:?}"
    );
}

/// View Basic + Minimum Continuation Point 01: Root browses to the standard folders;
/// a max-references-1 browse yields a continuation point that BrowseNext consumes.
#[tokio::test]
async fn browse_root_with_continuation_point() {
    let tester = spawn_nano().await;
    let session = connect(&tester, IdentityToken::Anonymous).await;

    let response = Browse::new(&session)
        .browse_node(browse_all(ObjectId::RootFolder))
        .send(session.channel())
        .await
        .expect("Browse of Root must succeed");
    let results = response.results.as_deref().unwrap_or_default();
    assert_eq!(results.len(), 1);
    let refs = results[0].references.as_deref().unwrap_or_default();
    let names: Vec<_> = refs
        .iter()
        .map(|r| r.browse_name.name.as_ref().to_owned())
        .collect();
    assert!(
        names.contains(&"Objects".to_owned()) && names.contains(&"Types".to_owned()),
        "Root must organize the standard folders, got {names:?}"
    );

    // Continuation point: at most one reference per node forces a continuation.
    let response = Browse::new(&session)
        .browse_node(browse_all(ObjectId::RootFolder))
        .max_references_per_node(1)
        .send(session.channel())
        .await
        .expect("Browse with max_references_per_node=1 must succeed");
    let first = &response.results.as_deref().unwrap_or_default()[0];
    assert_eq!(first.references.as_deref().unwrap_or_default().len(), 1);
    assert!(
        !first.continuation_point.is_null(),
        "a continuation point must be returned (View Minimum Continuation Point 01)"
    );

    let next = BrowseNext::new(&session)
        .continuation_point(first.continuation_point.clone())
        .send(session.channel())
        .await
        .expect("BrowseNext must succeed");
    let next_refs = next.results.as_deref().unwrap_or_default()[0]
        .references
        .as_deref()
        .unwrap_or_default();
    assert!(
        !next_refs.is_empty(),
        "BrowseNext must continue the paused browse"
    );
}

/// View RegisterNodes: registering a node returns a (possibly identical) id usable in
/// subsequent services.
#[tokio::test]
async fn register_nodes_round_trips() {
    let tester = spawn_nano().await;
    let session = connect(&tester, IdentityToken::Anonymous).await;

    let response = RegisterNodes::new(&session)
        .node_to_register(ObjectId::Server)
        .send(session.channel())
        .await
        .expect("RegisterNodes must succeed");
    let registered = response.registered_node_ids.as_deref().unwrap_or_default();
    assert_eq!(registered.len(), 1);
    assert!(!registered[0].is_null());
}

/// View TranslateBrowsePath: Root + "Objects" resolves to the Objects folder.
#[tokio::test]
async fn translate_browse_path_resolves_objects_folder() {
    let tester = spawn_nano().await;
    let session = connect(&tester, IdentityToken::Anonymous).await;

    let path = BrowsePath {
        starting_node: ObjectId::RootFolder.into(),
        relative_path: RelativePath {
            elements: Some(vec![RelativePathElement {
                reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
                is_inverse: false,
                include_subtypes: true,
                target_name: "Objects".into(),
            }]),
        },
    };
    let response = TranslateBrowsePaths::new(&session)
        .browse_path(path)
        .send(session.channel())
        .await
        .expect("TranslateBrowsePathsToNodeIds must succeed");
    let results = response.results.as_deref().unwrap_or_default();
    assert_eq!(results[0].status_code, StatusCode::Good);
    let targets = results[0].targets.as_deref().unwrap_or_default();
    assert_eq!(
        targets[0].target_id.node_id,
        NodeId::from(ObjectId::ObjectsFolder),
        "Root/Objects must resolve to the Objects folder"
    );
}

/// Security User Name Password (mandatory at Nano via the Core 2017 facet): the demo
/// user-name token activates a session over the policy-None endpoint.
#[tokio::test]
async fn username_password_activates_session() {
    let tester = spawn_nano().await;
    let session = connect(
        &tester,
        IdentityToken::UserName(
            nano::DEMO_USERNAME.into(),
            Password(nano::DEMO_PASSWORD.into()),
        ),
    )
    .await;

    // The activated session is usable.
    session
        .read(
            &[opcua::types::ReadValueId::from(NodeId::from(
                VariableId::Server_ServerStatus_State,
            ))],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("Read on a user/pass session must succeed");
}
