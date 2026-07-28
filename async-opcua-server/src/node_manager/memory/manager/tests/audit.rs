use std::time::{Duration, Instant};

use opcua_core::ResponseMessage;
use opcua_types::{
    AttributeId, ContentFilter, CreateSubscriptionRequest, DateTimeUtc, EventFilter,
    EventNotificationList, ExtensionObject, MonitoredItemCreateRequest, MonitoringMode,
    MonitoringParameters, ObjectId, PublishRequest, ReadValueId, RequestHeader,
    SimpleAttributeOperand, TimestampsToReturn, Variant,
};

use super::*;
use crate::subscriptions::{CreateMonitoredItem, PendingPublish};

#[tokio::test]
async fn add_nodes_audit_event_uses_request_client_audit_entry_id() {
    // Given: an AddNodes request context with a client-provided audit entry ID.
    let mut context = request_context();
    let client_audit_entry_id = UAString::from("request-scoped-add-nodes-audit");
    context.client_audit_entry_id = client_audit_entry_id.clone();
    context.session.write().activate(
        1,
        ByteString::null(),
        IdentityToken::Anonymous(AnonymousIdentityToken {
            policy_id: UAString::from("anonymous"),
        }),
        None,
        UserToken("anonymous".to_string()),
        None,
        context.user_roles.clone(),
    );
    let subscription_id = subscribe_to_event_fields(&context).await;
    let publish_response = queue_publish(&context).await;
    let parent_id = NodeId::new(1, "audit-parent");
    let added_node_id = NodeId::new(1, "audit-child");
    let address_space = AddressSpace::new();
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(object_node(&parent_id, "AuditParent"), None);
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_object_node_item(
        &parent_id,
        &added_node_id,
        QualifiedName::new(1, "AuditChild"),
    );

    // When: the real in-memory AddNodes path succeeds and emits its audit event.
    let mut nodes = vec![&mut item];
    manager
        .add_nodes(&context, nodes.as_mut_slice())
        .await
        .expect("AddNodes should complete");

    // Then: the published AuditAddNodes event carries the request-scoped ID.
    let response = tokio::time::timeout(Duration::from_secs(2), publish_response)
        .await
        .expect("AddNodes audit event should wake the queued publish request")
        .expect("publish response channel should remain open");
    assert_eq!(
        published_add_nodes_event_fields(response, subscription_id),
        vec![
            Variant::from(NodeId::from(ObjectTypeId::AuditAddNodesEventType)),
            Variant::from(client_audit_entry_id),
        ]
    );
}

async fn subscribe_to_event_fields(context: &RequestContext) -> u32 {
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
            ["EventType", "ClientAuditEntryId"]
                .into_iter()
                .map(|field| SimpleAttributeOperand::new_value(ObjectTypeId::BaseEventType, field))
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

async fn queue_publish(
    context: &RequestContext,
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
                        request_handle: 1,
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

fn published_add_nodes_event_fields(
    response: ResponseMessage,
    subscription_id: u32,
) -> Vec<Variant> {
    let ResponseMessage::PublishShared(response) = response else {
        panic!("expected Publish response, got {response:?}");
    };
    assert_eq!(response.subscription_id, subscription_id);
    let add_nodes_event_type = Variant::from(NodeId::from(ObjectTypeId::AuditAddNodesEventType));

    response
        .notification_message
        .notification_data
        .as_ref()
        .expect("publish response should contain notification data")
        .iter()
        .filter_map(|notification| {
            notification
                .clone()
                .into_inner_as::<EventNotificationList>()
        })
        .flat_map(|events| events.events.into_iter().flatten())
        .find_map(|event| {
            let fields = event.event_fields?;
            (fields.first() == Some(&add_nodes_event_type)).then_some(fields)
        })
        .expect("published notifications should include AuditAddNodesEventType")
}
