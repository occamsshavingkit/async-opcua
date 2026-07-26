use super::references::reference_type_is_known;
use super::*;

mod node_builder;

use node_builder::*;

pub(super) fn add_nodes_impl(
    context: &RequestContext,
    address_space: &RwLock<AddressSpace>,
    nodes_to_add: &mut [&mut AddNodeItem],
) {
    // Validate inputs before checking the gate flag. This ensures that
    // per-item errors (BadParentNodeIdInvalid, BadNodeIdExists) are reported
    // regardless of whether clients_can_modify_address_space is enabled.
    //
    // Items whose parent is being added earlier in the same batch are skipped
    // for the parent-existence check — the implementation phase processes the
    // batch in order and will resolve the in-batch parent then.
    let batch_new_ids: Vec<NodeId> = nodes_to_add
        .iter()
        .filter(|item| item.status() == StatusCode::BadNotSupported)
        .filter_map(|item| {
            let id = item.requested_new_node_id();
            if id.is_null() {
                None
            } else {
                Some(id.clone())
            }
        })
        .collect();

    {
        let as_read = address_space.read();
        let type_tree = context.type_tree.read();
        for item in nodes_to_add.iter_mut() {
            if item.status() != StatusCode::BadNotSupported {
                continue;
            }

            let parent_id = item.parent_node_id().node_id.clone();
            if (parent_id.is_null() || as_read.find(&parent_id).is_none())
                && !batch_new_ids.contains(&parent_id)
            {
                item.set_result(NodeId::null(), StatusCode::BadParentNodeIdInvalid);
                continue;
            }

            if !item.requested_new_node_id().is_null()
                && as_read.find(item.requested_new_node_id()).is_some()
            {
                item.set_result(NodeId::null(), StatusCode::BadNodeIdExists);
                continue;
            }

            if !reference_type_is_known(&*type_tree, item.reference_type_id()) {
                item.set_result(NodeId::null(), StatusCode::BadReferenceTypeIdInvalid);
                continue;
            }

            if let Err(status) = validate_type_definition(&as_read, &*type_tree, item) {
                item.set_result(NodeId::null(), status);
                continue;
            }
        }
    }

    if !clients_can_modify_address_space(context) {
        for item in nodes_to_add {
            if item.status() == StatusCode::BadNotSupported {
                item.set_result(NodeId::null(), StatusCode::BadServiceUnsupported);
            }
        }
        return;
    }

    let mut changes = Vec::new();
    let mut audit_items = Vec::new();

    {
        let address_space = address_space.write();

        for item in nodes_to_add.iter_mut() {
            if item.status().is_bad() && item.status() != StatusCode::BadNotSupported {
                continue;
            }

            let parent_id = item.parent_node_id().node_id.clone();
            if parent_id.is_null() || !address_space.node_exists(&parent_id) {
                item.set_result(NodeId::null(), StatusCode::BadParentNodeIdInvalid);
                continue;
            }

            if !authorize_node_management_permission(
                context,
                &address_space,
                &parent_id,
                PermissionType::AddNode,
            ) {
                item.set_result(NodeId::null(), StatusCode::BadUserAccessDenied);
                continue;
            }

            if item.reference_type_id().is_null() {
                item.set_result(NodeId::null(), StatusCode::BadReferenceTypeIdInvalid);
                continue;
            }

            let type_tree = context.type_tree.read();
            if address_space
                .find_node_by_browse_name(
                    &parent_id,
                    Some((item.reference_type_id().clone(), false)),
                    &*type_tree,
                    BrowseDirection::Forward,
                    item.browse_name().clone(),
                )
                .is_some()
            {
                item.set_result(NodeId::null(), StatusCode::BadBrowseNameDuplicated);
                continue;
            }
            drop(type_tree);

            let type_tree = context.type_tree.read();
            if let Err(status) = validate_type_definition(&address_space, &*type_tree, item) {
                item.set_result(NodeId::null(), status);
                continue;
            }
            drop(type_tree);

            let assigned_id = if item.requested_new_node_id().is_null() {
                next_unused_node_id(&address_space, parent_id.namespace)
            } else if !address_space
                .namespaces()
                .contains_key(&item.requested_new_node_id().namespace)
            {
                item.set_result(NodeId::null(), StatusCode::BadNodeIdRejected);
                continue;
            } else if address_space.node_exists(item.requested_new_node_id()) {
                item.set_result(NodeId::null(), StatusCode::BadNodeIdExists);
                continue;
            } else {
                item.requested_new_node_id().clone()
            };

            let node = match build_node(item, &assigned_id) {
                Ok(node) => node,
                Err(status) => {
                    item.set_result(NodeId::null(), status);
                    continue;
                }
            };

            {
                let type_tree = context.type_tree.read();
                if let Err(status) =
                    validate_type_refinement(&address_space, &*type_tree, item, &node)
                {
                    item.set_result(NodeId::null(), status);
                    continue;
                }
            }

            let type_definition_id = item.type_definition_id().node_id.clone();
            let has_type_definition_id = NodeId::from(ReferenceTypeId::HasTypeDefinition);
            let mut references = vec![(
                &parent_id,
                item.reference_type_id(),
                ReferenceDirection::Inverse,
            )];
            if !type_definition_id.is_null() {
                references.push((
                    &type_definition_id,
                    &has_type_definition_id,
                    ReferenceDirection::Forward,
                ));
            }

            if address_space.insert(node, Some(references.as_slice())) {
                item.set_result(assigned_id.clone(), StatusCode::Good);
                audit_items.push(audit_events::add_nodes_item(item));
                changes.push(model_change(assigned_id, MODEL_CHANGE_NODE_ADDED));
            } else {
                item.set_result(NodeId::null(), StatusCode::BadNodeIdExists);
            }
        }
    }

    audit_events::notify_add_nodes(context, audit_items);
    notify_model_changes(context, changes);
}

