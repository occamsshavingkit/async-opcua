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
    gds::{
        application_record::{ApplicationRecordDataType, GdsApplicationRecordTypeLoader},
        register_gds_pull_methods_from_companion,
    },
    node_manager::memory::CoreNodeManager,
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

/// Proves `RegisterApplication` (feature 108, Part 12 §6.5.6) reaches the registered Directory
/// method callback over a real network connection, mirroring the reachability-proof pattern above
/// for `StartNewKeyPairRequest`: an anonymous, `None`-security channel can't satisfy
/// `authorize_authenticated_security_admin`'s security-mode check, so a real dispatch reaching the
/// handler is distinguishable (`Bad_SecurityModeInsufficient`) from a fabricated/unresolved NodeId
/// (which the Call service would reject before ever reaching the handler).
#[tokio::test]
async fn register_application_call_requires_an_encrypted_channel() {
    if !xml_present() {
        eprintln!("skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally");
        return;
    }

    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_directory_write");
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
    let register_application_id = pull_handler.directory().register_application_id.clone();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut client = ClientBuilder::new()
        .application_name("GDS Pull Directory Integration Client (write)")
        .application_uri("urn:gds_pull_directory_integration_client_write")
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
    // Populates the client's own namespace-URI-to-index cache from the server's real
    // `Server_NamespaceArray` -- without this, the client can't resolve the GDS namespace URI
    // carried by the encoded `ApplicationRecordDataType` argument at all.
    session.read_namespace_array().await.unwrap();

    let record = ApplicationRecordDataType {
        application_id: opcua_types::NodeId::null(),
        application_uri: opcua_types::UAString::from("urn:test:wire-register-app"),
        application_type: opcua_types::ApplicationType::Server,
        application_names: None,
        product_uri: opcua_types::UAString::null(),
        discovery_urls: None,
        server_capabilities: None,
    };
    let args = vec![opcua_types::Variant::from(
        opcua_types::ExtensionObject::new(record),
    )];
    let result = session
        .call_one((directory_object_id, register_application_id, Some(args)))
        .await
        .unwrap();

    assert_eq!(
        result.status_code,
        StatusCode::BadSecurityModeInsufficient,
        "RegisterApplication should reach the registered handler and be rejected for lacking an \
         encrypted channel -- a different error here means the Call service never reached the \
         handler at all"
    );

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server_handle.cancel();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Proves the hand-authored `ApplicationRecordDataType` codec (feature 108, research.md R8)
/// actually round-trips over REAL wire bytes: the server encodes a registered application record
/// into `GetApplication`'s `CallMethodResult`, and the client -- with the same
/// `GdsApplicationRecordTypeLoader` registered via `Session::add_type_loader` -- decodes it back.
/// This is the one thing feature 108's unit tests (`gds::pull_methods::tests`) cannot exercise:
/// those build/inspect `ExtensionObject`s purely in-process via `into_inner_as`, without ever
/// serializing them to bytes.
#[tokio::test]
async fn get_application_call_round_trips_the_wire_encoded_application_record() {
    if !xml_present() {
        eprintln!("skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally");
        return;
    }

    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_directory_read");
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

    let (server, server_handle) = ServerBuilder::from_config(server_config)
        .with_type_loader(std::sync::Arc::new(GdsApplicationRecordTypeLoader))
        .build()
        .unwrap();

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
    let get_application_id = pull_handler.directory().get_application_id.clone();

    // Seeds a real registration via the same internal registry the Pull model's own
    // certificate-issuance workflow uses (research.md R5) -- not a fabricated stand-in.
    let application_id = pull_handler.registry().register_application(
        "urn:test:wire-get-app",
        pull_handler
            .directory()
            .default_application_group_id
            .clone(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut client = ClientBuilder::new()
        .application_name("GDS Pull Directory Integration Client (read)")
        .application_uri("urn:gds_pull_directory_integration_client_read")
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
    session.read_namespace_array().await.unwrap();
    session.add_type_loader(std::sync::Arc::new(GdsApplicationRecordTypeLoader));

    let args = vec![opcua_types::Variant::from(application_id)];
    let result = session
        .call_one((directory_object_id, get_application_id, Some(args)))
        .await
        .unwrap();

    assert_eq!(
        result.status_code,
        StatusCode::Good,
        "GetApplication should succeed for a real, registered application"
    );
    let outputs = result
        .output_arguments
        .expect("GetApplication should return an output argument");
    let opcua_types::Variant::ExtensionObject(obj) = &outputs[0] else {
        panic!("expected ExtensionObject output, got {:?}", outputs[0]);
    };
    let decoded = obj
        .clone()
        .into_inner_as::<ApplicationRecordDataType>()
        .expect(
            "the client should decode the wire-encoded ApplicationRecordDataType via its own \
             registered GdsApplicationRecordTypeLoader",
        );
    assert_eq!(
        decoded.application_uri,
        opcua_types::UAString::from("urn:test:wire-get-app")
    );

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server_handle.cancel();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&test_dir);
}
