use std::sync::Arc;

use super::super::InMemoryNodeManager;
use super::InMemoryNodeManagerImpl;
use crate::{
    address_space::{AddressSpace, EventNotifier, ReferenceDirection},
    authenticator::UserToken,
    builder::ServerBuilder,
    identity_token::IdentityToken,
    node_manager::{
        AddNodeItem, AddReferenceItem, DeleteNodeItem, NamespaceMetadata, NodeMutator,
        RequestContext, RequestContextInner, ServerContext,
    },
    session::instance::Session,
};
use async_trait::async_trait;
use opcua_core::sync::RwLock;
use opcua_nodes::{Object, ObjectType, ReferenceType, TypeTree, VariableType};
use opcua_types::{
    AddNodeAttributes, AddNodesItem, AddReferencesItem, AnonymousIdentityToken,
    ApplicationDescription, AttributesMask, ByteString, DataTypeId, DeleteNodesItem,
    DiagnosticBits, ExpandedNodeId, LocalizedText, MessageSecurityMode, NodeClass, NodeId,
    ObjectAttributes, ObjectTypeId, QualifiedName, ReferenceTypeId, StatusCode, UAString,
};

struct TestImpl;

#[async_trait]
impl InMemoryNodeManagerImpl for TestImpl {
    async fn init(&self, _address_space: &AddressSpace, _context: ServerContext) {}

    fn name(&self) -> &str {
        "test"
    }

    fn namespaces(&self) -> Vec<NamespaceMetadata> {
        vec![NamespaceMetadata {
            namespace_uri: "urn:test".to_string(),
            namespace_index: 1,
            ..Default::default()
        }]
    }
}

fn request_context() -> RequestContext {
    let mut builder = ServerBuilder::new_anonymous("add nodes duplicate browse name test");
    builder.config_mut().limits.clients_can_modify_address_space = true;
    let (_server, handle) = builder.build().expect("test server should build");
    let info = handle.info().clone();
    let session = Session::create(
        &info,
        NodeId::new(0, 1),
        1,
        60_000,
        0,
        0,
        UAString::from("opc.tcp://localhost"),
        opcua_crypto::SecurityPolicy::None.to_str().to_string(),
        IdentityToken::Anonymous(AnonymousIdentityToken {
            policy_id: UAString::from("anonymous"),
        }),
        None,
        ByteString::null(),
        UAString::from("test"),
        ApplicationDescription::default(),
        MessageSecurityMode::None,
    );

    RequestContext {
        current_node_manager_index: 0,
        inner: Arc::new(RequestContextInner {
            session: Arc::new(RwLock::new(session)),
            session_id: 1,
            authenticator: info.authenticator.clone(),
            token: UserToken("anonymous".to_string()),
            user_roles: Arc::new(Vec::new()),
            type_tree: info.type_tree.clone(),
            type_tree_getter: info.type_tree_getter.clone(),
            subscriptions: handle.subscriptions().clone(),
            info,
        }),
    }
}

fn object_attributes() -> ObjectAttributes {
    ObjectAttributes {
        specified_attributes: 0,
        display_name: LocalizedText::null(),
        description: LocalizedText::null(),
        write_mask: 0,
        user_write_mask: 0,
        event_notifier: 0,
    }
}

fn object_node(node_id: &NodeId, browse_name: &'static str) -> Object {
    Object::new(node_id, browse_name, browse_name, EventNotifier::empty())
}

fn add_object_node_item(
    parent_id: &NodeId,
    new_node_id: &NodeId,
    browse_name: QualifiedName,
) -> AddNodeItem {
    add_object_node_item_with_type_definition(
        parent_id,
        new_node_id,
        browse_name,
        ExpandedNodeId::from(NodeId::from(ObjectTypeId::BaseObjectType)),
    )
}

fn add_object_node_item_with_type_definition(
    parent_id: &NodeId,
    new_node_id: &NodeId,
    browse_name: QualifiedName,
    type_definition: ExpandedNodeId,
) -> AddNodeItem {
    AddNodeItem::new(
        AddNodesItem {
            parent_node_id: ExpandedNodeId::from(parent_id),
            reference_type_id: NodeId::from(ReferenceTypeId::HasComponent),
            requested_new_node_id: ExpandedNodeId::from(new_node_id),
            browse_name,
            node_class: NodeClass::Object,
            node_attributes: AddNodeAttributes::Object(object_attributes()).as_extension_object(),
            type_definition,
        },
        DiagnosticBits::empty(),
    )
}

fn add_object_node_item_with_attributes(
    parent_id: &NodeId,
    new_node_id: &NodeId,
    browse_name: QualifiedName,
    attributes: ObjectAttributes,
) -> AddNodeItem {
    AddNodeItem::new(
        AddNodesItem {
            parent_node_id: ExpandedNodeId::from(parent_id),
            reference_type_id: NodeId::from(ReferenceTypeId::HasComponent),
            requested_new_node_id: ExpandedNodeId::from(new_node_id),
            browse_name,
            node_class: NodeClass::Object,
            node_attributes: AddNodeAttributes::Object(attributes).as_extension_object(),
            type_definition: ExpandedNodeId::from(NodeId::from(ObjectTypeId::BaseObjectType)),
        },
        DiagnosticBits::empty(),
    )
}

fn add_reference_item_with_type(
    source_id: &NodeId,
    target_id: &NodeId,
    reference_type_id: &NodeId,
) -> AddReferenceItem {
    add_reference_item_full(source_id, target_id, reference_type_id, NodeClass::Object)
}

fn add_variable_type_subtype_item(
    parent_id: &NodeId,
    new_node_id: &NodeId,
    browse_name: QualifiedName,
    data_type: NodeId,
    value_rank: i32,
) -> AddNodeItem {
    let attributes = opcua_types::VariableTypeAttributes {
        specified_attributes: (AttributesMask::DATA_TYPE | AttributesMask::VALUE_RANK).bits(),
        data_type,
        value_rank,
        ..Default::default()
    };
    AddNodeItem::new(
        AddNodesItem {
            parent_node_id: ExpandedNodeId::from(parent_id),
            reference_type_id: NodeId::from(ReferenceTypeId::HasSubtype),
            requested_new_node_id: ExpandedNodeId::from(new_node_id),
            browse_name,
            node_class: NodeClass::VariableType,
            node_attributes: AddNodeAttributes::VariableType(attributes).as_extension_object(),
            type_definition: ExpandedNodeId::null(),
        },
        DiagnosticBits::empty(),
    )
}

fn add_reference_item_full(
    source_id: &NodeId,
    target_id: &NodeId,
    reference_type_id: &NodeId,
    target_node_class: NodeClass,
) -> AddReferenceItem {
    AddReferenceItem::new(
        AddReferencesItem {
            source_node_id: source_id.clone(),
            reference_type_id: reference_type_id.clone(),
            is_forward: true,
            target_server_uri: UAString::null(),
            target_node_id: ExpandedNodeId::from(target_id),
            target_node_class,
        },
        DiagnosticBits::empty(),
    )
}

fn deleted_node_item(node_id: &NodeId, delete_target_references: bool) -> DeleteNodeItem {
    let mut item = DeleteNodeItem::new(
        DeleteNodesItem {
            node_id: node_id.clone(),
            delete_target_references,
        },
        DiagnosticBits::empty(),
    );
    item.set_result(StatusCode::Good);
    item
}

mod crud;
mod references;
