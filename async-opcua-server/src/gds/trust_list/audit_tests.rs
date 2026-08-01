use std::time::{Duration, Instant};

use opcua_core::ResponseMessage;
use opcua_types::{
    AttributeId, ContentFilter, CreateSubscriptionRequest, DateTimeUtc, DiagnosticBits,
    EventFilter, EventNotificationList, ExtensionObject, MessageSecurityMode,
    MonitoredItemCreateRequest, MonitoringMode, MonitoringParameters, NodeId, ObjectId,
    ObjectTypeId, PublishRequest, ReadValueId, RequestHeader, SimpleAttributeOperand, StatusCode,
    TimestampsToReturn, TrustListDataType, Variant, VariantScalarTypeId,
};

use crate::{
    authenticator::UserToken,
    identity_token::IdentityToken,
    node_manager::RequestContext,
    subscriptions::{CreateMonitoredItem, PendingPublish},
};

use super::{
    encode_trust_list, masks, open_mode,
    tests::{handler, security_admin_request_context},
    GdsPushRegistry,
};

#[tokio::test]
async fn close_and_update_emits_trust_list_update_requested_audit_event() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    context.session.write().activate(
        1,
        opcua_types::ByteString::null(),
        IdentityToken::None,
        None,
        UserToken("trust-list-audit-test".to_string()),
        None,
        context.user_roles.clone(),
    );
    let subscription_id =
        subscribe_to_event_fields(&context, &[(ObjectTypeId::BaseEventType, "EventType")]).await;
    let registry = std::sync::Arc::new(GdsPushRegistry::default());
    let trust_list_handler = handler(registry);

    let payload = encode_trust_list(&TrustListDataType {
        specified_lists: masks::TRUSTED_CERTIFICATES,
        trusted_certificates: Some(Vec::new()),
        ..Default::default()
    })
    .expect("empty trust list should encode");
    let open_out = trust_list_handler
        .handle_open(
            &context,
            &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
        )
        .expect("open for write should succeed");
    let Variant::UInt32(file_handle) = open_out[0] else {
        panic!("expected UInt32 file handle");
    };
    trust_list_handler
        .handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(opcua_types::ByteString::from(payload)),
            ],
        )
        .expect("write should succeed");

    let publish_response = queue_publish(&context, 1).await;

    trust_list_handler
        .handle_close_and_update(&context, &[Variant::from(file_handle)])
        .expect("close and update should succeed");

    let response = tokio::time::timeout(Duration::from_secs(2), publish_response)
        .await
        .expect("audit event should wake the queued publish request")
        .expect("publish response channel should remain open");
    let event_type = published_event_fields(response, subscription_id)
        .into_iter()
        .next()
        .expect("audit event should include its EventType field");

    assert_eq!(
        event_type,
        Variant::from(NodeId::from(
            ObjectTypeId::TrustListUpdateRequestedAuditEventType
        ))
    );
}

#[tokio::test]
async fn gds_audit_client_user_id_comes_from_activated_session_identity() {
    // Given: an activated username identity distinct from the request authorization key.
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    context.session.write().activate(
        1,
        opcua_types::ByteString::null(),
        IdentityToken::UserName(opcua_types::UserNameIdentityToken {
            policy_id: opcua_types::UAString::from("userpass_none"),
            user_name: opcua_types::UAString::from("activated-session-user"),
            password: opcua_types::ByteString::null(),
            encryption_algorithm: opcua_types::UAString::null(),
        }),
        None,
        UserToken("trust-list-method-test".to_string()),
        None,
        context.user_roles.clone(),
    );
    let subscription_id = subscribe_to_event_fields(
        &context,
        &[
            (ObjectTypeId::BaseEventType, "EventType"),
            (ObjectTypeId::BaseEventType, "ClientUserId"),
        ],
    )
    .await;
    let publish_response = queue_publish(&context, 4).await;

    // When: an existing specialized GDS audit event is emitted through the real event path.
    super::super::audit::trust_list_update_requested(
        &context,
        NodeId::new(0, 12_642),
        NodeId::new(0, 12_666),
        NodeId::new(0, 12_642),
        super::super::audit::AuditAction::CloseAndUpdate,
        &[],
    );

    // Then: ClientUserId identifies the activated session user, not RequestContext.user_token.
    let response = tokio::time::timeout(Duration::from_secs(2), publish_response)
        .await
        .expect("audit event should wake the queued publish request")
        .expect("publish response channel should remain open");
    assert_eq!(
        published_event_fields(response, subscription_id),
        vec![
            Variant::from(NodeId::from(
                ObjectTypeId::TrustListUpdateRequestedAuditEventType,
            )),
            Variant::from(opcua_types::UAString::from("activated-session-user")),
        ]
    );
}