#[cfg(feature = "node-management")]
pub(super) fn delete_nodes_impl(
    context: &RequestContext,
    address_space: &RwLock<AddressSpace>,
    nodes_to_delete: &mut [&mut DeleteNodeItem],
) {
    // Validate node existence before checking the gate flag. Items arrive
    // with BadNodeIdUnknown (the routing "pending" status). If the node
    // exists, transition to BadNotSupported so the gate can distinguish
    // "node exists, service unsupported" from "node doesn't exist".
    {
        let as_read = address_space.read();
        for item in nodes_to_delete.iter_mut() {
            if item.status() != StatusCode::BadNodeIdUnknown {
                continue;
            }

            if as_read.node_exists(item.node_id()) {
                if !authorize_node_management_permission(
                    context,
                    &as_read,
                    item.node_id(),
                    PermissionType::DeleteNode,
                ) {
                    item.set_result(StatusCode::BadUserAccessDenied);
                    continue;
                }
                item.set_result(StatusCode::BadNotSupported);
            }
        }
    }

    if !clients_can_modify_address_space(context) {
        for item in nodes_to_delete {
            if item.status() == StatusCode::BadNotSupported {
                item.set_result(StatusCode::BadServiceUnsupported);
            }
        }
        return;
    }

    let mut changes = Vec::new();
    let mut audit_items = Vec::new();

    {
        let address_space = address_space.write();

        for item in nodes_to_delete.iter_mut() {
            if item.status().is_bad() && item.status() != StatusCode::BadNotSupported {
                continue;
            }

            if item.node_id().is_null() {
                item.set_result(StatusCode::BadNodeIdInvalid);
                continue;
            }

            if !authorize_node_management_permission(
                context,
                &address_space,
                item.node_id(),
                PermissionType::DeleteNode,
            ) {
                item.set_result(StatusCode::BadUserAccessDenied);
                continue;
            }

            let deleted_node_id = item.node_id().clone();
            if address_space
                .delete(item.node_id(), item.delete_target_references())
                .is_some()
            {
                item.set_result(StatusCode::Good);
                audit_items.push(audit_events::delete_nodes_item(item));
                changes.push(model_change(deleted_node_id, MODEL_CHANGE_NODE_DELETED));
            } else {
                item.set_result(StatusCode::BadNodeIdUnknown);
            }
        }
    }

    audit_events::notify_delete_nodes(context, audit_items);
    notify_model_changes(context, changes);
}
pub(super) fn validate_type_refinement(
    address_space: &AddressSpace,
    type_tree: &dyn TypeTree,
    item: &AddNodeItem,
    node: &NodeType,
) -> Result<(), StatusCode> {
    let NodeType::VariableType(child) = node else {
        return Ok(());
    };
    if !type_tree.is_subtype_of(
        item.reference_type_id(),
        &NodeId::from(ReferenceTypeId::HasSubtype),
    ) {
        return Ok(());
    }
    let Some(parent) = address_space.find(&item.parent_node_id().node_id) else {
        return Ok(());
    };
    let NodeType::VariableType(parent) = &*parent else {
        return Ok(());
    };

    // DataType: the subtype's DataType must be a subtype of the supertype's.
    // Only judge a DataType the type tree knows; an unknown one can't be proven
    // to widen, so it is allowed (conservative; equal DataTypes always pass).
    if type_tree.get(child.data_type()).is_some()
        && !type_tree.is_subtype_of(child.data_type(), parent.data_type())
    {
        return Err(StatusCode::BadNodeAttributesInvalid);
    }
    // ValueRank: the subtype must further-restrict (not widen) the supertype.
    if !value_rank_is_restriction_of(parent.value_rank(), child.value_rank()) {
        return Err(StatusCode::BadNodeAttributesInvalid);
    }
    Ok(())
}

