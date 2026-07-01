//! Per-operation diagnosticInfos integration tests (feature 050, conformance P4-GEN-01).
//!
//! Contract under test (OPC UA Part 4 §5.2/§5.3): when the client requests per-operation
//! diagnostics via `RequestHeader.returnDiagnostics`, the response's per-op `diagnosticInfos`
//! array is present and positionally aligned with the operation results; when not requested,
//! the array is absent. Entry *content* is deliberately not asserted — built-in node managers
//! leave entries default/empty (the extension point), matching Read/Call/Write.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use opcua_client::{services::Read, ClientBuilder, IdentityToken, Session};
use opcua_core::ResponseMessage;
use opcua_crypto::SecurityPolicy;
use opcua_server::{ServerBuilder, ServerEndpoint, ServerHandle, ANONYMOUS_USER_TOKEN_ID};
use opcua_types::{
    AttributeId, BrowseDescription, BrowseDirection, BrowseNextRequest, BrowseRequest,
    BrowseResultMask, ContentFilter, CreateMonitoredItemsRequest, CreateSubscriptionRequest,
    DateTime, DeleteMonitoredItemsRequest, DeleteRawModifiedDetails, DiagnosticBits,
    ExtensionObject, HistoryReadRequest, HistoryReadValueId, HistoryUpdateRequest,
    MessageSecurityMode, ModifyMonitoredItemsRequest, MonitoredItemCreateRequest,
    MonitoredItemModifyRequest, MonitoringMode, MonitoringParameters, NodeId, NodeTypeDescription,
    NumericRange, ObjectId, ObjectTypeId, QualifiedName, QueryDataDescription, QueryFirstRequest,
    ReadRawModifiedDetails, ReadValueId, RelativePath, RequestHeader, SetMonitoringModeRequest,
    SetTriggeringRequest, TimestampsToReturn, VariableId, ViewDescription,
};
use tokio::net::TcpListener;

static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

/// Any operational-level bit requests the per-op array.
const OP_BITS: DiagnosticBits = DiagnosticBits::OPERATIONAL_LEVEL_SYMBOLIC_ID;

struct TestServer {
    #[allow(dead_code)]
    handle: ServerHandle,
    session: Arc<Session>,
    event_loop_task: tokio::task::JoinHandle<opcua_types::StatusCode>,
    server_task: tokio::task::JoinHandle<()>,
    _temp_dir: TempDir,
}

impl TestServer {
    async fn start(test_name: &str) -> Self {
        let temp_dir = TempDir::new(test_name);
        let server_pki = temp_dir.path.join("server-pki");
        let client_pki = temp_dir.path.join("client-pki");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("per-op diagnostics test listener should bind");
        let addr = listener
            .local_addr()
            .expect("per-op diagnostics test listener should have an address");
        let endpoint = format!("opc.tcp://127.0.0.1:{}/", addr.port());
        let user_token_ids = [ANONYMOUS_USER_TOKEN_ID];

        let (server, handle) = ServerBuilder::new()
            .application_name("per_op_diagnostics_tests")
            .application_uri("urn:async-opcua:per-op-diagnostics-tests")
            .product_uri("urn:async-opcua:per-op-diagnostics-tests")
            .host("127.0.0.1")
            .pki_dir(&server_pki)
            .create_sample_keypair(true)
            .discovery_urls(vec![endpoint.clone()])
            .add_endpoint(
                "none",
                ServerEndpoint::new_none("/", &user_token_ids.map(str::to_string)),
            )
            .build()
            .expect("per-op diagnostics test server should build");
        let server_task = tokio::spawn(async move {
            server
                .run_with(listener)
                .await
                .expect("per-op diagnostics test server should run");
        });

        let mut client = ClientBuilder::new()
            .application_name("per_op_diagnostics_tests_client")
            .application_uri("urn:async-opcua:per-op-diagnostics-tests-client")
            .product_uri("urn:async-opcua:per-op-diagnostics-tests-client")
            .pki_dir(client_pki)
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_initial(Duration::from_millis(100))
            .client()
            .expect("per-op diagnostics test client should build");

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
            .expect("per-op diagnostics test client should connect");
        let event_loop_task = event_loop.spawn();

        tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
            .await
            .expect("per-op diagnostics test client should become connected");

        Self {
            handle,
            session,
            event_loop_task,
            server_task,
            _temp_dir: temp_dir,
        }
    }

