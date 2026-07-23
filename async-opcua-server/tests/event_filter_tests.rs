//! Event filter integration tests.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use opcua_client::{services::Read, ClientBuilder, EventCallback, IdentityToken, Session};
use opcua_core::ResponseMessage;
use opcua_crypto::SecurityPolicy;
use opcua_nodes::{BaseEventType, DefaultTypeTree, Event};
use opcua_server::{
    services::subscription::filter::ParsedEventFilter, IdentityMappingRule, ServerBuilder,
    ServerEndpoint, ServerHandle, ServerUserToken, WellKnownRole, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::CallMethodRequest;
use opcua_types::{
    match_extension_object, AttributeId, ByteString, ContentFilter, ContentFilterBuilder,
    ContentFilterElement, EventFilter, ExtensionObject, FilterOperator, IdentityCriteriaType,
    IdentityMappingRuleType, MessageSecurityMode, MethodId, MonitoredItemCreateRequest,
    MonitoringMode, MonitoringParameters, NodeId, NumericRange, ObjectId, ObjectTypeId, Operand,
    ReadRequest, ReadValueId, SimpleAttributeOperand, StatusCode, TimeZoneDataType,
    TimestampsToReturn, UAString, Variant,
};
use tokio::{net::TcpListener, sync::mpsc};

static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

struct EventFilterServer {
    handle: ServerHandle,
    session: Arc<Session>,
    endpoint: String,
    event_loop_task: tokio::task::JoinHandle<StatusCode>,
    server_task: tokio::task::JoinHandle<()>,
    _temp_dir: TempDir,
}

impl EventFilterServer {
    async fn start(test_name: &str) -> Self {
        let temp_dir = TempDir::new(test_name);
        let server_pki = temp_dir.path.join("server-pki");
        let client_pki = temp_dir.path.join("client-pki");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("event filter test listener should bind");
        let addr = listener
            .local_addr()
            .expect("test listener should have addr");
        let endpoint = format!("opc.tcp://127.0.0.1:{}/", addr.port());
        let user_token_ids = [ANONYMOUS_USER_TOKEN_ID, "password-user"];

        let (server, handle) = ServerBuilder::new()
            .application_name("event_filter_tests")
            .application_uri("urn:async-opcua:event-filter-tests")
            .product_uri("urn:async-opcua:event-filter-tests")
            .host("127.0.0.1")
            .pki_dir(&server_pki)
            .create_sample_keypair(true)
            .discovery_urls(vec![endpoint.clone()])
            .add_user_token(
                "password-user",
                ServerUserToken::user_pass("brew-operator", "correct-password"),
            )
            .add_endpoint(
                "none",
                ServerEndpoint::new_none("/", &user_token_ids.map(str::to_string)),
            )
            .build()
            .expect("event filter test server should build");
        let server_task = tokio::spawn(async move {
            server
                .run_with(listener)
                .await
                .expect("event filter test server should run");
        });

        let mut client = ClientBuilder::new()
            .application_name("event_filter_tests_client")
            .application_uri("urn:async-opcua:event-filter-tests-client")
            .product_uri("urn:async-opcua:event-filter-tests-client")
            .pki_dir(client_pki)
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_initial(Duration::from_millis(100))
            .client()
            .expect("event filter test client should build");

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
            .expect("event filter test client should connect");
        let event_loop_task = event_loop.spawn();

        tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
            .await
            .expect("event filter test client should become connected");

        Self {
            handle,
            session,
            endpoint,
            event_loop_task,
            server_task,
            _temp_dir: temp_dir,
        }
    }

    fn client(&self, pki_name: &str) -> opcua_client::Client {
        ClientBuilder::new()
            .application_name("event_filter_tests_client")
            .application_uri(format!(
                "urn:async-opcua:event-filter-tests-client:{pki_name}"
            ))
            .product_uri("urn:async-opcua:event-filter-tests-client")
            .pki_dir(self._temp_dir.path.join(pki_name))
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_limit(0)
            .session_retry_initial(Duration::from_millis(10))
            .client()
            .expect("event filter test client should build")
    }
}

impl Drop for EventFilterServer {
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
            .join("event_filter_tests")
            .join(format!("{test_name}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary event filter dir should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn event_filter_delivers_only_matching_events_with_selected_fields() {
    let server = EventFilterServer::start("matching-events").await;
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let subscription_id = server
        .session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            EventCallback::new(move |event_fields, _| {
                let _ = event_tx.send(event_fields.unwrap_or_default());
            }),
        )
        .await
        .expect("event filter subscription should be created");

    let create_results = server
        .session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![event_monitored_item(severity_filter(500))],
        )
        .await
        .expect("event monitored item request should complete");
    assert_eq!(create_results.len(), 1);
    assert_eq!(create_results[0].result.status_code, StatusCode::Good);

    let server_node = NodeId::from(ObjectId::Server);
    let low = base_event("low-severity", 100);
    let high = base_event("high-severity", 700);
    server.handle.subscriptions().notify_events(
        [
            (&low as &dyn Event, &server_node),
            (&high as &dyn Event, &server_node),
        ]
        .into_iter(),
    );
    server.session.trigger_publish_now();

    let fields = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("matching event should be published")
        .expect("matching event fields should be received");

    assert_eq!(fields.len(), 2);
    assert_eq!(localized_text(&fields[0]), Some("high-severity"));
    assert_eq!(fields[1], Variant::UInt16(700));
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn event_filter_rejects_unsupported_where_clause_operator() {
    let server = EventFilterServer::start("unsupported-operator").await;
    let subscription_id = server
        .session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            EventCallback::new(|_, _| {}),
        )
        .await
        .expect("event filter subscription should be created");

    let create_results = server
        .session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![event_monitored_item(unsupported_where_clause_filter())],
        )
        .await
        .expect("event monitored item request should complete");

    assert_eq!(create_results.len(), 1);
    assert_eq!(
        create_results[0].result.status_code,
        StatusCode::BadMonitoredItemFilterUnsupported
    );
}

