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
    address_space::TypeTree, gds::register_gds_pull_methods_from_companion,
    node_manager::memory::CoreNodeManager, services::subscription::filter::ParsedEventFilter,
    ServerBuilder, ServerConfig, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{
    ApplicationRecordDataType, AttributeId, ContentFilter, EndpointDescription, EventFilter,
    GdsApplicationRecordTypeLoader, MessageSecurityMode, NodeClass, NumericRange, QualifiedName,
    SimpleAttributeOperand, StatusCode, UserTokenPolicy,
};

const GDS_NAMESPACE_URI: &str = "http://opcfoundation.org/UA/GDS/";

fn xml_present() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml"
    ))
    .exists()
}

#[tokio::test]
async fn companion_registration_rejects_core_node_manager_from_another_server_before_mutation() {
    // Given: two independent servers and a recorded baseline for their GDS namespace metadata.
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_cross_server");
    let _ = std::fs::remove_dir_all(&test_dir);
    let build_server = |pki_dir: &std::path::Path| {
        let mut config = ServerConfig {
            pki_dir: pki_dir.to_path_buf(),
            create_sample_keypair: true,
            certificate_path: Some(pki_dir.join("own/cert.der")),
            private_key_path: Some(pki_dir.join("private/private.pem")),
            discovery_urls: vec!["opc.tcp://127.0.0.1:0/".to_string()],
            ..Default::default()
        };
        config.add_endpoint(
            "none",
            ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
        );
        ServerBuilder::from_config(config)
            .build()
            .expect("cross-server GDS test server should build")
    };
    let (_foreign_server, foreign_handle) = build_server(&test_dir.join("foreign_pki"));
    let (_handle_server, server_handle) = build_server(&test_dir.join("handle_pki"));
    let expected_handle_type_tree_gds_namespace = {
        let mut type_tree = server_handle.type_tree().write();
        Some(type_tree.namespaces_mut().add_namespace(GDS_NAMESPACE_URI))
    };
    let foreign_core_node_manager = foreign_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
        .expect("default server should have a CoreNodeManager for namespace 0");
    assert!(
        foreign_core_node_manager
            .address_space()
            .read()
            .namespaces()
            .values()
            .all(|uri| uri != GDS_NAMESPACE_URI),
        "the foreign core manager should start without the GDS namespace"
    );
    assert_eq!(
        server_handle
            .type_tree()
            .read()
            .namespaces()
            .get_index(GDS_NAMESPACE_URI),
        expected_handle_type_tree_gds_namespace,
        "the handle-owned type tree should start in the expected GDS namespace state"
    );

    // When: registration pairs the first server's core manager with the second server's handle.
    let registration =
        register_gds_pull_methods_from_companion(&foreign_core_node_manager, &server_handle);

    // Then: registration is rejected before either supplied state can gain GDS metadata.
    let foreign_address_space_has_gds = foreign_core_node_manager
        .address_space()
        .read()
        .namespaces()
        .values()
        .any(|uri| uri == GDS_NAMESPACE_URI);
    let handle_type_tree_gds_namespace = server_handle
        .type_tree()
        .read()
        .namespaces()
        .get_index(GDS_NAMESPACE_URI);
    assert_eq!(
        (
            registration.is_none(),
            foreign_address_space_has_gds,
            handle_type_tree_gds_namespace,
        ),
        (true, false, expected_handle_type_tree_gds_namespace,),
        "a mismatched manager/handle pair must return None without mutating either server state"
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[tokio::test]
async fn companion_registration_rejects_namespace_index_collision_before_mutation() {
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_namespace_collision");
    let _ = std::fs::remove_dir_all(&test_dir);
    let mut config = ServerConfig {
        pki_dir: test_dir.clone(),
        create_sample_keypair: true,
        certificate_path: Some(test_dir.join("own/cert.der")),
        private_key_path: Some(test_dir.join("private/private.pem")),
        discovery_urls: vec!["opc.tcp://127.0.0.1:0/".to_string()],
        ..Default::default()
    };
    config.add_endpoint(
        "none",
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );
    let (_server, server_handle) = ServerBuilder::from_config(config)
        .build()
        .expect("namespace-collision GDS test server should build");
    let core_node_manager = server_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
        .expect("default server should have a CoreNodeManager for namespace 0");
    let type_tree_uri = "urn:test:gds:type-tree-only";
    let address_space_uri = "urn:test:gds:address-space-only";
    let collision_index = server_handle
        .type_tree()
        .write()
        .namespaces_mut()
        .add_namespace(type_tree_uri);
    core_node_manager
        .address_space()
        .read()
        .add_namespace(address_space_uri, collision_index);
    let expected_type_tree_namespaces = server_handle.type_tree().read().namespaces().clone();
    let expected_address_space_namespaces = core_node_manager.address_space().read().namespaces();

    let registration = register_gds_pull_methods_from_companion(&core_node_manager, &server_handle);

    assert!(registration.is_none());
    assert_eq!(
        server_handle
            .type_tree()
            .read()
            .namespaces()
            .known_namespaces(),
        expected_type_tree_namespaces.known_namespaces()
    );
    assert_eq!(
        core_node_manager.address_space().read().namespaces(),
        expected_address_space_namespaces
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[tokio::test]
async fn companion_registration_rejects_namespace_uri_index_mismatch_before_mutation() {
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_namespace_uri_index_mismatch");
    let _ = std::fs::remove_dir_all(&test_dir);
    let mut config = ServerConfig {
        pki_dir: test_dir.clone(),
        create_sample_keypair: true,
        certificate_path: Some(test_dir.join("own/cert.der")),
        private_key_path: Some(test_dir.join("private/private.pem")),
        discovery_urls: vec!["opc.tcp://127.0.0.1:0/".to_string()],
        ..Default::default()
    };
    config.add_endpoint(
        "none",
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );
    let (_server, server_handle) = ServerBuilder::from_config(config)
        .build()
        .expect("namespace-URI/index-mismatch GDS test server should build");
    let core_node_manager = server_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
        .expect("default server should have a CoreNodeManager for namespace 0");
    let shared_uri = "urn:test:gds:shared-namespace";
    let type_tree_index = server_handle
        .type_tree()
        .write()
        .namespaces_mut()
        .add_namespace(shared_uri);
    let address_space_index = type_tree_index + 1;
    core_node_manager
        .address_space()
        .read()
        .add_namespace(shared_uri, address_space_index);
    assert_ne!(type_tree_index, address_space_index);
    assert_eq!(
        server_handle
            .type_tree()
            .read()
            .namespaces()
            .get_index(shared_uri),
        Some(type_tree_index)
    );
    assert_eq!(
        core_node_manager
            .address_space()
            .read()
            .namespaces()
            .iter()
            .find_map(|(index, uri)| (uri == shared_uri).then_some(*index)),
        Some(address_space_index)
    );
    let expected_type_tree_namespaces = server_handle.type_tree().read().namespaces().clone();
    let expected_address_space_namespaces = core_node_manager.address_space().read().namespaces();

    let registration = register_gds_pull_methods_from_companion(&core_node_manager, &server_handle);

    assert!(registration.is_none());
    assert_eq!(
        server_handle
            .type_tree()
            .read()
            .namespaces()
            .known_namespaces(),
        expected_type_tree_namespaces.known_namespaces()
    );
    assert_eq!(
        core_node_manager.address_space().read().namespaces(),
        expected_address_space_namespaces
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[tokio::test]
async fn companion_registration_publishes_gds_event_types_for_event_filters() {
    if !xml_present() {
        eprintln!("skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally");
        return;
    }

    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test_pki_gds_pull_type_tree");
    let _ = std::fs::remove_dir_all(&test_dir);
    let server_pki_dir = test_dir.join("server_pki");
    let mut server_config = ServerConfig {
        pki_dir: server_pki_dir.clone(),
        create_sample_keypair: true,
        certificate_path: Some(server_pki_dir.join("own/cert.der")),
        private_key_path: Some(server_pki_dir.join("private/private.pem")),
        discovery_urls: vec!["opc.tcp://127.0.0.1:0/".to_string()],
        ..Default::default()
    };
    server_config.add_endpoint(
        "none",
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );

    let (_server, server_handle) = ServerBuilder::from_config(server_config).build().unwrap();
    let core_node_manager = server_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
        .expect("default server should have a CoreNodeManager for namespace 0");
    register_gds_pull_methods_from_companion(&core_node_manager, &server_handle)
        .expect("companion XML is present, Pull-model wiring should succeed");

    let gds_namespace = core_node_manager
        .address_space()
        .read()
        .namespaces()
        .into_iter()
        .find_map(|(index, uri)| (uri == GDS_NAMESPACE_URI).then_some(index))
        .expect("companion import should register the GDS namespace");
    let requested_event_type = {
        let address_space = core_node_manager.address_space().read();
        let requested_event_type = address_space
            .nodes()
            .find_map(|node| {
                let node = node.as_node();
                (node.browse_name()
                    == &QualifiedName::new(gds_namespace, "CertificateRequestedAuditEventType"))
                    .then(|| node.node_id().clone())
            })
            .expect(
                "the imported GDS address space should contain CertificateRequestedAuditEventType",
            );
        requested_event_type
    };

    {
        let type_tree = server_handle.type_tree().read();
        assert_eq!(
            type_tree.namespaces().get_index(GDS_NAMESPACE_URI),
            Some(gds_namespace),
            "companion registration should update the shared type-tree namespace map"
        );
        assert_eq!(
            type_tree.get(&requested_event_type),
            Some(NodeClass::ObjectType),
            "CertificateRequestedAuditEventType should be loaded into the shared type tree"
        );
    }

    let snapshot = server_handle
        .info()
        .type_tree_snapshot()
        .expect("server construction should publish an initial type-tree snapshot");
    assert_eq!(
        snapshot.get(&requested_event_type),
        Some(NodeClass::ObjectType),
        "companion registration should republish the imported event type for subscription readers"
    );

    let filter = EventFilter {
        select_clauses: Some(vec![SimpleAttributeOperand {
            type_definition_id: requested_event_type,
            browse_path: Some(vec![QualifiedName::new(gds_namespace, "CertificateGroup")]),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
        }]),
        where_clause: ContentFilter::default(),
    };
    let (result, parsed) = ParsedEventFilter::parse(filter, snapshot.as_type_tree());
    assert_eq!(result.select_clause_results, Some(vec![StatusCode::Good]));
    parsed.expect("the published snapshot should validate a namespace-qualified GDS select clause");

    let _ = std::fs::remove_dir_all(&test_dir);
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
    let pull_handler = register_gds_pull_methods_from_companion(&core_node_manager, &server_handle)
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
    let pull_handler = register_gds_pull_methods_from_companion(&core_node_manager, &server_handle)
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
    let pull_handler = register_gds_pull_methods_from_companion(&core_node_manager, &server_handle)
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
