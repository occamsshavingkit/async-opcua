//! OPC-10000-4 5.12 (MonitoredItems) / Part 6 encoding — CU 3142.
//!
//! Tests that when a monitored item specifies an XML/JSON dataEncoding, the
//! subscription sampling pipeline receives the correct data_encoding parameter.

#![allow(missing_docs)]

use std::{path::PathBuf, sync::mpsc, time::Duration};

use opcua_client::{ClientBuilder, DataChangeCallback, IdentityToken, MonitoredItem};
use opcua_crypto::SecurityPolicy;
use opcua_server::{
    address_space::VariableBuilder,
    diagnostics::NamespaceMetadata,
    node_manager::memory::{simple_node_manager, SimpleNodeManager},
    ServerBuilder, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{
    Argument, DataEncoding, DataTypeId, ExtensionObject, MessageSecurityMode,
    MonitoredItemCreateRequest, MonitoringMode, MonitoringParameters, NodeId, QualifiedName,
    ReadValueId, TimestampsToReturn, Variant,
};
use tokio::net::TcpListener;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const NAMESPACE_URI: &str = "urn:async-opcua:monitored-item-data-encoding";

/// Verifies that the `data_encoding` from a monitored item create request
/// flows through to the subscription sampling pipeline.
/// Proof: calls `maybe_notify` with a custom sample closure that captures
/// the data_encoding parameter and asserts it matches the requested encoding.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn monitored_item_data_encoding_xml_passed_to_sample_closure() {
    let namespace = NamespaceMetadata {
        namespace_uri: NAMESPACE_URI.to_string(),
        namespace_index: 2,
        ..Default::default()
    };
    let temp_dir = tempfile::Builder::new()
        .prefix("data-encoding-sample")
        .tempdir()
        .expect("temp dir");
    let server_pki = temp_dir.path().join("server-pki");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener bind");
    let addr = listener.local_addr().expect("addr");
    let endpoint = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let factory = simple_node_manager(namespace, "data-encoding-sample");

    let (server, handle) = ServerBuilder::new_anonymous("data_encoding_sample")
        .with_node_manager(factory)
        .pki_dir(PathBuf::from(&server_pki))
        .create_sample_keypair(true)
        .trust_client_certs(true)
        .discovery_urls(vec![endpoint.clone()])
        .add_endpoint(
            "none",
            (
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                &[ANONYMOUS_USER_TOKEN_ID] as &[&str],
            ),
        )
        .build()
        .expect("server build");

    let node_id = NodeId::new(2, "EncodingTest");
    let arg = Argument {
        name: "TestArg".into(),
        data_type: NodeId::new(0, 12u32),
        value_rank: -1,
        array_dimensions: None,
        description: "Test argument for data encoding".into(),
    };

    {
        let node_manager = handle
            .node_managers()
            .get_of_type::<SimpleNodeManager>()
            .expect("SimpleNodeManager");
        let namespace_index = handle
            .get_namespace_index(NAMESPACE_URI)
            .expect("namespace index");
        let address_space = node_manager.address_space().write();
        VariableBuilder::new(
            &node_id,
            QualifiedName::new(namespace_index, "EncodingTest"),
            "EncodingTest",
        )
        .data_type(DataTypeId::Argument)
        .value(Variant::ExtensionObject(ExtensionObject::new(arg.clone())))
        .writable()
        .insert(&*address_space);
    }

    let _server_task = tokio::spawn(async move {
        server.run_with(listener).await.expect("server run");
    });

    let mut client = ClientBuilder::new()
        .application_name("data_encoding_sample_client")
        .application_uri("urn:async-opcua:data-encoding-sample-client")
        .product_uri("urn:async-opcua:data-encoding-sample-client")
        .pki_dir(temp_dir.path().join("client-pki"))
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_initial(Duration::from_millis(100))
        .client()
        .expect("client build");

    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                endpoint.as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
            ),
            IdentityToken::Anonymous,
        )
        .await
        .expect("client connect");
    let event_loop_task = event_loop.spawn();

    tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
        .await
        .expect("connected");

    // Create monitored item with Default XML data_encoding
    let xml_rvid = ReadValueId {
        node_id: node_id.clone(),
        attribute_id: opcua_types::AttributeId::Value as u32,
        index_range: opcua_types::NumericRange::None,
        data_encoding: QualifiedName::from("Default XML"),
    };

    let (tx, rx) = mpsc::channel();

    let subscription_id = session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            DataChangeCallback::new(move |_value, _item: &MonitoredItem| {}),
        )
        .await
        .expect("subscription created");

    let created = session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![MonitoredItemCreateRequest::new(
                xml_rvid,
                MonitoringMode::Reporting,
                MonitoringParameters {
                    client_handle: 1,
                    sampling_interval: 50.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            )],
        )
        .await
        .expect("XML monitored item create should complete");

    assert_eq!(created.len(), 1);
    assert!(
        created[0].result.status_code.is_good(),
        "CreateMonitoredItems with XML data_encoding should succeed, got {}",
        created[0].result.status_code,
    );

    // Use maybe_notify to verify the data_encoding flows through the pipeline.
    // The sample closure captures the data_encoding parameter and asserts it.
    handle.subscriptions().maybe_notify(
        [(&node_id, opcua_types::AttributeId::Value)].into_iter(),
        move |_node_id: &NodeId,
              _attribute_id: opcua_types::AttributeId,
              _index_range: &opcua_types::NumericRange,
              data_encoding: &DataEncoding| {
            let _ = tx.send(data_encoding.clone());
            None
        },
    );

    let captured_encoding = rx
        .recv_timeout(TEST_TIMEOUT)
        .expect("sample closure should run");

    assert!(
        matches!(captured_encoding, DataEncoding::XML),
        "maybe_notify should pass DataEncoding::XML to sample closure when monitored item uses Default XML, got {:?}",
        captured_encoding,
    );

    // Also create a default (binary) monitored item to verify it passes Binary
    session
        .delete_monitored_items(subscription_id, &[created[0].result.monitored_item_id])
        .await
        .expect("delete should succeed");

    let bin_rvid = ReadValueId::from(node_id.clone());
    let created_bin = session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![MonitoredItemCreateRequest::new(
                bin_rvid,
                MonitoringMode::Reporting,
                MonitoringParameters {
                    client_handle: 2,
                    sampling_interval: 50.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            )],
        )
        .await
        .expect("binary monitored item create should complete");
    assert_eq!(created_bin.len(), 1);
    assert!(created_bin[0].result.status_code.is_good());

    let (tx2, rx2) = mpsc::channel();
    handle.subscriptions().maybe_notify(
        [(&node_id, opcua_types::AttributeId::Value)].into_iter(),
        move |_node_id: &NodeId,
              _attribute_id: opcua_types::AttributeId,
              _index_range: &opcua_types::NumericRange,
              data_encoding: &DataEncoding| {
            let _ = tx2.send(data_encoding.clone());
            None
        },
    );

    let captured_default = rx2
        .recv_timeout(TEST_TIMEOUT)
        .expect("sample closure should run for binary");

    assert!(
        matches!(captured_default, DataEncoding::Binary),
        "maybe_notify should pass DataEncoding::Binary to sample closure when monitored item uses default encoding, got {:?}",
        captured_default,
    );

    // Also verify the notification path works end-to-end:
    // Create XML monitored item, get initial notification, verify it succeeds.
    let (tx3, rx3) = mpsc::channel();
    let target3 = node_id.clone();

    let sub_id = session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            DataChangeCallback::new(move |_value, item: &MonitoredItem| {
                if item.item_to_monitor().node_id == target3 {
                    let _ = tx3.send(());
                }
            }),
        )
        .await
        .expect("subscription 2 created");

    let xml_rvid2 = ReadValueId {
        node_id: node_id.clone(),
        attribute_id: opcua_types::AttributeId::Value as u32,
        index_range: opcua_types::NumericRange::None,
        data_encoding: QualifiedName::from("Default XML"),
    };
    let xml_created = session
        .create_monitored_items(
            sub_id,
            TimestampsToReturn::Both,
            vec![MonitoredItemCreateRequest::new(
                xml_rvid2,
                MonitoringMode::Reporting,
                MonitoringParameters {
                    client_handle: 3,
                    sampling_interval: 50.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            )],
        )
        .await
        .expect("XML monitored item create for notification should complete");
    assert_eq!(xml_created.len(), 1);
    assert!(xml_created[0].result.status_code.is_good());

    session.trigger_publish_now();
    rx3.recv_timeout(TEST_TIMEOUT)
        .expect("notification with XML data_encoding should be delivered via Publish");

    event_loop_task.abort();
}
