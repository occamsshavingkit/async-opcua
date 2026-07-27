use std::time::{Duration, Instant};

use opcua_core::ResponseMessage;
use opcua_types::{
    AttributeId, ContentFilter, CreateSubscriptionRequest, DateTimeUtc, DiagnosticBits,
    EventFilter, EventNotificationList, ExtensionObject, MessageSecurityMode,
    MonitoredItemCreateRequest, MonitoringMode, MonitoringParameters, NodeId, ObjectId,
    ObjectTypeId, PublishRequest, ReadValueId, RequestHeader, SimpleAttributeOperand, StatusCode,
    TimestampsToReturn, TrustListDataType, Variant,
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
    let subscription_id = subscribe_to_event_type(&context).await;
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

    let (pending, publish_response) = pending_publish(1);
    context
        .subscriptions()
        .enqueue_publish_request(
            context.session_id(),
            DateTimeUtc::from(chrono::Utc::now()),
            Instant::now(),
            pending,
        )
        .await
        .expect("publish request should queue");

    trust_list_handler
        .handle_close_and_update(&context, &[Variant::from(file_handle)])
        .expect("close and update should succeed");

    let response = tokio::time::timeout(Duration::from_secs(2), publish_response)
        .await
        .expect("audit event should wake the queued publish request")
        .expect("publish response channel should remain open");
    let ResponseMessage::PublishShared(response) = response else {
        panic!("expected Publish response, got {response:?}");
    };
    assert_eq!(response.subscription_id, subscription_id);

    let event_type = response
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
        .and_then(|fields| fields.into_iter().next())
        .expect("audit event should include its EventType field");

    assert_eq!(
        event_type,
        Variant::from(NodeId::from(
            ObjectTypeId::TrustListUpdateRequestedAuditEventType
        ))
    );
}

async fn subscribe_to_event_type(context: &RequestContext) -> u32 {
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
        select_clauses: Some(vec![SimpleAttributeOperand::new_value(
            ObjectTypeId::BaseEventType,
            "EventType",
        )]),
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
    let type_tree = context.info.type_tree.read();
    let mut item = CreateMonitoredItem::new(
        request,
        context.info.monitored_item_id_handle.next(),
        subscription_id,
        &context.info,
        TimestampsToReturn::Both,
        DiagnosticBits::empty(),
        &*type_tree,
        None,
    );
    item.set_status(StatusCode::Good);
    drop(type_tree);
    let results = context
        .subscriptions()
        .create_monitored_items(context.session_id(), subscription_id, vec![item])
        .await
        .expect("event monitored item should be created");
    assert!(results.iter().all(|result| result.status_code.is_good()));
    subscription_id
}

fn pending_publish(
    request_handle: u32,
) -> (
    PendingPublish,
    tokio::sync::oneshot::Receiver<ResponseMessage>,
) {
    let (response, receiver) = tokio::sync::oneshot::channel();
    (
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
        receiver,
    )
}
