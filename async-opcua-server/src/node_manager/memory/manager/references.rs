use super::{
    authorize_node_management_permission, clients_can_modify_address_space, model_change,
    notify_model_changes, MODEL_CHANGE_REFERENCE_ADDED, MODEL_CHANGE_REFERENCE_DELETED,
};
use crate::{
    address_space::{AddressSpace, NodeType},
    node_manager::{audit_events, AddReferenceItem, DeleteReferenceItem, RequestContext},
};
use opcua_core::sync::RwLock;
use opcua_nodes::TypeTree;
use opcua_types::{
    BrowseDirection, NodeClass, NodeId, PermissionType, ReferenceTypeId, StatusCode,
};

#[cfg(feature = "node-management")]
pub(super) fn add_references_impl(
    context: &RequestContext,
    address_space: &RwLock<AddressSpace>,
    references_to_add: &mut [&mut AddReferenceItem],
) {
    // Validate inputs before checking the gate flag. This ensures that
    // per-item errors (BadSourceNodeIdInvalid, BadTargetNodeIdInvalid,
    // BadReferenceTypeIdInvalid) are reported regardless of whether
    // clients_can_modify_address_space is enabled.
    {
        let as_read = address_space.read();
        let type_tree = context.type_tree.read();

        for item in references_to_add.iter_mut() {
            if item.source_status() != StatusCode::BadNotSupported
                && item.target_status() != StatusCode::BadNotSupported
            {
                continue;
            }

            let source_owned = as_read
                .namespaces()
                .contains_key(&item.source_node_id().namespace);
            let target_owned = as_read
                .namespaces()
                .contains_key(&item.target_node_id().node_id.namespace);

            let handle_source = source_owned && item.source_status() == StatusCode::BadNotSupported;
            let handle_target = target_owned && item.target_status() == StatusCode::BadNotSupported;
            if !handle_source && !handle_target {
                continue;
            }

            let source_exists = as_read.node_exists(item.source_node_id());
            let target_exists = as_read.node_exists(&item.target_node_id().node_id);

            if handle_source && !source_exists {
                item.set_source_result(StatusCode::BadSourceNodeIdInvalid);
            }
            if handle_target && !target_exists {
                item.set_target_result(StatusCode::BadTargetNodeIdInvalid);
            }

            if !type_tree
                .get(item.reference_type_id())
                .is_some_and(|node_class| node_class == NodeClass::ReferenceType)
            {
                if handle_source && item.source_status() == StatusCode::BadNotSupported {
                    item.set_source_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                if handle_target && item.target_status() == StatusCode::BadNotSupported {
                    item.set_target_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                continue;
            }

            if source_exists && target_exists {
                let (source_node, target_node) = if item.is_forward() {
                    (item.source_node_id(), &item.target_node_id().node_id)
                } else {
                    (&item.target_node_id().node_id, item.source_node_id())
                };

                if source_node == target_node {
                    if handle_source && item.source_status() == StatusCode::BadNotSupported {
                        item.set_source_result(StatusCode::BadInvalidSelfReference);
                    }
                    if handle_target && item.target_status() == StatusCode::BadNotSupported {
                        item.set_target_result(StatusCode::BadInvalidSelfReference);
                    }
                    continue;
                }

                if as_read.has_reference(source_node, target_node, item.reference_type_id()) {
                    if handle_source && item.source_status() == StatusCode::BadNotSupported {
                        item.set_source_result(StatusCode::BadDuplicateReferenceNotAllowed);
                    }
                    if handle_target && item.target_status() == StatusCode::BadNotSupported {
                        item.set_target_result(StatusCode::BadDuplicateReferenceNotAllowed);
                    }
                    continue;
                }
            }
        }
    }

    if !clients_can_modify_address_space(context) {
        for item in references_to_add {
            if item.source_status() == StatusCode::BadNotSupported {
                item.set_source_result(StatusCode::BadServiceUnsupported);
            }
            if item.target_status() == StatusCode::BadNotSupported {
                item.set_target_result(StatusCode::BadServiceUnsupported);
            }
        }
        return;
    }

    let mut changes = Vec::new();
    let mut audit_items = Vec::new();

    {
        let address_space = address_space.write();
        let type_tree = context.type_tree.read();

        for item in references_to_add.iter_mut() {
            let source_owned = address_space
                .namespaces()
                .contains_key(&item.source_node_id().namespace);
            let target_owned = address_space
                .namespaces()
                .contains_key(&item.target_node_id().node_id.namespace);

            let handle_source = source_owned && item.source_status() == StatusCode::BadNotSupported;
            let handle_target = target_owned && item.target_status() == StatusCode::BadNotSupported;
            if !handle_source && !handle_target {
                continue;
            }

            let source_exists = address_space.node_exists(item.source_node_id());
            let target_exists = address_space.node_exists(&item.target_node_id().node_id);

            if source_owned && item.source_status() == StatusCode::BadSourceNodeIdInvalid {
                if target_owned && item.target_status() == StatusCode::BadNotSupported {
                    item.set_target_result(StatusCode::BadSourceNodeIdInvalid);
                }
                continue;
            }
            if target_owned && item.target_status() == StatusCode::BadTargetNodeIdInvalid {
                if source_owned && item.source_status() == StatusCode::BadNotSupported {
                    item.set_source_result(StatusCode::BadTargetNodeIdInvalid);
                }
                continue;
            }

            if source_exists
                && !authorize_node_management_permission(
                    context,
                    &address_space,
                    item.source_node_id(),
                    PermissionType::AddReference,
                )
            {
                item.set_source_result(StatusCode::BadUserAccessDenied);
                item.set_target_result(StatusCode::BadUserAccessDenied);
                continue;
            }

            if !type_tree
                .get(item.reference_type_id())
                .is_some_and(|node_class| node_class == NodeClass::ReferenceType)
            {
                if handle_source {
                    item.set_source_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                if handle_target {
                    item.set_target_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                continue;
            }
            if reference_type_is_abstract(&address_space, item.reference_type_id()) {
                if handle_source {
                    item.set_source_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                if handle_target {
                    item.set_target_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                continue;
            }

            if handle_source && !source_exists {
                item.set_source_result(StatusCode::BadSourceNodeIdInvalid);
            }
            if handle_target && !target_exists {
                item.set_target_result(StatusCode::BadTargetNodeIdInvalid);
            }

            if (handle_source && !source_exists) || (handle_target && !target_exists) {
                continue;
            }

            let source_ready = handle_source && source_exists;
            let target_ready = handle_target && target_exists;
            if !source_ready && !target_ready {
                continue;
            }

            if item.source_node_id() == &item.target_node_id().node_id {
                if source_ready {
                    item.set_source_result(StatusCode::BadInvalidSelfReference);
                }
                if target_ready {
                    item.set_target_result(StatusCode::BadInvalidSelfReference);
                }
                continue;
            }

            // OPC 10000-4 §5.8.3: the requested targetNodeClass must match the
            // actual target node's NodeClass. `Unspecified` means the client
            // makes no assertion. Only checkable when the target is local.
            if target_exists && item.target_node_class() != NodeClass::Unspecified {
                let actual_class = address_space
                    .find(&item.target_node_id().node_id)
                    .map(|n| n.node_class());
                if actual_class.is_some_and(|c| c != item.target_node_class()) {
                    if source_ready {
                        item.set_source_result(StatusCode::BadNodeClassInvalid);
                    }
                    if target_ready {
                        item.set_target_result(StatusCode::BadNodeClassInvalid);
                    }
                    continue;
                }
            }

            let (source_node, target_node) = if item.is_forward() {
                (item.source_node_id(), &item.target_node_id().node_id)
            } else {
                (&item.target_node_id().node_id, item.source_node_id())
            };

            if !reference_is_structurally_allowed(
                &address_space,
                &*type_tree,
                item.reference_type_id(),
                source_node,
                target_node,
            ) {
                if source_ready {
                    item.set_source_result(StatusCode::BadReferenceNotAllowed);
                }
                if target_ready {
                    item.set_target_result(StatusCode::BadReferenceNotAllowed);
                }
                continue;
            }

            // OPC 10000-3 §5.5.1 / §5.6.2: an Object/Variable is the SourceNode of
            // exactly one HasTypeDefinition Reference. Reject a second one (the
            // duplicate-to-same-target case is handled below).
            if item.reference_type_id() == &NodeId::from(ReferenceTypeId::HasTypeDefinition)
                && !address_space
                    .find_references(
                        source_node,
                        Some((ReferenceTypeId::HasTypeDefinition, false)),
                        &*type_tree,
                        BrowseDirection::Forward,
                    )
                    .is_empty()
            {
                if source_ready {
                    item.set_source_result(StatusCode::BadReferenceNotAllowed);
                }
                if target_ready {
                    item.set_target_result(StatusCode::BadReferenceNotAllowed);
                }
                continue;
            }

            if address_space.has_reference(source_node, target_node, item.reference_type_id()) {
                if source_ready {
                    item.set_source_result(StatusCode::BadDuplicateReferenceNotAllowed);
                }
                if target_ready {
                    item.set_target_result(StatusCode::BadDuplicateReferenceNotAllowed);
                }
                continue;
            }

            address_space.insert_reference(source_node, target_node, item.reference_type_id());

            if source_ready {
                item.set_source_result(StatusCode::Good);
            }
            if target_ready {
                item.set_target_result(StatusCode::Good);
            }
            audit_items.push(audit_events::add_references_item(item));
            changes.push(model_change(
                item.source_node_id().clone(),
                MODEL_CHANGE_REFERENCE_ADDED,
            ));
        }
    }

    audit_events::notify_add_references(context, audit_items);
    notify_model_changes(context, changes);
}

#[cfg(feature = "node-management")]
pub(super) fn delete_references_impl(
    context: &RequestContext,
    address_space: &RwLock<AddressSpace>,
    references_to_delete: &mut [&mut DeleteReferenceItem],
) {
    // Validate inputs before checking the gate flag. This ensures that
    // per-item errors (BadSourceNodeIdInvalid, BadTargetNodeIdInvalid,
    // BadReferenceTypeIdInvalid) are reported regardless of whether
    // clients_can_modify_address_space is enabled.
    {
        let as_read = address_space.read();
        let type_tree = context.type_tree.read();

        for item in references_to_delete.iter_mut() {
            if item.source_status() != StatusCode::BadNotSupported
                && item.target_status() != StatusCode::BadNotSupported
            {
                continue;
            }

            let source_owned = as_read
                .namespaces()
                .contains_key(&item.source_node_id().namespace);
            let target_owned = as_read
                .namespaces()
                .contains_key(&item.target_node_id().node_id.namespace);

            let handle_source = source_owned && item.source_status() == StatusCode::BadNotSupported;
            let handle_target = target_owned && item.target_status() == StatusCode::BadNotSupported;
            if !handle_source && !handle_target {
                continue;
            }

            let source_exists = as_read.node_exists(item.source_node_id());
            let target_exists = as_read.node_exists(&item.target_node_id().node_id);

            if handle_source && !source_exists {
                item.set_source_result(StatusCode::BadSourceNodeIdInvalid);
            }
            if handle_target && !target_exists {
                item.set_target_result(StatusCode::BadTargetNodeIdInvalid);
            }

            if !type_tree
                .get(item.reference_type_id())
                .is_some_and(|node_class| node_class == NodeClass::ReferenceType)
            {
                if handle_source && item.source_status() == StatusCode::BadNotSupported {
                    item.set_source_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                if handle_target && item.target_status() == StatusCode::BadNotSupported {
                    item.set_target_result(StatusCode::BadReferenceTypeIdInvalid);
                }
            }
        }
    }

    if !clients_can_modify_address_space(context) {
        for item in references_to_delete {
            if item.source_status() == StatusCode::BadNotSupported {
                item.set_source_result(StatusCode::BadServiceUnsupported);
            }
            if item.target_status() == StatusCode::BadNotSupported {
                item.set_target_result(StatusCode::BadServiceUnsupported);
            }
        }
        return;
    }

    let mut changes = Vec::new();
    let mut audit_items = Vec::new();

    {
        let address_space = address_space.write();
        let type_tree = context.type_tree.read();

        for item in references_to_delete.iter_mut() {
            let source_owned = address_space
                .namespaces()
                .contains_key(&item.source_node_id().namespace);
            let target_owned = address_space
                .namespaces()
                .contains_key(&item.target_node_id().node_id.namespace);

            let handle_source = source_owned && item.source_status() == StatusCode::BadNotSupported;
            let handle_target = target_owned && item.target_status() == StatusCode::BadNotSupported;
            if !handle_source && !handle_target {
                continue;
            }

            let source_exists = address_space.node_exists(item.source_node_id());
            let target_exists = address_space.node_exists(&item.target_node_id().node_id);

            if source_owned && item.source_status() == StatusCode::BadSourceNodeIdInvalid {
                if target_owned && item.target_status() == StatusCode::BadNotSupported {
                    item.set_target_result(StatusCode::BadSourceNodeIdInvalid);
                }
                continue;
            }
            if target_owned && item.target_status() == StatusCode::BadTargetNodeIdInvalid {
                if source_owned && item.source_status() == StatusCode::BadNotSupported {
                    item.set_source_result(StatusCode::BadTargetNodeIdInvalid);
                }
                continue;
            }

            if source_exists
                && !authorize_node_management_permission(
                    context,
                    &address_space,
                    item.source_node_id(),
                    PermissionType::RemoveReference,
                )
            {
                item.set_source_result(StatusCode::BadUserAccessDenied);
                item.set_target_result(StatusCode::BadUserAccessDenied);
                continue;
            }

            if !type_tree
                .get(item.reference_type_id())
                .is_some_and(|node_class| node_class == NodeClass::ReferenceType)
            {
                if handle_source {
                    item.set_source_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                if handle_target {
                    item.set_target_result(StatusCode::BadReferenceTypeIdInvalid);
                }
                continue;
            }

            if handle_source && !source_exists {
                item.set_source_result(StatusCode::BadSourceNodeIdInvalid);
            }
            if handle_target && !target_exists {
                item.set_target_result(StatusCode::BadTargetNodeIdInvalid);
            }

            if (handle_source && !source_exists) || (handle_target && !target_exists) {
                continue;
            }

            let source_ready = handle_source && source_exists;
            let target_ready = handle_target && target_exists;
            if !source_ready && !target_ready {
                continue;
            }

            let (source_node, target_node) = if item.is_forward() {
                (item.source_node_id(), &item.target_node_id().node_id)
            } else {
                (&item.target_node_id().node_id, item.source_node_id())
            };

            address_space.delete_reference(source_node, target_node, item.reference_type_id());
            if item.delete_bidirectional() {
                address_space.delete_reference(target_node, source_node, item.reference_type_id());
            }

            if source_ready {
                item.set_source_result(StatusCode::Good);
            }
            if target_ready {
                item.set_target_result(StatusCode::Good);
            }
            audit_items.push(audit_events::delete_references_item(item));
            changes.push(model_change(
                item.source_node_id().clone(),
                MODEL_CHANGE_REFERENCE_DELETED,
            ));
        }
    }

    audit_events::notify_delete_references(context, audit_items);
    notify_model_changes(context, changes);
}

/// Resolve a node's NodeClass, preferring a full node in the address space and
/// falling back to type metadata for type-only nodes; `None` if unknown.
#[cfg(feature = "node-management")]
pub(super) fn resolve_node_class(
    address_space: &AddressSpace,
    type_tree: &dyn TypeTree,
    node_id: &NodeId,
) -> Option<NodeClass> {
    if let Some(node) = address_space.find(node_id) {
        return Some(node.node_class());
    }
    type_tree.get(node_id)
}

/// Enforce the NodeClass structural constraints of hierarchical reference types
/// (OPC 10000-4 §5.8.3, OPC 10000-3 §5.3). Conservative: only rejects clearly
/// forbidden combinations; unknown endpoints are permitted so legitimate models
/// (including every combination in the standard nodeset) are never rejected.
#[cfg(feature = "node-management")]
pub(super) fn reference_is_structurally_allowed(
    address_space: &AddressSpace,
    type_tree: &dyn TypeTree,
    reference_type_id: &NodeId,
    source_node_id: &NodeId,
    target_node_id: &NodeId,
) -> bool {
    // HasProperty: the target of a Property reference must be a Variable.
    if type_tree.is_subtype_of(
        reference_type_id,
        &NodeId::from(ReferenceTypeId::HasProperty),
    ) {
        return match address_space.find(target_node_id) {
            Some(target_node) => matches!(&*target_node, NodeType::Variable(_)),
            None => true,
        };
    }

    // HasSubtype: connects a type node to a subtype of the SAME type NodeClass.
    if type_tree.is_subtype_of(
        reference_type_id,
        &NodeId::from(ReferenceTypeId::HasSubtype),
    ) {
        let source_class = resolve_node_class(address_space, type_tree, source_node_id);
        let target_class = resolve_node_class(address_space, type_tree, target_node_id);
        if let (Some(source_class), Some(target_class)) = (source_class, target_class) {
            let is_type_class = |class: NodeClass| {
                matches!(
                    class,
                    NodeClass::ObjectType
                        | NodeClass::VariableType
                        | NodeClass::ReferenceType
                        | NodeClass::DataType
                )
            };
            // A valid HasSubtype is type→type of the same class; anything else is forbidden.
            if !is_type_class(source_class) || source_class != target_class {
                return false;
            }
        }
    }

    true
}
#[cfg(feature = "node-management")]
pub(super) fn reference_type_is_abstract(
    address_space: &AddressSpace,
    reference_type_id: &NodeId,
) -> bool {
    if let Some(reference_type) = address_space.find(reference_type_id) {
        return match &*reference_type {
            NodeType::ReferenceType(reference_type) => reference_type.is_abstract(),
            _ => false,
        };
    }

    standard_reference_type_is_abstract(reference_type_id)
}

#[cfg(feature = "node-management")]
pub(super) fn reference_type_is_known(
    type_tree: &dyn TypeTree,
    reference_type_id: &NodeId,
) -> bool {
    type_tree
        .get(reference_type_id)
        .is_some_and(|node_class| node_class == NodeClass::ReferenceType)
        || reference_type_id.as_reference_type_id().is_ok()
}

#[cfg(feature = "node-management")]
pub(super) fn standard_reference_type_is_abstract(reference_type_id: &NodeId) -> bool {
    reference_type_id
        .as_reference_type_id()
        .is_ok_and(|reference_type_id| {
            matches!(
                reference_type_id,
                ReferenceTypeId::References
                    | ReferenceTypeId::NonHierarchicalReferences
                    | ReferenceTypeId::HierarchicalReferences
                    | ReferenceTypeId::HasChild
                    | ReferenceTypeId::Aggregates
            )
        })
}
