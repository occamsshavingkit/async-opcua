//! End-to-end integration test for the GDS Pull model (OPC UA Part 12 §7.9), gated on the
//! `companion-gds` feature. Proves the full wire dispatch path -- client -> network -> server ->
//! Call service -> `CoreNodeManager` -> the registered Pull-model callback -- actually reaches a
//! real, companion-instantiated `CertificateDirectoryType` object, not a fabricated NodeId (the
//! exact class of bug feature 101 found and fixed on the Push-model side, and that this feature
//! fixes on the Pull-model side; see `specs/103-gds-pull-fix/research.md`).
//!
//! Skipped gracefully (not a failure) if `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` isn't
//! present locally -- see `schemas/companion/README.md`.

#![cfg(all(
    feature = "companion-gds",
    feature = "method-call",
    feature = "generated-address-space"
))]

use std::time::Duration;

use tokio::net::TcpListener;

use opcua_client::{ClientBuilder, IdentityToken};
use opcua_server::{
    gds::register_gds_pull_methods_from_companion, node_manager::memory::CoreNodeManager,
    ServerBuilder, ServerConfig, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{EndpointDescription, MessageSecurityMode, StatusCode, UserTokenPolicy};

fn xml_present() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml"
    ))
    .exists()
}

#[tokio::test]
async fn start_new_key_pair_request_call_reaches_the_pull_method_callback() {
    if !xml_present() {
        eprintln!("skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally");
        return;
    }

    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_companion");
    let _ = std::fs::remove_dir_all(&test_dir);
    let server_pki_dir = test_dir.join("server_pki");
    let client_pki_dir = test_dir.join("client_pki");

    let mut server_config = ServerConfig {
        pki_dir: server_pki_dir.clone(),
        create_sample_keypair: true,
        certificate_path: Some(server_pki_dir.join("own/cert.der")),
        private_key_path: Some(server_pki_dir.join("private/private.pem")),
        discovery_urls: vec!["opc.tcp://127.0.0.1:0/".to_string()],
        ..Default::default()
    };
    server_config.tcp_config.host = "127.0.0.1".to_string();
    server_config.tcp_config.port = 0;
    server_config.add_endpoint(
        "none",
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );

    let (server, server_handle) = ServerBuilder::from_config(server_config).build().unwrap();

    let core_node_manager = server_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
        .expect("default server should have a CoreNodeManager for namespace 0");
    let pull_handler = register_gds_pull_methods_from_companion(
        &core_node_manager,
        &server_handle.type_tree().read(),
    )
    .expect("companion XML is present, Pull-model wiring should succeed");
    let directory_object_id = pull_handler.directory().directory_object_id.clone();
    let start_new_key_pair_request_id = pull_handler
        .directory()
        .start_new_key_pair_request_id
        .clone();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut client = ClientBuilder::new()
        .application_name("GDS Pull Companion Integration Client")
        .application_uri("urn:gds_pull_companion_integration_client")
        .pki_dir(client_pki_dir)
        .create_sample_keypair(true)
        .client()
        .unwrap();

    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", addr.port());
    let endpoint: EndpointDescription = (
        endpoint_url.as_str(),
        "None",
        MessageSecurityMode::None,
        UserTokenPolicy::anonymous(),
    )
        .into();

    let (session, event_loop) = client
        .connect_to_matching_endpoint(endpoint, IdentityToken::Anonymous)
        .await
        .unwrap();
    let client_handle = event_loop.spawn();
    session.wait_for_connection().await;

    // Anonymous, over a `None`-security channel: StartNewKeyPairRequest (Part 12 §7.9.4) requires
    // an encrypted channel + SecurityAdmin. If the callback is reachable (real, companion-
    // instantiated NodeId), the server-side handler runs and returns Bad_SecurityModeInsufficient.
    // If the NodeId didn't resolve to anything real, the Call service itself would reject the
    // request before ever reaching the handler, with a different error
    // (Bad_NodeIdUnknown/Bad_MethodInvalid).
    let args = vec![
        opcua_types::Variant::from(opcua_types::NodeId::null()),
        opcua_types::Variant::from(opcua_types::NodeId::null()),
        opcua_types::Variant::from(opcua_types::NodeId::null()),
        opcua_types::Variant::from(opcua_types::UAString::null()),
        opcua_types::Variant::Empty,
        opcua_types::Variant::from(opcua_types::UAString::null()),
        opcua_types::Variant::from(opcua_types::UAString::null()),
    ];
    let result = session
        .call_one((
            directory_object_id,
            start_new_key_pair_request_id,
            Some(args),
        ))
        .await
        .unwrap();

    assert_eq!(
        result.status_code,
        StatusCode::BadSecurityModeInsufficient,
        "StartNewKeyPairRequest should reach the registered Pull-model handler and be rejected \
         for lacking an encrypted channel -- a different error here means the Call service never \
         reached the handler at all"
    );

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server_handle.cancel();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&test_dir);
}