/// Whether `child` ValueRank is a valid restriction of `parent` ValueRank
/// (OPC 10000-3): Any (-2) accepts anything; ScalarOrOneDimension (-3) accepts
/// scalar or a single dimension; OneOrMoreDimensions (0) accepts any array;
/// Scalar (-1) and fixed ranks (>=1) require an exact match.
#[cfg(feature = "node-management")]
pub(super) fn value_rank_is_restriction_of(parent: i32, child: i32) -> bool {
    const ANY: i32 = -2;
    const SCALAR_OR_ONE_DIMENSION: i32 = -3;
    const ONE_OR_MORE_DIMENSIONS: i32 = 0;
    const SCALAR: i32 = -1;
    match parent {
        ANY => true,
        SCALAR_OR_ONE_DIMENSION => matches!(child, SCALAR_OR_ONE_DIMENSION | SCALAR | 1),
        ONE_OR_MORE_DIMENSIONS => child == ONE_OR_MORE_DIMENSIONS || child >= 1,
        SCALAR => child == SCALAR,
        n => child == n,
    }
}
#[cfg(feature = "node-management")]
pub(super) fn validate_type_definition(
    address_space: &AddressSpace,
    type_tree: &dyn TypeTree,
    item: &AddNodeItem,
) -> Result<(), StatusCode> {
    let type_definition_id = &item.type_definition_id().node_id;
    if type_definition_id.is_null() {
        return Ok(());
    }

    let expected_type_class = match item.node_class() {
        NodeClass::Object => NodeClass::ObjectType,
        NodeClass::Variable => NodeClass::VariableType,
        _ => return Ok(()),
    };

    if let Some(type_definition) = address_space.find(type_definition_id) {
        return match (expected_type_class, &*type_definition) {
            (NodeClass::ObjectType, NodeType::ObjectType(object_type))
                if object_type.is_abstract() =>
            {
                Err(StatusCode::BadTypeDefinitionInvalid)
            }
            (NodeClass::VariableType, NodeType::VariableType(variable_type))
                if variable_type.is_abstract() =>
            {
                Err(StatusCode::BadTypeDefinitionInvalid)
            }
            (NodeClass::ObjectType, NodeType::ObjectType(_))
            | (NodeClass::VariableType, NodeType::VariableType(_)) => Ok(()),
            _ => Err(StatusCode::BadTypeDefinitionInvalid),
        };
    }

    // Type definition present only in the type metadata (no full node). Require
    // the correct type NodeClass AND reject abstract types — OPC 10000-3 §5.5.2
    // (ObjectType) / §5.6.5 (VariableType): abstract types cannot be instantiated.
    match type_tree.get(type_definition_id) {
        Some(node_class) if node_class == expected_type_class => {
            if type_tree.is_abstract(type_definition_id) == Some(true) {
                Err(StatusCode::BadTypeDefinitionInvalid)
            } else {
                Ok(())
            }
        }
        _ => Err(StatusCode::BadTypeDefinitionInvalid),
    }
}

#[cfg(feature = "node-management")]
pub(super) fn next_unused_node_id(address_space: &AddressSpace, namespace: u16) -> NodeId {
    loop {
        let node_id = NodeId::next_numeric(namespace);
        if !address_space.node_exists(&node_id) {
            return node_id;
        }
    }
}
