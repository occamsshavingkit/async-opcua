use std::{sync::Arc, time::Duration};

use custom_codegen::generated::node_ids::{DataTypeId, ObjectId};
use opcua::{
    client::{ClientBuilder, IdentityToken, Session},
    crypto::SecurityPolicy,
    types::{
        BrowseDescription, BrowseDirection, BrowseResultMask, MessageSecurityMode, NodeClassMask,
        NodeId, ReferenceTypeId,
    },
};
use tokio::net::TcpListener;

struct TestServer {
    _handle: opcua::server::ServerHandle,
    url: String,
}

async fn spawn_server() -> TestServer {
    let _ = env_logger::try_init();

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().unwrap();
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let (server, handle) = custom_codegen::build_server(None);
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("custom-codegen test server exited: {e}");
        }
    });

    TestServer {
        _handle: handle,
        url,
    }
}

async fn connect(tester: &TestServer) -> Arc<Session> {
    let mut client = ClientBuilder::new()
        .application_name("encoding-test-client")
        .application_uri("urn:encoding-test-client")
        .create_sample_keypair(false)
        .trust_server_certs(true)
        .session_retry_limit(1)
        .client()
        .expect("client should build");

    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                tester.url.as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
            ),
            IdentityToken::Anonymous,
        )
        .await
        .expect("connect to server");
    event_loop.spawn();
    tokio::time::timeout(Duration::from_secs(10), session.wait_for_connection())
        .await
        .expect("session should activate");
    session
}

fn has_encoding_desc(node_id: NodeId) -> BrowseDescription {
    BrowseDescription {
        node_id,
        browse_direction: BrowseDirection::Forward,
        reference_type_id: ReferenceTypeId::HasEncoding.into(),
        include_subtypes: false,
        node_class_mask: NodeClassMask::all().bits(),
        result_mask: BrowseResultMask::All as u32,
    }
}

/// Custom EventTypes (ObjectTypes) from the Profinet namespace.
/// From: samples/custom-codegen/src/generated/events/generated.rs
#[allow(clippy::enum_variant_names)]
enum ProfinetEventType {
    DiagnosisAlarmType = 1002,
    AssetChangedEventType = 1003,
    TopologyChangedEventType = 1004,
}

/// Structured DataTypes in the Profinet namespace that have
/// HasEncoding references to their encoding objects.
struct DataTypeInfo {
    name: &'static str,
    id: u32,
    binary_encoding_id: u32,
    xml_encoding_id: Option<u32>,
    json_encoding_id: Option<u32>,
}

fn profinet_data_types() -> Vec<DataTypeInfo> {
    vec![
        DataTypeInfo {
            name: "PnDeviceRoleOptionSet",
            id: DataTypeId::PnDeviceRoleOptionSet as u32,
            binary_encoding_id: ObjectId::PnDeviceRoleOptionSet_Encoding_DefaultBinary as u32,
            xml_encoding_id: Some(ObjectId::PnDeviceRoleOptionSet_Encoding_DefaultXml as u32),
            json_encoding_id: Some(ObjectId::PnDeviceRoleOptionSet_Encoding_DefaultJson as u32),
        },
        DataTypeInfo {
            name: "PnDeviceDiagnosisDataType",
            id: DataTypeId::PnDeviceDiagnosisDataType as u32,
            binary_encoding_id: ObjectId::PnDeviceDiagnosisDataType_Encoding_DefaultBinary as u32,
            xml_encoding_id: Some(ObjectId::PnDeviceDiagnosisDataType_Encoding_DefaultXml as u32),
            json_encoding_id: Some(ObjectId::PnDeviceDiagnosisDataType_Encoding_DefaultJson as u32),
        },
        DataTypeInfo {
            name: "PnIM5DataType",
            id: DataTypeId::PnIM5DataType as u32,
            binary_encoding_id: ObjectId::PnIM5DataType_Encoding_DefaultBinary as u32,
            xml_encoding_id: Some(ObjectId::PnIM5DataType_Encoding_DefaultXml as u32),
            json_encoding_id: Some(ObjectId::PnIM5DataType_Encoding_DefaultJson as u32),
        },
    ]
}

#[tokio::test]
async fn custom_event_types_have_encoding_objects() {
    let tester = spawn_server().await;
    let session = connect(&tester).await;

    let ns = session
        .get_namespace_index("http://opcfoundation.org/UA/PROFINET/")
        .await
        .expect("PROFINET namespace registered");

    // Enumerate the custom EventTypes and verify they exist as browsable
    // ObjectType nodes in the address space.
    let event_type_ids = [
        ProfinetEventType::DiagnosisAlarmType as u32,
        ProfinetEventType::AssetChangedEventType as u32,
        ProfinetEventType::TopologyChangedEventType as u32,
    ];
    for id in &event_type_ids {
        let node_id = NodeId::new(ns, *id);
        let r = session
            .browse(&[has_encoding_desc(node_id.clone())], 100, None)
            .await
            .unwrap_or_else(|_| panic!("browse for EventType node ns={ns}, id={id}"));
        assert!(
            r[0].status_code.is_good(),
            "EventType ns={}, id={} must exist: {:?}",
            ns,
            id,
            r[0].status_code,
        );
    }

    // For each struct DataType used by the EventTypes, verify HasEncoding
    // references to encoding objects (Default Binary, and Default XML/JSON
    // where generated) exist.
    for dt in profinet_data_types() {
        let node_id = NodeId::new(ns, dt.id);
        let r = session
            .browse(&[has_encoding_desc(node_id)], 100, None)
            .await
            .unwrap_or_else(|_| panic!("browse HasEncoding for {}", dt.name));
        let results = &r[0];
        assert!(
            results.status_code.is_good(),
            "browse for {} (ns={}, id={}) must succeed: {:?}",
            dt.name,
            ns,
            dt.id,
            results.status_code,
        );

        let refs = results.references.as_deref().unwrap_or_default();
        let encoding_ids: Vec<u32> = refs
            .iter()
            .filter_map(|r| r.node_id.node_id.as_u32())
            .collect();

        // Default Binary MUST always be present (Part 3 §5.8.3)
        assert!(
            encoding_ids.contains(&dt.binary_encoding_id),
            "{} must have HasEncoding → Default Binary ({}) in namespace {}, found {:?}",
            dt.name,
            dt.binary_encoding_id,
            ns,
            encoding_ids,
        );

        #[cfg(feature = "xml")]
        if let Some(xml_id) = dt.xml_encoding_id {
            assert!(
                encoding_ids.contains(&xml_id),
                "{} must have HasEncoding → Default XML ({}) when xml feature enabled, found {:?}",
                dt.name,
                xml_id,
                encoding_ids,
            );
        }

        #[cfg(feature = "json")]
        if let Some(json_id) = dt.json_encoding_id {
            assert!(
                encoding_ids.contains(&json_id),
                "{} must have HasEncoding → Default JSON ({}) when json feature enabled, found {:?}",
                dt.name,
                json_id,
                encoding_ids,
            );
        }
    }
}
