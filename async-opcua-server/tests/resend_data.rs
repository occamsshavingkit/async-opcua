//! CU 3544 / OPC-10000-4 5.13.6 requires ResendData to enqueue the current
//! value of every reporting monitored item in the target subscription.

#![allow(missing_docs)]

use std::{collections::HashMap, sync::Arc, time::Duration};

use opcua_client::{ClientBuilder, DataChangeCallback, IdentityToken, MonitoredItem, Session};
use opcua_crypto::SecurityPolicy;
use opcua_server::{ServerBuilder, ServerHandle, ANONYMOUS_USER_TOKEN_ID};
use opcua_types::{
    ExtensionObject, MessageSecurityMode, MethodId, MonitoredItemCreateRequest, MonitoringMode,
    MonitoringParameters, NodeId, ObjectId, ReadValueId, StatusCode, TimestampsToReturn,
    VariableId, Variant,
};
use tokio::{net::TcpListener, sync::mpsc, task::JoinHandle};

const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const MONITORED_ITEM_COUNT: usize = 2;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resend_data_returns_good_and_resends_all_current_values() {
    tokio::time::timeout(TEST_TIMEOUT, run_resend_data_probe())
        .await
        .expect("CU 3544 ResendData probe should not hang");
}

async fn run_resend_data_probe() {
    // Given: a subscription reporting two stable ServerStatus values.
    let server = ResendDataServer::start().await;
    let (notification_tx, mut notification_rx) = mpsc::channel(4);
    let subscription_id = server
        .session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            DataChangeCallback::new(move |value, item: &MonitoredItem| {
                let _ = notification_tx.try_send((item.id(), value.value.clone()));
            }),
        )
        .await
        .expect("subscription should be created");

    let create_results = server
        .session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![
                monitored_item(VariableId::Server_ServerStatus_StartTime, 1),
                monitored_item(VariableId::Server_ServerStatus_State, 2),
            ],
        )
        .await
        .expect("CreateMonitoredItems should complete");

    assert_eq!(create_results.len(), MONITORED_ITEM_COUNT);
    assert!(create_results
        .iter()
        .all(|result| result.result.status_code == StatusCode::Good));

    server.session.trigger_publish_now();
    let initial_values = receive_values(&mut notification_rx).await;

    // When: ResendData is called without changing either monitored value.
    let resend_result = server
        .session
        .call_one((
            NodeId::from(ObjectId::Server),
            NodeId::from(MethodId::Server_ResendData),
            Some(vec![Variant::from(subscription_id)]),
        ))
        .await
        .expect("ResendData Call service should complete");
    server.session.trigger_publish_now();

    // Then: CU 3544 requires another notification containing every current value.
    assert_eq!(resend_result.status_code, StatusCode::Good);
    let resent_values = receive_values(&mut notification_rx).await;
    assert_eq!(resent_values, initial_values);
}

async fn receive_values(
    receiver: &mut mpsc::Receiver<(u32, Option<Variant>)>,
) -> HashMap<u32, Option<Variant>> {
    let mut values = HashMap::with_capacity(MONITORED_ITEM_COUNT);
    for _ in 0..MONITORED_ITEM_COUNT {
        let (monitored_item_id, value) = tokio::time::timeout(TEST_TIMEOUT, receiver.recv())
            .await
            .expect("Publish should deliver a monitored-item notification")
            .expect("notification channel should remain open");
        assert!(
            value.is_some(),
            "notification should contain a current value"
        );
        values.insert(monitored_item_id, value);
    }
    values
}

fn monitored_item(node: VariableId, client_handle: u32) -> MonitoredItemCreateRequest {
    MonitoredItemCreateRequest::new(
        ReadValueId::from(<VariableId as Into<NodeId>>::into(node)),
        MonitoringMode::Reporting,
        MonitoringParameters {
            client_handle,
            sampling_interval: 50.0,
            filter: ExtensionObject::null(),
            queue_size: 1,
            discard_oldest: true,
        },
    )
}

struct ResendDataServer {
    handle: ServerHandle,
    session: Arc<Session>,
    event_loop_task: JoinHandle<StatusCode>,
    server_task: JoinHandle<()>,
    _temp_dir: tempfile::TempDir,
}

impl ResendDataServer {
    async fn start() -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("resend-data")
            .tempdir()
            .expect("temporary ResendData test dir should be created");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ResendData test listener should bind");
        let addr = listener
            .local_addr()
            .expect("ResendData test listener should have an address");
        let endpoint = format!("opc.tcp://127.0.0.1:{}/", addr.port());

        let (server, handle) = ServerBuilder::new()
            .application_name("resend_data_test")
            .application_uri("urn:async-opcua:resend-data-test")
            .product_uri("urn:async-opcua:resend-data-test")
            .host("127.0.0.1")
            .pki_dir(temp_dir.path().join("server-pki"))
            .create_sample_keypair(true)
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
            .expect("ResendData test server should build");
        let server_task = tokio::spawn(async move {
            server
                .run_with(listener)
                .await
                .expect("ResendData test server should run");
        });

        let mut client = ClientBuilder::new()
            .application_name("resend_data_test_client")
            .application_uri("urn:async-opcua:resend-data-test-client")
            .product_uri("urn:async-opcua:resend-data-test-client")
            .pki_dir(temp_dir.path().join("client-pki"))
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_initial(Duration::from_millis(100))
            .client()
            .expect("ResendData test client should build");
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
            .expect("ResendData test client should connect");
        let event_loop_task = event_loop.spawn();

        tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
            .await
            .expect("ResendData test client should become connected");

        Self {
            handle,
            session,
            event_loop_task,
            server_task,
            _temp_dir: temp_dir,
        }
    }
}

impl Drop for ResendDataServer {
    fn drop(&mut self) {
        self.handle.cancel();
        self.event_loop_task.abort();
        self.server_task.abort();
    }
}
