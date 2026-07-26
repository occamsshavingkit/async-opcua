//! End-to-end test for the GDS Pull model client-side fix (Run 2): proves `GdsClient::discover`
//! resolves real NodeIds via dynamic discovery (OPC-10000-4 §5.8.4 `TranslateBrowsePathsToNodeIds`)
//! against a real server whose GDS companion namespace is assigned a non-default index -- not any
//! of the fabricated namespace-0 constants this fix removes -- and that subsequent Call requests
//! dispatch correctly against the discovered NodeIds. See `specs/105-gds-pull-client-fix/`.
//!
//! Skipped gracefully (not a failure) if `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` isn't
//! present locally -- see `schemas/companion/README.md`.
//!
//! `companion-gds`/`method-call`/`generated-address-space` are `async-opcua-server` features
//! (this crate's dev-dependency, not this crate's own), enabled unconditionally via
//! `async-opcua-client/Cargo.toml`'s dev-dependency entry -- there is no feature gate needed here.

use std::time::Duration;

use tokio::net::TcpListener;

use opcua_client::{gds::GdsClient, ClientBuilder, IdentityToken};
use opcua_crypto::SecurityPolicy;
use opcua_server::{
    gds::register_gds_pull_methods_from_companion, node_manager::memory::CoreNodeManager,
    IdentityMappingRule, ServerBuilder, ServerConfig, ServerEndpoint, ServerUserToken,
    WellKnownRole, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{
    ApplicationRecordDataType, ApplicationType, EndpointDescription,
    GdsApplicationRecordTypeLoader, LocalizedText, MessageSecurityMode, NodeId, StatusCode,
    UAString, UserTokenPolicy,
};

fn xml_present() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml"
    ))
    .exists()
}

/// The old, fabricated NodeIds this fix removes -- discovery must never coincidentally produce
/// any of these.
const FABRICATED_IDENTIFIERS: [u32; 5] = [22384, 22385, 22388, 22400, 22402];
const SECURITY_ADMIN_USER: &str = "security-admin";

struct RunningServer {
    test_dir: std::path::PathBuf,
    addr: std::net::SocketAddr,
    server_handle: opcua_server::ServerHandle,
    server_task: tokio::task::JoinHandle<()>,
}

async fn start_server_with_gds() -> RunningServer {
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!(
            "test_pki_gds_pull_client_discovery_{}",
            std::process::id()
        ));
    let _ = std::fs::remove_dir_all(&test_dir);
    let server_pki_dir = test_dir.join("server_pki");

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", addr.port());
    let (server, server_handle) = ServerBuilder::new()
        .application_name("GDS Pull Client Discovery Test Server")
        .application_uri("urn:gds_pull_client_discovery_test_server")
        .product_uri("urn:gds_pull_client_discovery_test_server")
        .host("127.0.0.1")
        .port(addr.port())
        .pki_dir(&server_pki_dir)
        .create_sample_keypair(true)
        .trust_client_certs(true)
        .discovery_urls(vec![endpoint_url])
        .add_user_token(
            SECURITY_ADMIN_USER,
            ServerUserToken::user_pass(SECURITY_ADMIN_USER, "correct-password"),
        )
        .add_endpoint(
            "basic256sha256-sign-encrypt",
            (
                "/",
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt,
                &[SECURITY_ADMIN_USER] as &[&str],
            ),
        )
        .identity_mapping_rule(
            WellKnownRole::SecurityAdmin.node_id(),
            IdentityMappingRule::UserName(SECURITY_ADMIN_USER.into()),
        )
        .with_type_loader(std::sync::Arc::new(GdsApplicationRecordTypeLoader))
        .build()
        .unwrap();

    let core_node_manager = server_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
        .expect("default server should have a CoreNodeManager for namespace 0");
    register_gds_pull_methods_from_companion(&core_node_manager, &server_handle.type_tree().read())
        .expect("companion XML is present, Pull-model wiring should succeed");

    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    RunningServer {
        test_dir,
        addr,
        server_handle,
        server_task,
    }
}

impl RunningServer {
    async fn stop(self) {
        self.server_handle.cancel();
        let _ = self.server_task.await;
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }
}