#[test]
fn unsupported_event_filter_operator_returns_bad_filter_operator_unsupported() {
    let status = first_where_element_status(EventFilter {
        select_clauses: Some(vec![SimpleAttributeOperand::new_value(
            ObjectTypeId::BaseEventType,
            "Severity",
        )]),
        where_clause: ContentFilter {
            elements: Some(vec![(
                FilterOperator::RelatedTo,
                vec![
                    Operand::literal(NodeId::from(ObjectId::Server)),
                    Operand::literal(NodeId::null()),
                    Operand::literal(NodeId::null()),
                    Operand::literal(NodeId::null()),
                    Operand::literal(0u32),
                    Operand::literal(false),
                ],
            )
                .into()]),
        },
    });

    // OPC-10000-4 7.7: unsupported ContentFilter operators report BadFilterOperatorUnsupported.
    assert_eq!(status, StatusCode::BadFilterOperatorUnsupported);
}

#[test]
fn event_filter_wrong_operand_count_returns_bad_filter_operand_count_mismatch() {
    let status = first_where_element_status(EventFilter {
        select_clauses: Some(vec![SimpleAttributeOperand::new_value(
            ObjectTypeId::BaseEventType,
            "Severity",
        )]),
        where_clause: ContentFilter {
            elements: Some(vec![ContentFilterElement::from((
                FilterOperator::GreaterThanOrEqual,
                vec![Operand::literal(500u16)],
            ))]),
        },
    });

    // OPC-10000-4 7.7: wrong operand counts report BadFilterOperandCountMismatch.
    assert_eq!(status, StatusCode::BadFilterOperandCountMismatch);
}

#[test]
fn event_filter_invalid_operand_returns_bad_filter_operand_invalid() {
    let status = first_where_element_status(EventFilter {
        select_clauses: Some(vec![SimpleAttributeOperand::new_value(
            ObjectTypeId::BaseEventType,
            "Severity",
        )]),
        where_clause: ContentFilter {
            elements: Some(vec![ContentFilterElement::from((
                FilterOperator::Not,
                vec![Operand::element(1)],
            ))]),
        },
    });

    // OPC-10000-4 7.7: invalid operands report BadFilterOperandInvalid.
    assert_eq!(status, StatusCode::BadFilterOperandInvalid);
}