#[tokio::test]
async fn certificate_update_requested_emits_redacted_input_arguments() {
    // Given
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    context.session.write().activate(
        1,
        opcua_types::ByteString::null(),
        IdentityToken::None,
        None,
        UserToken("certificate-input-audit-test".to_string()),
        None,
        context.user_roles.clone(),
    );
    let subscription_id = subscribe_to_event_fields(
        &context,
        &[
            (ObjectTypeId::BaseEventType, "EventType"),
            (ObjectTypeId::BaseEventType, "InputArguments"),
        ],
    )
    .await;
    let input_arguments = vec![
        Variant::from(NodeId::new(2, 2_001u32)),
        Variant::from(NodeId::new(3, 3_002u32)),
        Variant::from(opcua_types::ByteString::from(vec![0x21, 0x22, 0x23])),
        Variant::from(vec![
            opcua_types::ByteString::from(vec![0x31, 0x32]),
            opcua_types::ByteString::from(vec![0x41, 0x42, 0x43]),
        ]),
        Variant::from("PEM"),
        Variant::from(opcua_types::ByteString::from(vec![0x51, 0x52])),
    ];
    let expected_input_arguments = Variant::from((
        VariantScalarTypeId::Variant,
        vec![
            // certificateGroupId
            Variant::Variant(Box::new(input_arguments[0].clone())),
            // certificateTypeId
            Variant::Variant(Box::new(input_arguments[1].clone())),
            // certificate
            Variant::Variant(Box::new(Variant::Empty)),
            // issuerCertificates
            Variant::Variant(Box::new(Variant::Empty)),
            // privateKeyFormat
            Variant::Variant(Box::new(input_arguments[4].clone())),
            // privateKey
            Variant::Variant(Box::new(Variant::Empty)),
        ],
    ));
    let publish_response = queue_publish(&context, 2).await;

    // When
    super::super::audit::certificate_update_requested(
        &context,
        NodeId::new(4, 4_003u32),
        NodeId::new(5, 5_004u32),
        NodeId::new(6, 6_005u32),
        NodeId::new(7, 7_006u32),
        input_arguments.as_slice(),
    );

    // Then
    let response = tokio::time::timeout(Duration::from_secs(2), publish_response)
        .await
        .expect("audit event should wake the queued publish request")
        .expect("publish response channel should remain open");
    assert_eq!(
        published_event_fields(response, subscription_id),
        vec![
            Variant::from(NodeId::from(
                ObjectTypeId::CertificateUpdateRequestedAuditEventType,
            )),
            expected_input_arguments,
        ]
    );
}

