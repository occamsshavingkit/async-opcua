//! End-to-end integration test for File Access real I/O (OPC UA Part 20 §4.2), proving the full
//! wire dispatch path -- client -> network -> server -> Call service -> `SimpleNodeManager` ->
//! the registered File Access callback -- performs real byte-level file I/O against a real
//! backing file on disk, not an inert node. See `specs/106-file-access-io/`.

#![cfg(all(
    feature = "fota",
    feature = "method-call",
    feature = "generated-address-space"
))]

use std::time::Duration;

use tokio::net::TcpListener;

use opcua_client::{ClientBuilder, IdentityToken};
use opcua_server::{
    fota::{
        file_access::register_file_access_methods,
        file_node::{TemporaryFileNode, TemporaryFileNodeConfig},
    },
    node_manager::memory::{simple_node_manager, SimpleNodeManager},
    node_manager::NamespaceMetadata,
    ServerBuilder, ServerConfig, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{
    ByteString, EndpointDescription, MessageSecurityMode, StatusCode, UserTokenPolicy,
};

const FOTA_NAMESPACE_URI: &str = "urn:async-opcua:fota-file-access-test";
const FOTA_NAMESPACE_INDEX: u16 = 2;

#[tokio::test]
async fn client_writes_and_reads_back_real_file_content() {
    let test_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("test_pki_fota_file_access_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&test_dir);
    let server_pki_dir = test_dir.join("server_pki");
    let client_pki_dir = test_dir.join("client_pki");
    let backing_path = test_dir.join("firmware.bin");
    std::fs::create_dir_all(&test_dir).unwrap();

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

    let namespace = NamespaceMetadata {
        namespace_uri: FOTA_NAMESPACE_URI.to_string(),
        namespace_index: FOTA_NAMESPACE_INDEX,
        ..Default::default()
    };
    let (server, server_handle) = ServerBuilder::from_config(server_config)
        .with_node_manager(simple_node_manager(namespace, "fota-file-access-test"))
        .build()
        .unwrap();

    let node_manager = server_handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .expect("server should have the configured SimpleNodeManager");

    let file_node = {
        let address_space = node_manager.address_space().write();
        let mut config = TemporaryFileNodeConfig::new(
            FOTA_NAMESPACE_INDEX,
            opcua_types::NodeId::new(0, "fota-file-access-test-session"),
            "firmware.bin",
        );
        config.namespace_uri = FOTA_NAMESPACE_URI.to_string();
        TemporaryFileNode::create(&address_space, config)
            .expect("temporary file node should be created")
    };
    let directory_object_id = file_node.file_id.clone();
    let open_id = file_node.open_id.clone();
    let close_id = file_node.close_id.clone();
    let read_id = file_node.read_id.clone();
    let write_id = file_node.write_id.clone();

    register_file_access_methods(
        &node_manager,
        &file_node,
        backing_path.clone(),
        64 * 1024,
        true,
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        server.run_with(listener).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut client = ClientBuilder::new()
        .application_name("FOTA File Access Integration Client")
        .application_uri("urn:fota_file_access_integration_client")
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

    // Open for write (Write | EraseExisting), write a known byte sequence, close.
    let write_mode = 2u8 | 4u8; // Write | EraseExisting
    let result = session
        .call_one((
            directory_object_id.clone(),
            open_id.clone(),
            Some(vec![opcua_types::Variant::from(write_mode)]),
        ))
        .await
        .unwrap();
    assert_eq!(
        result.status_code,
        StatusCode::Good,
        "Open for write should succeed"
    );
    let opcua_types::Variant::UInt32(write_handle) = result.output_arguments.unwrap()[0].clone()
    else {
        panic!("expected a UInt32 file handle");
    };

    let payload = b"real firmware bytes, end to end".to_vec();
    let result = session
        .call_one((
            directory_object_id.clone(),
            write_id.clone(),
            Some(vec![
                opcua_types::Variant::from(write_handle),
                opcua_types::Variant::from(ByteString::from(payload.clone())),
            ]),
        ))
        .await
        .unwrap();
    assert_eq!(result.status_code, StatusCode::Good, "Write should succeed");

    let result = session
        .call_one((
            directory_object_id.clone(),
            close_id.clone(),
            Some(vec![opcua_types::Variant::from(write_handle)]),
        ))
        .await
        .unwrap();
    assert_eq!(result.status_code, StatusCode::Good, "Close should succeed");

    // Independently verify the bytes actually landed on disk -- not just that a subsequent Read
    // through the same handler reports them, which could pass even without real file-backed I/O.
    assert_eq!(
        std::fs::read(&backing_path).expect("backing file should exist after Write"),
        payload,
        "Write should persist real bytes to the backing file on disk"
    );

    // Open for read, read the whole file back, close, and diff the bytes.
    let read_mode = 1u8; // Read
    let result = session
        .call_one((
            directory_object_id.clone(),
            open_id.clone(),
            Some(vec![opcua_types::Variant::from(read_mode)]),
        ))
        .await
        .unwrap();
    assert_eq!(
        result.status_code,
        StatusCode::Good,
        "Open for read should succeed"
    );
    let opcua_types::Variant::UInt32(read_handle) = result.output_arguments.unwrap()[0].clone()
    else {
        panic!("expected a UInt32 file handle");
    };

    let mut read_back = Vec::new();
    loop {
        let result = session
            .call_one((
                directory_object_id.clone(),
                read_id.clone(),
                Some(vec![
                    opcua_types::Variant::from(read_handle),
                    opcua_types::Variant::from(64_i32),
                ]),
            ))
            .await
            .unwrap();
        assert_eq!(result.status_code, StatusCode::Good, "Read should succeed");
        let opcua_types::Variant::ByteString(data) = result.output_arguments.unwrap()[0].clone()
        else {
            panic!("expected a ByteString");
        };
        let Some(bytes) = data.value else { break };
        if bytes.is_empty() {
            break;
        }
        read_back.extend_from_slice(&bytes);
    }

    let result = session
        .call_one((
            directory_object_id,
            close_id,
            Some(vec![opcua_types::Variant::from(read_handle)]),
        ))
        .await
        .unwrap();
    assert_eq!(result.status_code, StatusCode::Good, "Close should succeed");

    assert_eq!(
        read_back, payload,
        "content read back through real OPC UA File Access methods should match what was written"
    );

    let _ = session.disconnect().await;
    let _ = client_handle.await;
    server_handle.cancel();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&test_dir);
}