#[tokio::test]
async fn failed_username_activation_dispatches_audit_event() {
    let server = EventFilterServer::start("failed-auth-audit").await;
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let subscription_id = server
        .session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            EventCallback::new(move |event_fields, _| {
                let _ = event_tx.send(event_fields.unwrap_or_default());
            }),
        )
        .await
        .expect("audit subscription should be created");

    let create_results = server
        .session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![event_monitored_item(audit_failure_filter())],
        )
        .await
        .expect("audit monitored item request should complete");
    assert_eq!(create_results[0].result.status_code, StatusCode::Good);

    let mut client = server.client("failed-auth-client");
    let (bad_session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                server.endpoint.as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
            ),
            IdentityToken::new_user_name("brew-operator", "wrong-password"),
        )
        .await
        .expect("bad auth client session should be constructed");
    bad_session.disable_reconnects();

    let status = tokio::time::timeout(Duration::from_secs(5), event_loop.spawn())
        .await
        .expect("bad auth event loop should finish")
        .expect("bad auth event loop task should complete");
    assert_eq!(status, StatusCode::BadUserAccessDenied);

    server.session.trigger_publish_now();
    // The bad client's CreateSession succeeds and now emits an AuditCreateSessionEventType ahead of
    // the ActivateSession failure; skip non-activate audit events and assert on the activation one.
    let activate_type = Variant::from(NodeId::from(ObjectTypeId::AuditActivateSessionEventType));
    let mut fields = None;
    // The CloseSecureChannel added by SC-01 dispatches an extra AuditChannelEventType;
    // extend the poll window to accommodate it before reaching ActivateSession audit.
    for _ in 0..6 {
        let Ok(Some(received)) =
            tokio::time::timeout(Duration::from_secs(5), event_rx.recv()).await
        else {
            break;
        };
        if received.first() == Some(&activate_type) {
            fields = Some(received);
            break;
        }
        server.session.trigger_publish_now();
    }
    let fields = fields.expect("an AuditActivateSessionEventType failure should be published");

    assert_eq!(
        localized_text(&fields[1]),
        Some("ActivateSession failed: BadUserAccessDenied")
    );
    assert_eq!(fields[2], Variant::UInt16(900));
}

#[tokio::test]
async fn failed_service_invocation_dispatches_audit_event() {
    let server = EventFilterServer::start("failed-service-audit").await;
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let subscription_id = server
        .session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            EventCallback::new(move |event_fields, _| {
                let _ = event_tx.send(event_fields.unwrap_or_default());
            }),
        )
        .await
        .expect("audit subscription should be created");

    let create_results = server
        .session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![event_monitored_item(audit_failure_filter())],
        )
        .await
        .expect("audit monitored item request should complete");
    assert_eq!(create_results[0].result.status_code, StatusCode::Good);

    let request = ReadRequest {
        request_header: Read::new(&server.session).header().clone(),
        max_age: -1.0,
        timestamps_to_return: TimestampsToReturn::Both,
        nodes_to_read: Some(vec![ReadValueId::new(
            ObjectId::Server.into(),
            AttributeId::NodeId,
        )]),
    };
    let response = server
        .session
        .channel()
        .send(request, Duration::from_secs(5))
        .await
        .expect("invalid read request should receive a service fault");
    let ResponseMessage::ServiceFault(response) = response else {
        panic!("invalid read request should return ServiceFault");
    };
    assert_eq!(
        response.response_header.service_result,
        StatusCode::BadMaxAgeInvalid
    );

    server.session.trigger_publish_now();
    let fields = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("failed service audit event should be published")
        .expect("failed service audit event fields should be received");

    assert_eq!(
        fields[0],
        Variant::from(NodeId::from(ObjectTypeId::AuditSecurityEventType))
    );
    assert_eq!(
        localized_text(&fields[1]),
        Some("Read failed: BadMaxAgeInvalid")
    );
    assert_eq!(fields[2], Variant::UInt16(900));
}