#[tokio::test]
async fn certificate_update_requested_exposes_certificate_group_and_type() {
    // Given
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    context.session.write().activate(
        1,
        opcua_types::ByteString::null(),
        IdentityToken::None,
        None,
        UserToken("certificate-metadata-audit-test".to_string()),
        None,
        context.user_roles.clone(),
    );
    let subscription_id = subscribe_to_event_fields(
        &context,
        &[
            (ObjectTypeId::BaseEventType, "EventType"),
            (ObjectTypeId::BaseEventType, "CertificateGroup"),
            (ObjectTypeId::BaseEventType, "CertificateType"),
        ],
    )
    .await;
    let source_node = NodeId::new(4, 4_003u32);
    let method_id = NodeId::new(5, 5_004u32);
    let certificate_group = NodeId::new(7, 7_001u32);
    let certificate_type = NodeId::new(8, 8_002u32);
    let publish_response = queue_publish(&context, 2).await;

    // When
    super::super::audit::certificate_update_requested(
        &context,
        source_node,
        method_id,
        certificate_group.clone(),
        certificate_type.clone(),
        &[],
    );

    // Then
    let response = tokio::time::timeout(Duration::from_secs(2), publish_response)
        .await
        .expect("audit event should wake the queued publish request")
        .expect("publish response channel should remain open");
    assert_eq!(
        published_event_fields(response, subscription_id),
        vec![
            Variant::from(NodeId::from(
                ObjectTypeId::CertificateUpdateRequestedAuditEventType,
            )),
            Variant::from(certificate_group),
            Variant::from(certificate_type),
        ]
    );
}

fn published_event_fields(response: ResponseMessage, subscription_id: u32) -> Vec<Variant> {
    let ResponseMessage::PublishShared(response) = response else {
        panic!("expected Publish response, got {response:?}");
    };
    assert_eq!(response.subscription_id, subscription_id);

    response
        .notification_message
        .notification_data
        .as_ref()
        .expect("publish response should contain notification data")
        .iter()
        .find_map(|notification| {
            notification
                .clone()
                .into_inner_as::<EventNotificationList>()
        })
        .and_then(|events| events.events)
        .and_then(|events| events.into_iter().next())
        .and_then(|event| event.event_fields)
        .expect("audit event should include selected fields")
}

async fn subscribe_to_event_fields(
    context: &RequestContext,
    fields: &[(ObjectTypeId, &str)],
) -> u32 {
    let subscription_id = context
        .subscriptions()
        .create_subscription(
            context.session_id(),
            &CreateSubscriptionRequest {
                requested_publishing_interval: 30_000.0,
                requested_lifetime_count: 30,
                requested_max_keep_alive_count: 20,
                publishing_enabled: true,
                ..Default::default()
            },
            context,
        )
        .await
        .expect("subscription should be created")
        .subscription_id;
    let filter = EventFilter {
        select_clauses: Some(
            fields
                .iter()
                .map(|(event_type, field)| SimpleAttributeOperand::new_value(*event_type, field))
                .collect(),
        ),
        where_clause: ContentFilter::default(),
    };
    let request = MonitoredItemCreateRequest::new(
        ReadValueId::new(NodeId::from(ObjectId::Server), AttributeId::EventNotifier),
        MonitoringMode::Reporting,
        MonitoringParameters {
            client_handle: 1,
            sampling_interval: 0.0,
            filter: ExtensionObject::from_message(filter),
            queue_size: 10,
            discard_oldest: true,
        },
    );
    let mut item = {
        let type_tree = context.info.type_tree.read();
        CreateMonitoredItem::new(
            request,
            context.info.monitored_item_id_handle.next(),
            subscription_id,
            &context.info,
            TimestampsToReturn::Both,
            DiagnosticBits::empty(),
            &*type_tree,
            None,
        )
    };
    item.set_status(StatusCode::Good);
    let results = context
        .subscriptions()
        .create_monitored_items(context.session_id(), subscription_id, vec![item])
        .await
        .expect("event monitored item should be created");
    assert!(results.iter().all(|result| result.status_code.is_good()));
    subscription_id
}

async fn queue_publish(
    context: &RequestContext,
    request_handle: u32,
) -> tokio::sync::oneshot::Receiver<ResponseMessage> {
    let (response, receiver) = tokio::sync::oneshot::channel();
    context
        .subscriptions()
        .enqueue_publish_request(
            context.session_id(),
            DateTimeUtc::from(chrono::Utc::now()),
            Instant::now(),
            PendingPublish {
                response,
                request: Box::new(PublishRequest {
                    request_header: RequestHeader {
                        request_handle,
                        ..Default::default()
                    },
                    subscription_acknowledgements: None,
                }),
                ack_results: None,
                deadline: Instant::now() + Duration::from_secs(30),
            },
        )
        .await
        .expect("publish request should queue");
    receiver
}