    /// A valid request header for this session with the given `returnDiagnostics`.
    fn request_header(&self, diagnostics: DiagnosticBits) -> RequestHeader {
        let mut header = Read::new(&self.session)
            .diagnostics(diagnostics)
            .header()
            .clone();
        header.return_diagnostics = diagnostics;
        header
    }

    async fn send(&self, request: impl Into<opcua_core::RequestMessage>) -> ResponseMessage {
        self.session
            .channel()
            .send(request.into(), Duration::from_secs(5))
            .await
            .expect("per-op diagnostics test request should receive a response")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.cancel();
        self.event_loop_task.abort();
        self.server_task.abort();
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(test_name: &str) -> Self {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::current_dir()
            .expect("current directory")
            .join("target")
            .join("per_op_diagnostics_tests")
            .join(format!("{test_name}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary per-op diagnostics dir should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn browse_description(node_id: NodeId) -> BrowseDescription {
    BrowseDescription {
        node_id,
        browse_direction: BrowseDirection::Forward,
        reference_type_id: NodeId::null(),
        include_subtypes: true,
        node_class_mask: 0,
        result_mask: BrowseResultMask::All as u32,
    }
}

// ---------------------------------------------------------------------------
// US1: Browse & BrowseNext
// ---------------------------------------------------------------------------

#[tokio::test]
async fn browse_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("browse-per-op-diagnostics").await;

    // Two operations with mixed outcomes: a valid node and an unknown node, so
    // alignment is checked against a result array that is not uniformly good.
    let nodes_to_browse = vec![
        browse_description(ObjectId::Server.into()),
        browse_description(NodeId::new(1, "per-op-diagnostics-unknown-node")),
    ];

    // 1) REQUESTED: per-op diagnostics asked for -> aligned array present.
    let request = BrowseRequest {
        request_header: server.request_header(OP_BITS),
        view: ViewDescription::default(),
        requested_max_references_per_node: 0,
        nodes_to_browse: Some(nodes_to_browse.clone()),
    };
    let ResponseMessage::Browse(response) = server.send(request).await else {
        panic!("Browse should return BrowseResponse");
    };
    let results = response
        .results
        .as_ref()
        .expect("Browse should return results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(
        diags.len(),
        results.len(),
        "per-op diagnosticInfos must be aligned with results"
    );

    // 2) NOT REQUESTED: default -> no array (no regression).
    let request = BrowseRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        view: ViewDescription::default(),
        requested_max_references_per_node: 0,
        nodes_to_browse: Some(nodes_to_browse),
    };
    let ResponseMessage::Browse(response) = server.send(request).await else {
        panic!("Browse should return BrowseResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(
        response.diagnostic_infos.is_none(),
        "per-op diagnosticInfos must be absent when not requested"
    );
}

#[tokio::test]
async fn browse_next_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("browse-next-per-op-diagnostics").await;

    // Obtain a fresh continuation point: the Server object has far more than one
    // forward reference, so max-1-reference browses always leave a remainder.
    async fn continuation_point(server: &TestServer) -> opcua_types::ByteString {
        let request = BrowseRequest {
            request_header: server.request_header(DiagnosticBits::empty()),
            view: ViewDescription::default(),
            requested_max_references_per_node: 1,
            nodes_to_browse: Some(vec![browse_description(ObjectId::Server.into())]),
        };
        let ResponseMessage::Browse(response) = server.send(request).await else {
            panic!("Browse should return BrowseResponse");
        };
        let cp = response.results.as_ref().expect("results")[0]
            .continuation_point
            .clone();
        assert!(
            !cp.is_null_or_empty(),
            "setup Browse should return a continuation point"
        );
        cp
    }

    // 1) REQUESTED: aligned array present.
    let cp = continuation_point(&server).await;
    let request = BrowseNextRequest {
        request_header: server.request_header(OP_BITS),
        release_continuation_points: false,
        continuation_points: Some(vec![cp]),
    };
    let ResponseMessage::BrowseNext(response) = server.send(request).await else {
        panic!("BrowseNext should return BrowseNextResponse");
    };
    let results = response
        .results
        .as_ref()
        .expect("BrowseNext should return results");
    assert_eq!(results.len(), 1);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(
        diags.len(),
        results.len(),
        "per-op diagnosticInfos must be aligned with results"
    );

    // 2) NOT REQUESTED: no array (no regression).
    let cp = continuation_point(&server).await;
    let request = BrowseNextRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        release_continuation_points: false,
        continuation_points: Some(vec![cp]),
    };
    let ResponseMessage::BrowseNext(response) = server.send(request).await else {
        panic!("BrowseNext should return BrowseNextResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(1));
    assert!(
        response.diagnostic_infos.is_none(),
        "per-op diagnosticInfos must be absent when not requested"
    );
}

// ---------------------------------------------------------------------------
// US2: MonitoredItems service group
// ---------------------------------------------------------------------------

async fn create_subscription(server: &TestServer) -> u32 {
    let request = CreateSubscriptionRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        requested_publishing_interval: 100.0,
        requested_lifetime_count: 100,
        requested_max_keep_alive_count: 30,
        max_notifications_per_publish: 0,
        publishing_enabled: true,
        priority: 0,
    };
    let ResponseMessage::CreateSubscription(response) = server.send(request).await else {
        panic!("CreateSubscription should return CreateSubscriptionResponse");
    };
    response.subscription_id
}

fn item_to_create(node_id: NodeId, client_handle: u32) -> MonitoredItemCreateRequest {
    MonitoredItemCreateRequest {
        item_to_monitor: ReadValueId::new(node_id, AttributeId::Value),
        monitoring_mode: MonitoringMode::Reporting,
        requested_parameters: MonitoringParameters {
            client_handle,
            sampling_interval: 100.0,
            filter: ExtensionObject::null(),
            queue_size: 1,
            discard_oldest: true,
        },
    }
}

/// Create two valid monitored items on the subscription and return their server ids.
async fn create_two_items(server: &TestServer, subscription_id: u32) -> Vec<u32> {
    let request = CreateMonitoredItemsRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        subscription_id,
        timestamps_to_return: TimestampsToReturn::Both,
        items_to_create: Some(vec![
            item_to_create(VariableId::Server_ServerStatus_CurrentTime.into(), 1),
            item_to_create(VariableId::Server_ServerStatus_State.into(), 2),
        ]),
    };
    let ResponseMessage::CreateMonitoredItems(response) = server.send(request).await else {
        panic!("CreateMonitoredItems should return CreateMonitoredItemsResponse");
    };
    let results = response.results.expect("create results");
    results
        .iter()
        .map(|r| {
            assert!(r.status_code.is_good(), "setup item should be created");
            r.monitored_item_id
        })
        .collect()
}

#[tokio::test]
async fn create_monitored_items_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("create-monitored-items-per-op-diagnostics").await;
    let subscription_id = create_subscription(&server).await;