fn event_monitored_item(filter: EventFilter) -> MonitoredItemCreateRequest {
    MonitoredItemCreateRequest::new(
        ReadValueId::new(ObjectId::Server.into(), AttributeId::EventNotifier),
        MonitoringMode::Reporting,
        MonitoringParameters {
            client_handle: 1,
            sampling_interval: 0.0,
            filter: ExtensionObject::from_message(filter),
            queue_size: 10,
            discard_oldest: true,
        },
    )
}

fn first_where_element_status(filter: EventFilter) -> StatusCode {
    let type_tree = DefaultTypeTree::new();
    let (result, parsed) = ParsedEventFilter::parse(filter, &type_tree);
    assert!(parsed.is_err());

    result
        .where_clause_result
        .element_results
        .expect("where clause element result should be present")[0]
        .status_code
}

fn severity_filter(min_severity: u16) -> EventFilter {
    let base_event_type = NodeId::from(ObjectTypeId::BaseEventType);
    EventFilter {
        select_clauses: Some(vec![
            SimpleAttributeOperand::new_value(base_event_type.clone(), "Message"),
            SimpleAttributeOperand::new_value(base_event_type.clone(), "Severity"),
        ]),
        where_clause: ContentFilterBuilder::new()
            .gte(
                Operand::simple_attribute(
                    base_event_type,
                    "Severity",
                    AttributeId::Value,
                    NumericRange::None,
                ),
                Operand::literal(min_severity),
            )
            .build(),
    }
}

fn unsupported_where_clause_filter() -> EventFilter {
    EventFilter {
        select_clauses: Some(vec![SimpleAttributeOperand::new_value(
            ObjectTypeId::BaseEventType,
            "Severity",
        )]),
        where_clause: ContentFilter {
            elements: Some(vec![(
                FilterOperator::RelatedTo,
                vec![
                    Operand::literal(NodeId::from(ObjectId::Server)),
                    Operand::literal(NodeId::null()),
                    Operand::literal(NodeId::null()),
                    Operand::literal(NodeId::null()),
                    Operand::literal(0u32),
                    Operand::literal(false),
                ],
            )
                .into()]),
        },
    }
}

fn audit_failure_filter() -> EventFilter {
    let event_type = NodeId::from(ObjectTypeId::BaseEventType);
    EventFilter {
        select_clauses: Some(vec![
            SimpleAttributeOperand::new_value(event_type.clone(), "EventType"),
            SimpleAttributeOperand::new_value(event_type.clone(), "Message"),
            SimpleAttributeOperand::new_value(event_type, "Severity"),
        ]),
        where_clause: ContentFilter::default(),
    }
}

fn base_event(message: &str, severity: u16) -> BaseEventType {
    BaseEventType::new(
        ObjectTypeId::BaseEventType,
        ByteString::from(message.as_bytes()),
        message,
        opcua_types::DateTime::now(),
    )
    .set_severity(severity)
}

fn localized_text(value: &Variant) -> Option<&str> {
    let Variant::LocalizedText(text) = value else {
        return None;
    };
    Some(text.text.as_ref())
}

