//! End-to-end integration test for the GDS push model (OPC UA Part 12 §7.10).
//!
//! Proves the full wire dispatch path -- client -> network -> server -> Call service ->
//! `CoreNodeManager` -> the registered push-method callback -- actually reaches the callback
//! registered by `register_gds_push_methods_with_handle`. This is exactly the layer where the
//! pre-fix bug lived: fabricated NodeIds meant the callbacks were registered against nodes that
//! don't exist, so a real Call against the (correct) standard NodeIds would previously have
//! failed with `BadNodeIdUnknown`/`BadMethodInvalid` rather than ever reaching the handler.

#![cfg(all(feature = "gds", feature = "method-call"))]

use std::time::Duration;

use tokio::net::TcpListener;

use opcua_client::{ClientBuilder, IdentityToken};
use opcua_server::{
    gds::push_methods::{
        create_signing_request_method_id, register_gds_push_methods_with_handle,
        server_configuration_object_id,
    },
    node_manager::memory::CoreNodeManager,
    ServerBuilder, ServerConfig, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{EndpointDescription, MessageSecurityMode, StatusCode, UserTokenPolicy};

#[tokio::test]
async fn create_signing_request_call_reaches_the_push_method_callback() {
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_push");
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
    register_gds_push_methods_with_handle(&core_node_manager, server_handle.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut client = ClientBuilder::new()
        .application_name("GDS Push Integration Client")
        .application_uri("urn:gds_push_integration_client")
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

    // Anonymous, over a `None`-security channel: CreateSigningRequest (Part 12 §7.10.10) requires
    // an encrypted channel + SecurityAdmin. If the callback is reachable (correct NodeIds, correct
    // node manager), the server-side handler runs and returns Bad_SecurityModeInsufficient. If the
    // NodeIds were still wrong (the pre-fix bug), the Call service itself would reject the request
    // before ever reaching the handler, with a different error (Bad_NodeIdUnknown/Bad_MethodInvalid).
    let args = vec![
        opcua_types::Variant::from(opcua_types::NodeId::null()),
        opcua_types::Variant::from(opcua_types::NodeId::null()),
        opcua_types::Variant::from(opcua_types::UAString::null()),
        opcua_types::Variant::from(false),
        opcua_types::Variant::from(opcua_types::ByteString::null()),
    ];
    let result = session
        .call_one((
            server_configuration_object_id(),
            create_signing_request_method_id(),
            Some(args),
        ))
        .await
        .unwrap();

    assert_eq!(
        result.status_code,
        StatusCode::BadSecurityModeInsufficient,
        "CreateSigningRequest should reach the registered push-method handler and be rejected \
         for lacking an encrypted channel -- a different error here means the Call service never \
         reached the handler at all"
    );

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server_handle.cancel();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&test_dir);
}