    // Mixed outcomes: one valid node, one unknown node.
    let items = || {
        Some(vec![
            item_to_create(VariableId::Server_ServerStatus_CurrentTime.into(), 1),
            item_to_create(NodeId::new(1, "per-op-diagnostics-unknown-node"), 2),
        ])
    };

    // 1) REQUESTED -> aligned array present.
    let request = CreateMonitoredItemsRequest {
        request_header: server.request_header(OP_BITS),
        subscription_id,
        timestamps_to_return: TimestampsToReturn::Both,
        items_to_create: items(),
    };
    let ResponseMessage::CreateMonitoredItems(response) = server.send(request).await else {
        panic!("CreateMonitoredItems should return CreateMonitoredItemsResponse");
    };
    let results = response.results.as_ref().expect("results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(diags.len(), results.len());

    // 2) NOT REQUESTED -> no array.
    let request = CreateMonitoredItemsRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        subscription_id,
        timestamps_to_return: TimestampsToReturn::Both,
        items_to_create: items(),
    };
    let ResponseMessage::CreateMonitoredItems(response) = server.send(request).await else {
        panic!("CreateMonitoredItems should return CreateMonitoredItemsResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(response.diagnostic_infos.is_none());
}

#[tokio::test]
async fn modify_monitored_items_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("modify-monitored-items-per-op-diagnostics").await;
    let subscription_id = create_subscription(&server).await;
    let item_ids = create_two_items(&server, subscription_id).await;

    // Mixed outcomes: one valid item, one unknown item id.
    let items = |ids: &[u32]| {
        Some(vec![
            MonitoredItemModifyRequest {
                monitored_item_id: ids[0],
                requested_parameters: MonitoringParameters {
                    client_handle: 1,
                    sampling_interval: 200.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            },
            MonitoredItemModifyRequest {
                monitored_item_id: u32::MAX,
                requested_parameters: MonitoringParameters {
                    client_handle: 2,
                    sampling_interval: 200.0,
                    filter: ExtensionObject::null(),
                    queue_size: 1,
                    discard_oldest: true,
                },
            },
        ])
    };

    // 1) REQUESTED -> aligned array present.
    let request = ModifyMonitoredItemsRequest {
        request_header: server.request_header(OP_BITS),
        subscription_id,
        timestamps_to_return: TimestampsToReturn::Both,
        items_to_modify: items(&item_ids),
    };
    let ResponseMessage::ModifyMonitoredItems(response) = server.send(request).await else {
        panic!("ModifyMonitoredItems should return ModifyMonitoredItemsResponse");
    };
    let results = response.results.as_ref().expect("results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(diags.len(), results.len());

    // 2) NOT REQUESTED -> no array.
    let request = ModifyMonitoredItemsRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        subscription_id,
        timestamps_to_return: TimestampsToReturn::Both,
        items_to_modify: items(&item_ids),
    };
    let ResponseMessage::ModifyMonitoredItems(response) = server.send(request).await else {
        panic!("ModifyMonitoredItems should return ModifyMonitoredItemsResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(response.diagnostic_infos.is_none());
}

#[tokio::test]
async fn set_monitoring_mode_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("set-monitoring-mode-per-op-diagnostics").await;
    let subscription_id = create_subscription(&server).await;
    let item_ids = create_two_items(&server, subscription_id).await;

    // 1) REQUESTED -> aligned array present. Mixed outcomes: valid + unknown id.
    let request = SetMonitoringModeRequest {
        request_header: server.request_header(OP_BITS),
        subscription_id,
        monitoring_mode: MonitoringMode::Sampling,
        monitored_item_ids: Some(vec![item_ids[0], u32::MAX]),
    };
    let ResponseMessage::SetMonitoringMode(response) = server.send(request).await else {
        panic!("SetMonitoringMode should return SetMonitoringModeResponse");
    };
    let results = response.results.as_ref().expect("results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(diags.len(), results.len());

    // 2) NOT REQUESTED -> no array.
    let request = SetMonitoringModeRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        subscription_id,
        monitoring_mode: MonitoringMode::Reporting,
        monitored_item_ids: Some(vec![item_ids[0], u32::MAX]),
    };
    let ResponseMessage::SetMonitoringMode(response) = server.send(request).await else {
        panic!("SetMonitoringMode should return SetMonitoringModeResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(response.diagnostic_infos.is_none());
}

#[tokio::test]
async fn set_triggering_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("set-triggering-per-op-diagnostics").await;
    let subscription_id = create_subscription(&server).await;
    let item_ids = create_two_items(&server, subscription_id).await;

    // 1) REQUESTED -> BOTH arrays aligned to their respective request arrays.
    // links_to_add has mixed outcomes (valid + unknown), links_to_remove one unknown.
    let request = SetTriggeringRequest {
        request_header: server.request_header(OP_BITS),
        subscription_id,
        triggering_item_id: item_ids[0],
        links_to_add: Some(vec![item_ids[1], u32::MAX]),
        links_to_remove: Some(vec![u32::MAX - 1]),
    };
    let ResponseMessage::SetTriggering(response) = server.send(request).await else {
        panic!("SetTriggering should return SetTriggeringResponse");
    };
    let add_results = response.add_results.as_ref().expect("add results");
    assert_eq!(add_results.len(), 2);
    let add_diags = response
        .add_diagnostic_infos
        .as_ref()
        .expect("add diagnosticInfos must be present when requested");
    assert_eq!(add_diags.len(), add_results.len());
    let remove_results = response.remove_results.as_ref().expect("remove results");
    assert_eq!(remove_results.len(), 1);
    let remove_diags = response
        .remove_diagnostic_infos
        .as_ref()
        .expect("remove diagnosticInfos must be present when requested");
    assert_eq!(remove_diags.len(), remove_results.len());

    // 2) NOT REQUESTED -> neither array.
    let request = SetTriggeringRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        subscription_id,
        triggering_item_id: item_ids[0],
        links_to_add: Some(vec![item_ids[1], u32::MAX]),
        links_to_remove: Some(vec![u32::MAX - 1]),
    };
    let ResponseMessage::SetTriggering(response) = server.send(request).await else {
        panic!("SetTriggering should return SetTriggeringResponse");
    };
    assert!(response.add_diagnostic_infos.is_none());
    assert!(response.remove_diagnostic_infos.is_none());
}

#[tokio::test]
async fn delete_monitored_items_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("delete-monitored-items-per-op-diagnostics").await;
    let subscription_id = create_subscription(&server).await;
    let item_ids = create_two_items(&server, subscription_id).await;

    // 1) REQUESTED -> aligned array present. Mixed outcomes: valid + unknown id.
    let request = DeleteMonitoredItemsRequest {
        request_header: server.request_header(OP_BITS),
        subscription_id,
        monitored_item_ids: Some(vec![item_ids[0], u32::MAX]),
    };
    let ResponseMessage::DeleteMonitoredItems(response) = server.send(request).await else {
        panic!("DeleteMonitoredItems should return DeleteMonitoredItemsResponse");
    };
    let results = response.results.as_ref().expect("results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(diags.len(), results.len());

    // 2) NOT REQUESTED -> no array.
    let request = DeleteMonitoredItemsRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        subscription_id,
        monitored_item_ids: Some(vec![item_ids[1], u32::MAX]),
    };
    let ResponseMessage::DeleteMonitoredItems(response) = server.send(request).await else {
        panic!("DeleteMonitoredItems should return DeleteMonitoredItemsResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(response.diagnostic_infos.is_none());
}

// ---------------------------------------------------------------------------
// US3: HistoryRead & HistoryUpdate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn history_read_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("history-read-per-op-diagnostics").await;

    // The default server exposes no history, so per-op results carry bad
    // statuses — the structural contract (alignment/gating) is unaffected.
    let details = || {
        ExtensionObject::from_message(ReadRawModifiedDetails {
            is_read_modified: false,
            start_time: DateTime::epoch(),
            end_time: DateTime::now(),
            num_values_per_node: 10,
            return_bounds: false,
        })
    };
    let nodes_to_read = || {
        Some(vec![
            HistoryReadValueId {
                node_id: VariableId::Server_ServerStatus_CurrentTime.into(),
                index_range: NumericRange::None,
                data_encoding: QualifiedName::null(),
                continuation_point: Default::default(),
            },
            HistoryReadValueId {
                node_id: NodeId::new(1, "per-op-diagnostics-unknown-node"),
                index_range: NumericRange::None,
                data_encoding: QualifiedName::null(),
                continuation_point: Default::default(),
            },
        ])
    };

    // 1) REQUESTED -> aligned array present.
    let request = HistoryReadRequest {
        request_header: server.request_header(OP_BITS),
        history_read_details: details(),
        timestamps_to_return: TimestampsToReturn::Both,
        release_continuation_points: false,
        nodes_to_read: nodes_to_read(),
    };
    let ResponseMessage::HistoryRead(response) = server.send(request).await else {
        panic!("HistoryRead should return HistoryReadResponse");
    };
    let results = response.results.as_ref().expect("results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(diags.len(), results.len());

    // 2) NOT REQUESTED -> no array.
    let request = HistoryReadRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        history_read_details: details(),
        timestamps_to_return: TimestampsToReturn::Both,
        release_continuation_points: false,
        nodes_to_read: nodes_to_read(),
    };
    let ResponseMessage::HistoryRead(response) = server.send(request).await else {
        panic!("HistoryRead should return HistoryReadResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(response.diagnostic_infos.is_none());
}

#[tokio::test]
async fn history_update_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("history-update-per-op-diagnostics").await;

    // Two update operations (delete-raw on a known and an unknown node); the
    // default server has no history backend, so statuses are bad but aligned.
    let details = || {
        Some(vec![
            ExtensionObject::from_message(DeleteRawModifiedDetails {
                node_id: VariableId::Server_ServerStatus_CurrentTime.into(),
                is_delete_modified: false,
                start_time: DateTime::epoch(),
                end_time: DateTime::now(),
            }),
            ExtensionObject::from_message(DeleteRawModifiedDetails {
                node_id: NodeId::new(1, "per-op-diagnostics-unknown-node"),
                is_delete_modified: false,
                start_time: DateTime::epoch(),
                end_time: DateTime::now(),
            }),
        ])
    };

    // 1) REQUESTED -> aligned array present.
    let request = HistoryUpdateRequest {
        request_header: server.request_header(OP_BITS),
        history_update_details: details(),
    };
    let ResponseMessage::HistoryUpdate(response) = server.send(request).await else {
        panic!("HistoryUpdate should return HistoryUpdateResponse");
    };
    let results = response.results.as_ref().expect("results");
    assert_eq!(results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("per-op diagnosticInfos must be present when requested");
    assert_eq!(diags.len(), results.len());

    // 2) NOT REQUESTED -> no array.
    let request = HistoryUpdateRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        history_update_details: details(),
    };
    let ResponseMessage::HistoryUpdate(response) = server.send(request).await else {
        panic!("HistoryUpdate should return HistoryUpdateResponse");
    };
    assert_eq!(response.results.as_ref().map(Vec::len), Some(2));
    assert!(response.diagnostic_infos.is_none());
}

// ---------------------------------------------------------------------------
// US4: QueryFirst (QueryNext has no diagnosticInfos on the wire, Part 4 B.2.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn query_first_returns_aligned_diagnostic_infos_only_when_requested() {
    let server = TestServer::start("query-first-per-op-diagnostics").await;

    // Two node types: one parseable, one whose data description has an invalid
    // attribute id, so parsing fails -> the response carries parsingResults
    // (one per requested NodeTypeDescription, Part 4 B.2.3) and the op-level
    // diagnosticInfos must align with them. The failing entry also carries
    // nested data_status_codes, whose data_diagnostic_infos follow the same
    // bits-gated rule.
    let node_types = || {
        Some(vec![
            NodeTypeDescription {
                type_definition_node: NodeId::from(ObjectTypeId::BaseObjectType).into(),
                include_sub_types: true,
                data_to_return: Some(vec![QueryDataDescription {
                    relative_path: RelativePath { elements: None },
                    attribute_id: AttributeId::Value as u32,
                    index_range: NumericRange::None,
                }]),
            },
            NodeTypeDescription {
                type_definition_node: NodeId::from(ObjectTypeId::BaseObjectType).into(),
                include_sub_types: true,
                data_to_return: Some(vec![QueryDataDescription {
                    relative_path: RelativePath { elements: None },
                    attribute_id: 0, // invalid -> BadAttributeIdInvalid
                    index_range: NumericRange::None,
                }]),
            },
        ])
    };

    // 1) REQUESTED -> op-level array aligned with parsingResults, nested
    //    data_diagnostic_infos aligned with data_status_codes.
    let request = QueryFirstRequest {
        request_header: server.request_header(OP_BITS),
        view: ViewDescription::default(),
        node_types: node_types(),
        filter: ContentFilter { elements: None },
        max_data_sets_to_return: 100,
        max_references_to_return: 100,
    };
    let ResponseMessage::QueryFirst(response) = server.send(request).await else {
        panic!("QueryFirst should return QueryFirstResponse");
    };
    let parsing_results = response
        .parsing_results
        .as_ref()
        .expect("parsing results on the parse-failure path");
    assert_eq!(parsing_results.len(), 2);
    let diags = response
        .diagnostic_infos
        .as_ref()
        .expect("op-level diagnosticInfos must be present when requested");
    assert_eq!(
        diags.len(),
        parsing_results.len(),
        "op-level diagnosticInfos must align with parsingResults (one per NodeTypeDescription)"
    );
    let failed = &parsing_results[1];
    let data_status_codes = failed
        .data_status_codes
        .as_ref()
        .expect("failing parsing result carries data status codes");
    let data_diags = failed
        .data_diagnostic_infos
        .as_ref()
        .expect("nested data_diagnostic_infos must be present when requested");
    assert_eq!(
        data_diags.len(),
        data_status_codes.len(),
        "nested data_diagnostic_infos must align with data_status_codes"
    );

    // 2) NOT REQUESTED -> neither level carries an array.
    let request = QueryFirstRequest {
        request_header: server.request_header(DiagnosticBits::empty()),
        view: ViewDescription::default(),
        node_types: node_types(),
        filter: ContentFilter { elements: None },
        max_data_sets_to_return: 100,
        max_references_to_return: 100,
    };
    let ResponseMessage::QueryFirst(response) = server.send(request).await else {
        panic!("QueryFirst should return QueryFirstResponse");
    };
    let parsing_results = response
        .parsing_results
        .as_ref()
        .expect("parsing results on the parse-failure path");
    assert_eq!(parsing_results.len(), 2);
    assert!(
        response.diagnostic_infos.is_none(),
        "op-level diagnosticInfos must be absent when not requested"
    );
    assert!(
        parsing_results[1].data_diagnostic_infos.is_none(),
        "nested data_diagnostic_infos must be absent when not requested"
    );
}
