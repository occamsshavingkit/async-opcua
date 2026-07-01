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
    BrowseDescription, BrowseDirection, BrowseNextRequest, BrowseRequest, BrowseResultMask,
    DiagnosticBits, MessageSecurityMode, NodeId, ObjectId, RequestHeader, ViewDescription,
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
