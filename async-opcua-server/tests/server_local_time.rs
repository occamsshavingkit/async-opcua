//! Integration test for CU 2476 — Server_LocalTime (TimeZoneDataType).
//! OPC-10000-5 §6.3.3 / §12.2.12.11.
//!
//! Reads the Server_LocalTime Property from the Server object and asserts
//! the returned TimeZoneDataType is plausible.

use std::time::Duration;

use opcua_client::{ClientBuilder, IdentityToken};
use opcua_server::{ServerBuilder, ANONYMOUS_USER_TOKEN_ID};
use opcua_types::{
    match_extension_object, AttributeId, EndpointDescription, MessageSecurityMode, ReadValueId,
    TimeZoneDataType, TimestampsToReturn, UserTokenPolicy, VariableId, Variant,
};
use tokio::net::TcpListener;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_server_local_time_returns_plausible_timezone() {
    tokio::time::timeout(TEST_TIMEOUT, run())
        .await
        .expect("server local time test should not hang");
}

async fn run() {
    let temp_dir = tempfile::Builder::new()
        .prefix("server-local-time")
        .tempdir()
        .expect("temporary test dir should be created");
    let server_pki = temp_dir.path().join("server-pki");
    let client_pki = temp_dir.path().join("client-pki");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener should bind");
    let addr = listener
        .local_addr()
        .expect("test listener should have an address");
    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let (server, handle) = ServerBuilder::new()
        .application_name("server_local_time_test")
        .application_uri("urn:async-opcua:server-local-time-test")
        .product_uri("urn:async-opcua:server-local-time-test")
        .host("127.0.0.1")
        .pki_dir(server_pki)
        .create_sample_keypair(true)
        .trust_client_certs(true)
        .discovery_urls(vec![endpoint_url.clone()])
        .add_endpoint(
            "none",
            (
                "/",
                opcua_crypto::SecurityPolicy::None,
                MessageSecurityMode::None,
                &[ANONYMOUS_USER_TOKEN_ID] as &[&str],
            ),
        )
        .build()
        .expect("test server should build");

    let server_task = tokio::spawn(async move {
        server
            .run_with(listener)
            .await
            .expect("test server should run");
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut client = ClientBuilder::new()
        .application_name("server_local_time_client")
        .application_uri("urn:async-opcua:server-local-time-client")
        .product_uri("urn:async-opcua:server-local-time-client")
        .pki_dir(client_pki)
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_initial(Duration::from_millis(100))
        .client()
        .expect("test client should build");

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
        .expect("test client should connect");
    let event_loop_task = event_loop.spawn();

    session.wait_for_connection().await;

    let result = session
        .read(
            &[ReadValueId::new(
                VariableId::Server_LocalTime.into(),
                AttributeId::Value,
            )],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("read Server_LocalTime should succeed");

    assert_eq!(result.len(), 1, "expected one DataValue in read result");
    let dv = &result[0];

    let status = dv
        .status
        .expect("Server_LocalTime DataValue should have a status code");
    assert!(
        status.is_good(),
        "Server_LocalTime status should be good, got {status:?}"
    );

    let value = dv
        .value
        .as_ref()
        .expect("Server_LocalTime DataValue should have a value");

    let Variant::ExtensionObject(ref obj) = value else {
        panic!(
            "Server_LocalTime value should be an ExtensionObject, got {:?}",
            value
        );
    };

    assert!(
        !obj.is_null(),
        "Server_LocalTime ExtensionObject should not be null"
    );

    let obj_id = obj
        .object_id()
        .expect("Server_LocalTime ExtensionObject should have an object ID");
    assert_eq!(
        obj_id,
        opcua_types::ObjectId::TimeZoneDataType_Encoding_DefaultBinary,
        "Server_LocalTime should be encoded as TimeZoneDataType_Encoding_DefaultBinary"
    );

    match_extension_object!(obj,
        tz: TimeZoneDataType => {
            assert!(
                (-720..=840).contains(&tz.offset),
                "TimeZoneDataType.offset ({}) should be in plausible range [-720, +840]",
                tz.offset
            );
            // daylight_saving_in_offset is a bool — both true and false are valid.
            // We just check that reading succeeds (the struct is round-tripped correctly).
        },
        _ => panic!(
            "Server_LocalTime ExtensionObject should contain TimeZoneDataType, got {:?}",
            obj.type_name()
        ),
    );

    handle.cancel();
    event_loop_task.abort();
    server_task.abort();
}