#[tokio::test]
async fn discover_resolves_real_dynamic_node_ids_and_dispatches_against_them() {
    if !xml_present() {
        eprintln!("skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally");
        return;
    }

    let server = start_server_with_gds().await;
    let client_pki_dir = server.test_dir.join("client_pki");

    let mut client = ClientBuilder::new()
        .application_name("GDS Pull Client Discovery Test")
        .application_uri("urn:gds_pull_client_discovery_test")
        .pki_dir(client_pki_dir)
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .client()
        .unwrap();

    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", server.addr.port());
    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                endpoint_url.as_str(),
                SecurityPolicy::Basic256Sha256.to_str(),
                MessageSecurityMode::SignAndEncrypt,
            ),
            IdentityToken::new_user_name(SECURITY_ADMIN_USER, "correct-password"),
        )
        .await
        .unwrap();
    let client_handle = event_loop.spawn();
    session.wait_for_connection().await;

    let gds_client = GdsClient::discover(&session).await.expect(
        "discovery should succeed against a server with the GDS companion NodeSet imported",
    );

    // Proves real, dynamic resolution: never the old fabricated constants, and a non-zero
    // namespace index (namespace 0 would mean discovery silently fell back to something wrong).
    let directory_id = gds_client.registration().directory_object_id.clone();
    assert_ne!(
        directory_id.namespace, 0,
        "GDS Directory should resolve in a non-core namespace, not ns=0"
    );
    for id in [
        &gds_client.registration().directory_object_id,
        &gds_client.registration().register_method_id,
        &gds_client.csr().directory_object_id,
        &gds_client.csr().start_signing_request_id,
        &gds_client.csr().finish_signing_request_id,
    ] {
        if let opcua_types::Identifier::Numeric(n) = &id.identifier {
            assert!(
                !FABRICATED_IDENTIFIERS.contains(n),
                "discovered NodeId {id} matches an old fabricated constant"
            );
        }
    }
    assert_eq!(gds_client.registration().directory_object_id, directory_id);
    assert_eq!(gds_client.csr().directory_object_id, directory_id);

    // An encrypted SecurityAdmin session reaches argument decoding, so this succeeds only when the
    // client sends Part 12 §6.5.5 ApplicationRecordDataType rather than ApplicationDescription.
    let application_id = gds_client
        .register_application(
            &session,
            ApplicationRecordDataType {
                application_id: NodeId::null(),
                application_uri: UAString::from("urn:gds_pull_client_discovery_test"),
                application_names: Some(vec![LocalizedText::from("Test Application")]),
                application_type: ApplicationType::Client,
                product_uri: UAString::from("urn:gds_pull_client_discovery_test:product"),
                discovery_urls: None,
                server_capabilities: None,
            },
        )
        .await
        .expect("RegisterApplication should decode the shared application record wire type");
    assert!(!application_id.is_null());

    // The same authorized session reaches StartSigningRequest argument validation; an empty CSR is
    // rejected after dispatch rather than by the security gate.
    let csr_result = gds_client
        .request_signing_csr(
            &session,
            application_id.clone(),
            NodeId::null(),
            NodeId::null(),
            &[],
        )
        .await;
    assert_eq!(csr_result, Err(StatusCode::BadInvalidArgument));

    // T009a: discovery is one-shot -- calling again on the same already-discovered client reuses
    // the resolved NodeIds (no second discover() call), and dispatches identically.
    let csr_result_again = gds_client
        .request_signing_csr(
            &session,
            application_id,
            NodeId::null(),
            NodeId::null(),
            &[],
        )
        .await;
    assert_eq!(csr_result_again, Err(StatusCode::BadInvalidArgument));
    assert_eq!(gds_client.csr().directory_object_id, directory_id);

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server.stop().await;
}

#[tokio::test]
async fn discover_fails_closed_against_a_server_without_the_gds_namespace() {
    let mut server_config = ServerConfig {
        create_sample_keypair: true,
        discovery_urls: vec!["opc.tcp://127.0.0.1:0/".to_string()],
        ..Default::default()
    };
    server_config.tcp_config.host = "127.0.0.1".to_string();
    server_config.tcp_config.port = 0;
    server_config.add_endpoint(
        "none",
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("test_pki_gds_no_namespace_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&test_dir);
    server_config.pki_dir = test_dir.join("server_pki");
    server_config.certificate_path = Some(server_config.pki_dir.join("own/cert.der"));
    server_config.private_key_path = Some(server_config.pki_dir.join("private/private.pem"));

    let (server, server_handle) = ServerBuilder::from_config(server_config).build().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let client_pki_dir = test_dir.join("client_pki");
    let mut client = ClientBuilder::new()
        .application_name("GDS Pull Client Discovery Negative Test")
        .application_uri("urn:gds_pull_client_discovery_negative_test")
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

    let result = GdsClient::discover(&session).await;
    assert!(
        result.is_err(),
        "discovery against a non-GDS server should fail closed, not succeed"
    );

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server_handle.cancel();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&test_dir);
}