/// CU 3546 — BaseEventType.local_time MUST be populated when an event is emitted.
/// OPC-10000-5 §6.4.2 BaseEventType.
#[tokio::test]
async fn emitted_event_has_populated_local_time() {
    let server = EventFilterServer::start("local-time").await;
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let subscription_id = server
        .session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            EventCallback::new(move |event_fields, _| {
                let _ = event_tx.send(event_fields.unwrap_or_default());
            }),
        )
        .await
        .expect("local-time subscription should be created");

    let base_event_type = NodeId::from(ObjectTypeId::BaseEventType);
    let filter = EventFilter {
        select_clauses: Some(vec![
            SimpleAttributeOperand::new_value(base_event_type.clone(), "Message"),
            SimpleAttributeOperand::new_value(base_event_type.clone(), "LocalTime"),
        ]),
        where_clause: ContentFilter { elements: None },
    };
    let create_results = server
        .session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![event_monitored_item(filter)],
        )
        .await
        .expect("local-time monitored item request should complete");
    assert_eq!(create_results.len(), 1);
    assert_eq!(create_results[0].result.status_code, StatusCode::Good);

    let server_node = NodeId::from(ObjectId::Server);
    let event = base_event("local-time-test", 500);
    server
        .handle
        .subscriptions()
        .notify_events(std::iter::once((&event as &dyn Event, &server_node)));
    server.session.trigger_publish_now();

    let fields = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("event with local time should be published")
        .expect("event fields should be received");

    assert_eq!(fields.len(), 2);
    assert_eq!(localized_text(&fields[0]), Some("local-time-test"));

    let local_time_var = &fields[1];
    let Variant::ExtensionObject(ref obj) = local_time_var else {
        panic!(
            "LocalTime should be an ExtensionObject, got {:?}",
            local_time_var
        );
    };
    assert!(
        !obj.is_null(),
        "LocalTime ExtensionObject should not be null"
    );
    let obj_id = obj
        .object_id()
        .expect("LocalTime ExtensionObject should have an object ID");
    assert_eq!(
        obj_id,
        opcua_types::ObjectId::TimeZoneDataType_Encoding_DefaultBinary,
        "LocalTime should be encoded as TimeZoneDataType_Encoding_DefaultBinary"
    );

    match_extension_object!(obj,
        tz: TimeZoneDataType => {
            assert!(
                (-720..=840).contains(&tz.offset),
                "TimeZoneDataType.offset ({}) should be in plausible range [-720, +840]",
                tz.offset
            );
        },
        _ => panic!(
            "LocalTime ExtensionObject should contain TimeZoneDataType, got {:?}",
            obj.type_name()
        ),
    );
}

/// CU 3194 — MaxSelectClauseParameters / MaxWhereClauseParameters ServerCapabilities
/// MUST be populated from server limits.
/// OPC-10000-4 §7.7 (event filter) + OPC-10000-5 §6.3.2 (ServerCapabilities).
#[tokio::test]
async fn max_select_where_clause_parameters_are_populated() {
    let server = EventFilterServer::start("max-clause-params").await;

    let result = server
        .session
        .read(
            &[
                ReadValueId::new(
                    NodeId::from(
                        opcua_types::VariableId::Server_ServerCapabilities_MaxSelectClauseParameters,
                    ),
                    AttributeId::Value,
                ),
                ReadValueId::new(
                    NodeId::from(
                        opcua_types::VariableId::Server_ServerCapabilities_MaxWhereClauseParameters,
                    ),
                    AttributeId::Value,
                ),
            ],
            TimestampsToReturn::Neither,
            0.0,
        )
        .await
        .expect("read MaxSelect/MaxWhereClauseParameters should succeed");

    assert_eq!(result.len(), 2);

    let dv_select = &result[0];
    let status = dv_select
        .status
        .expect("MaxSelectClauseParameters should have status");
    assert!(
        status.is_good(),
        "MaxSelectClauseParameters status should be good, got {status:?}"
    );
    let value = dv_select
        .value
        .as_ref()
        .expect("MaxSelectClauseParameters should have a value");
    let Variant::UInt32(select_val) = value else {
        panic!(
            "MaxSelectClauseParameters should be UInt32, got {:?}",
            value
        );
    };
    assert!(
        *select_val > 0,
        "MaxSelectClauseParameters should be non-zero, got {select_val}"
    );

    let dv_where = &result[1];
    let status = dv_where
        .status
        .expect("MaxWhereClauseParameters should have status");
    assert!(
        status.is_good(),
        "MaxWhereClauseParameters status should be good, got {status:?}"
    );
    let value = dv_where
        .value
        .as_ref()
        .expect("MaxWhereClauseParameters should have a value");
    let Variant::UInt32(where_val) = value else {
        panic!("MaxWhereClauseParameters should be UInt32, got {:?}", value);
    };
    assert!(
        *where_val > 0,
        "MaxWhereClauseParameters should be non-zero, got {where_val}"
    );
}

