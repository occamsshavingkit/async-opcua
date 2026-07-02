//! T032 (US4): Standard 2017 UA Server Profile — smoke test.
//!
//! Grounding: Standard 2017 CUs — Session Minimum 50 Parallel, Enhanced DataChange
//! facet; Discovery Register/Register2, Session Cancel; OPC 10000-4 §5.7.5;
//! OPC 10000-12 §4.2.2.
//!
//! The RegisterServer2 flow and X509 user-token activation require a second
//! in-process server and two-phase secure client connect respectively; those
//! tests are #[ignore]d with TODO comments.

#![cfg(feature = "profile-tests")]

mod common;

use common::{connect, spawn_standard};

use opcua::types::{NodeId, ObjectId, ReadValueId, StatusCode, TimestampsToReturn, VariableId};

/// The standard sample starts, accepts a session, and serves a Read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standard_server_serves_read() {
    let tester = spawn_standard().await;
    let session = connect(&tester).await;

    let values = session
        .read(
            &[ReadValueId::from(NodeId::from(
                VariableId::Server_ServerStatus,
            ))],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("Read must succeed");
    assert_eq!(values[0].status(), StatusCode::Good);
}

/// Session Minimum 50 Parallel: the server is configured for ≥50 sessions.
/// We open 3 and confirm all can read simultaneously.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_parallel_sessions() {
    let tester = spawn_standard().await;
    let s1 = connect(&tester).await;
    let s2 = connect(&tester).await;
    let s3 = connect(&tester).await;

    for session in [&s1, &s2, &s3] {
        let values = session
            .read(
                &[ReadValueId::from(NodeId::from(
                    VariableId::Server_ServerStatus,
                ))],
                TimestampsToReturn::Neither,
                0.0,
            )
            .await
            .expect("Read must succeed");
        assert_eq!(values[0].status(), StatusCode::Good);
    }
}

/// Browse the Objects folder — the generated type system is present
/// (inherited from the embedded composition).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browse_objects_folder() {
    let tester = spawn_standard().await;
    let session = connect(&tester).await;

    let results = session
        .browse(
            &[opcua::types::BrowseDescription {
                node_id: ObjectId::ObjectsFolder.into(),
                browse_direction: opcua::types::BrowseDirection::Forward,
                reference_type_id: opcua::types::ReferenceTypeId::HierarchicalReferences.into(),
                include_subtypes: true,
                node_class_mask: 0,
                result_mask: opcua::types::BrowseDescriptionResultMask::all().bits(),
            }],
            100,
            None,
        )
        .await
        .expect("Browse must succeed");

    assert!(
        !results[0]
            .references
            .as_deref()
            .unwrap_or_default()
            .is_empty(),
        "Objects folder must contain nodes"
    );
}

/// Cancel of an outstanding request per Part 4 §5.7.5.
/// The standard profile includes Cancel in the core session service set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_service_is_available() {
    let tester = spawn_standard().await;
    let session = connect(&tester).await;

    // Send a Cancel via the raw UARequest builder. Cancel(request_handle_to_cancel=0)
    // proves the service is available (not BadServiceUnsupported).
    use opcua::client::{services::Cancel, UARequest};
    let result = Cancel::new(0, &session).send(session.channel()).await;

    match result {
        Ok(response) => {
            assert!(
                response.response_header.service_result == StatusCode::Good
                    || response.response_header.service_result == StatusCode::BadNothingToDo,
                "Cancel must not return BadServiceUnsupported, got {:?}",
                response.response_header.service_result
            );
        }
        Err(e) => {
            assert_ne!(
                e.status(),
                StatusCode::BadServiceUnsupported,
                "Cancel must be available on standard"
            );
        }
    }
}

/// TODO: X509 user-token activation over Sign&Encrypt — requires two-phase
/// client connect (GetEndpoints to extract server cert, then reconnect with
/// Sign&Encrypt and X509 token). #[ignore] until the test harness supports it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs two-phase secure client connect + X509 token provisioning"]
async fn x509_user_token_activation() {}

/// TODO: RegisterServer2 flow against an in-process LDS peer — requires
/// spawning a second server with the `lds` feature enabled and verifying
/// the standard server's periodic registration arrives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs in-process LDS peer server + registration verification"]
async fn register_server2_flow() {}