#[tokio::test]
async fn add_identity_dispatches_role_mapping_rule_changed_audit_event() {
    let temp_dir = TempDir::new("role-mapping-audit");
    let server_pki = temp_dir.path.join("server-pki");
    let client_pki = temp_dir.path.join("client-pki");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("role mapping audit listener should bind");
    let addr = listener
        .local_addr()
        .expect("role mapping audit listener should have addr");
    let endpoint = format!("opc.tcp://127.0.0.1:{}/", addr.port());
    let user_token_ids = ["password-user"];

    let (server, handle) = ServerBuilder::new()
        .application_name("role-mapping-audit-test")
        .application_uri("urn:role-mapping-audit-test")
        .product_uri("urn:role-mapping-audit-test")
        .host("127.0.0.1")
        .pki_dir(&server_pki)
        .create_sample_keypair(true)
        .discovery_urls(vec![endpoint.clone()])
        .add_user_token(
            "password-user",
            ServerUserToken::user_pass("admin", "admin-pass"),
        )
        .add_endpoint(
            "none",
            ServerEndpoint::new_none("/", &user_token_ids.map(str::to_string)),
        )
        .identity_mapping_rule(
            WellKnownRole::SecurityAdmin.node_id(),
            IdentityMappingRule::UserName("admin".into()),
        )
        .build()
        .expect("role mapping audit test server should build");
    let server_task = tokio::spawn(async move {
        server
            .run_with(listener)
            .await
            .expect("role mapping audit test server should run");
    });

    let mut client = ClientBuilder::new()
        .application_name("role-mapping-audit-client")
        .application_uri("urn:role-mapping-audit-client")
        .product_uri("urn:role-mapping-audit-client")
        .pki_dir(&client_pki)
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_initial(Duration::from_millis(100))
        .client()
        .expect("role mapping audit client should build");

    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                endpoint.as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
            ),
            IdentityToken::new_user_name("admin", "admin-pass"),
        )
        .await
        .expect("role mapping audit client should connect");
    let event_loop_task = event_loop.spawn();

    tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
        .await
        .expect("role mapping audit client should become connected");

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let subscription_id = session
        .create_subscription(
            Duration::from_millis(100),
            30,
            10,
            0,
            0,
            true,
            EventCallback::new(move |event_fields, _| {
                let _ = event_tx.send(event_fields.unwrap_or_default());
            }),
        )
        .await
        .expect("audit subscription should be created");

    let create_results = session
        .create_monitored_items(
            subscription_id,
            TimestampsToReturn::Both,
            vec![event_monitored_item(audit_failure_filter())],
        )
        .await
        .expect("audit monitored item request should complete");
    assert_eq!(create_results[0].result.status_code, StatusCode::Good);

    let rule = IdentityMappingRuleType {
        criteria_type: IdentityCriteriaType::UserName,
        criteria: UAString::from("operator"),
    };
    let object_id = NodeId::from(ObjectId::WellKnownRole_Operator);
    let method_id = NodeId::from(MethodId::WellKnownRole_Operator_AddIdentity);
    let result = session
        .call_one((object_id, method_id, Some(vec![Variant::from(rule)])))
        .await
        .expect("AddIdentity call should succeed");
    assert!(
        result.status_code.is_good(),
        "AddIdentity should succeed; got {:?}",
        result.status_code
    );

    session.trigger_publish_now();

    let expected_event_type = Variant::from(NodeId::from(
        ObjectTypeId::RoleMappingRuleChangedAuditEventType,
    ));
    let mut got_event = false;
    for _ in 0..10 {
        let Ok(Some(received)) =
            tokio::time::timeout(Duration::from_secs(5), event_rx.recv()).await
        else {
            break;
        };
        if received.first() == Some(&expected_event_type) {
            got_event = true;
            break;
        }
        session.trigger_publish_now();
    }

    assert!(
        got_event,
        "RoleMappingRuleChangedAuditEventType should be emitted after AddIdentity"
    );

    handle.cancel();
    event_loop_task.abort();
    server_task.abort();
    drop(server_pki);
    drop(client_pki);
    drop(temp_dir);
}
