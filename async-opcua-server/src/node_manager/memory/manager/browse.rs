use super::{super::InMemoryNodeManager, InMemoryNodeManagerImpl};
use crate::{
    address_space::{AddressSpace, NodeType, ReferenceDirection},
    node_manager::{
        view::{AddReferenceResult, ExternalReference, ExternalReferenceRequest, NodeMetadata},
        BrowseNode, BrowsePathItem, DefaultTypeTree, RegisterNodeItem, RequestContext,
        ViewProvider,
    },
    rbac,
};
use async_trait::async_trait;
use opcua_core::{trace_read_lock, trace_write_lock};
use opcua_types::{
    AccessRestrictionType, BrowseDescriptionResultMask, BrowseDirection, ExpandedNodeId, NodeClass,
    NodeId, PermissionType, QualifiedName, ReferenceDescription, ReferenceTypeId, StatusCode,
};
use std::{
    collections::{HashSet, VecDeque},
    ops::Deref,
};
use tracing::warn;

#[cfg(feature = "query")]
use crate::node_manager::QueryRequest;

#[derive(Default)]
struct BrowseContinuationPoint {
    nodes: VecDeque<ReferenceDescription>,
}

impl<TImpl: InMemoryNodeManagerImpl> InMemoryNodeManager<TImpl> {
    fn get_reference(
        address_space: &AddressSpace,
        type_tree: &DefaultTypeTree,
        target_node: &NodeType,
        result_mask: BrowseDescriptionResultMask,
    ) -> NodeMetadata {
        let node_ref = target_node.as_node();

        let target_node_id = node_ref.node_id().clone();

        let type_definition =
            if result_mask.contains(BrowseDescriptionResultMask::RESULT_MASK_TYPE_DEFINITION) {
                // Type definition NodeId of the TargetNode. Type definitions are only available
                // for the NodeClasses Object and Variable. For all other NodeClasses a null NodeId
                // shall be returned.
                match node_ref.node_class() {
                    NodeClass::Object | NodeClass::Variable => {
                        let type_defs = address_space.find_references(
                            &target_node_id,
                            Some((ReferenceTypeId::HasTypeDefinition, false)),
                            type_tree,
                            BrowseDirection::Forward,
                        );
                        if let Some(type_def) = type_defs.first() {
                            ExpandedNodeId::new(type_def.target_id.clone())
                        } else {
                            ExpandedNodeId::null()
                        }
                    }
                    _ => ExpandedNodeId::null(),
                }
            } else {
                ExpandedNodeId::null()
            };

        NodeMetadata {
            node_id: ExpandedNodeId::new(target_node_id),
            browse_name: node_ref.browse_name().clone(),
            display_name: node_ref.display_name().clone(),
            node_class: node_ref.node_class(),
            type_definition,
        }
    }

    fn can_browse_target(context: &RequestContext, target_node: &NodeType) -> bool {
        if !rbac::decision::authorize_ctx(context, target_node, PermissionType::Browse) {
            return false;
        }

        let apply_restrictions_to_browse =
            target_node
                .as_node()
                .access_restrictions()
                .is_some_and(|restrictions| {
                    restrictions.contains(AccessRestrictionType::ApplyRestrictionsToBrowse)
                });
        if apply_restrictions_to_browse {
            rbac::decision::access_restrictions_ok_ctx(context, target_node).is_ok()
        } else {
            true
        }
    }

    /// Browses a single node, returns any external references found.
    fn browse_node(
        address_space: &AddressSpace,
        type_tree: &DefaultTypeTree,
        context: &RequestContext,
        node: &mut BrowseNode,
        namespaces: &hashbrown::HashMap<u16, String>,
    ) {
        let reference_type_id = if node.reference_type_id().is_null() {
            None
        } else if let Ok(reference_type_id) = node.reference_type_id().as_reference_type_id() {
            Some((reference_type_id, node.include_subtypes()))
        } else {
            None
        };

        let mut cont_point = BrowseContinuationPoint::default();

        let source_node_id = node.node_id().clone();

        for reference in address_space.find_references(
            &source_node_id,
            reference_type_id,
            type_tree,
            node.browse_direction(),
        ) {
            if reference.target_id.is_null() {
                warn!(
                    "Target node in reference from {} of type {} is null",
                    node.node_id(),
                    reference.type_id
                );
                continue;
            }
            let target_node = address_space.find_node(&reference.target_id);
            let Some(target_node) = target_node else {
                if namespaces.contains_key(&reference.target_id.namespace) {
                    warn!(
                        "Target node {} in reference from {} of type {} does not exist",
                        reference.target_id,
                        node.node_id(),
                        reference.type_id
                    );
                } else {
                    node.push_external_reference(ExternalReference::new(
                        reference.target_id.into(),
                        reference.type_id.clone(),
                        if reference.is_forward {
                            ReferenceDirection::Forward
                        } else {
                            ReferenceDirection::Inverse
                        },
                    ))
                }

                continue;
            };

            if !Self::can_browse_target(context, &target_node) {
                continue;
            }

            let r_node =
                Self::get_reference(address_space, type_tree, &target_node, node.result_mask());

            let ref_desc = ReferenceDescription {
                reference_type_id: reference.type_id.clone(),
                is_forward: reference.is_forward,
                node_id: r_node.node_id,
                browse_name: r_node.browse_name,
                display_name: r_node.display_name,
                node_class: r_node.node_class,
                type_definition: r_node.type_definition,
            };

            if let AddReferenceResult::Full(c) = node.add(type_tree, ref_desc) {
                cont_point.nodes.push_back(c);
            }
        }

        if !cont_point.nodes.is_empty() {
            node.set_next_continuation_point(Box::new(cont_point));
        }
    }

    fn translate_browse_paths(
        address_space: &AddressSpace,
        type_tree: &DefaultTypeTree,
        context: &RequestContext,
        namespaces: &hashbrown::HashMap<u16, String>,
        item: &mut BrowsePathItem,
    ) {
        if let Some(name) = item.unmatched_browse_name() {
            let is_full_match = address_space
                .find_node(item.node_id())
                .is_some_and(|n| name.is_null() || n.as_node().browse_name() == name);
            if !is_full_match {
                return;
            } else {
                item.set_browse_name_matched(context.current_node_manager_index);
            }
        }

        let mut matching_nodes = HashSet::new();
        matching_nodes.insert(item.node_id().clone());
        let mut next_matching_nodes = HashSet::new();
        let mut results = Vec::new();

        let index = address_space.browse_name_index();

        let mut depth = 0;
        for element in item.path() {
            depth += 1;
            for node_id in matching_nodes.drain() {
                let reference_filter = {
                    if element.reference_type_id.is_null() {
                        None
                    } else {
                        Some((element.reference_type_id.clone(), element.include_subtypes))
                    }
                };

                if element.target_name.is_null() {
                    for rf in address_space.find_references(
                        &node_id,
                        reference_filter,
                        type_tree,
                        if element.is_inverse {
                            BrowseDirection::Inverse
                        } else {
                            BrowseDirection::Forward
                        },
                    ) {
                        if !next_matching_nodes.contains(&rf.target_id) {
                            let target_id = rf.target_id;
                            if address_space.find_node(&target_id).is_none() {
                                if !namespaces.contains_key(&target_id.namespace) {
                                    results.push((target_id, depth, Some(QualifiedName::null())));
                                }
                                continue;
                            };
                            next_matching_nodes.insert(target_id.clone());
                            results.push((target_id, depth, None));
                        }
                    }
                    continue;
                }

                if element.is_inverse || reference_filter.is_some() {
                    for rf in address_space.find_references(
                        &node_id,
                        reference_filter,
                        type_tree,
                        if element.is_inverse {
                            BrowseDirection::Inverse
                        } else {
                            BrowseDirection::Forward
                        },
                    ) {
                        if !next_matching_nodes.contains(&rf.target_id) {
                            let target_id = rf.target_id;
                            let Some(node) = address_space.find_node(&target_id) else {
                                if !namespaces.contains_key(&target_id.namespace) {
                                    results.push((
                                        target_id,
                                        depth,
                                        Some(element.target_name.clone()),
                                    ));
                                }
                                continue;
                            };

                            if node.as_node().browse_name() == &element.target_name {
                                next_matching_nodes.insert(target_id.clone());
                                results.push((target_id, depth, None));
                            }
                        }
                    }
                } else if let Some(candidates) =
                    index.get(&(node_id.clone(), element.target_name.clone()))
                {
                    for target_id in candidates {
                        if !next_matching_nodes.contains(target_id) {
                            next_matching_nodes.insert(target_id.clone());
                            results.push((target_id.clone(), depth, None));
                        }
                    }
                }
            }
            std::mem::swap(&mut matching_nodes, &mut next_matching_nodes);
        }

        for res in results {
            item.add_element(res.0.clone(), res.1, res.2);
        }
    }
}

#[async_trait]
impl<TImpl: InMemoryNodeManagerImpl> ViewProvider for InMemoryNodeManager<TImpl> {
    async fn resolve_external_references(
        &self,
        context: &RequestContext,
        items: &mut [&mut ExternalReferenceRequest],
    ) {
        let address_space = trace_read_lock!(self.address_space);
        let type_tree = trace_read_lock!(context.type_tree);

        for item in items {
            let target_node = address_space.find_node(item.node_id());

            let Some(target_node) = target_node else {
                continue;
            };

            if !Self::can_browse_target(context, &target_node) {
                continue;
            }

            item.set(Self::get_reference(
                &address_space,
                type_tree.deref(),
                &target_node,
                item.result_mask(),
            ));
        }
    }

    async fn browse(
        &self,
        context: &RequestContext,
        nodes_to_browse: &mut [BrowseNode],
    ) -> Result<(), StatusCode> {
        let address_space = trace_read_lock!(self.address_space);
        let type_tree = trace_read_lock!(context.type_tree);

        for node in nodes_to_browse.iter_mut() {
            if node.node_id().is_null() {
                continue;
            }

            node.set_status(StatusCode::Good);

            if let Some(mut point) = node.take_continuation_point::<BrowseContinuationPoint>() {
                loop {
                    if node.remaining() == 0 {
                        break;
                    }
                    let Some(ref_desc) = point.nodes.pop_back() else {
                        break;
                    };
                    // Node is already filtered.
                    node.add_unchecked(ref_desc);
                }
                if !point.nodes.is_empty() {
                    node.set_next_continuation_point(point);
                }
            } else {
                let namespaces = self.namespaces.read();
                Self::browse_node(&address_space, &type_tree, context, node, &namespaces);
            }
        }

        Ok(())
    }

    async fn translate_browse_paths_to_node_ids(
        &self,
        context: &RequestContext,
        nodes: &mut [&mut BrowsePathItem],
    ) -> Result<(), StatusCode> {
        {
            let address_space = trace_read_lock!(self.address_space);
            if !address_space.browse_name_index_is_built() {
                drop(address_space);
                let address_space = trace_write_lock!(self.address_space);
                let type_tree = trace_read_lock!(context.type_tree);
                address_space.ensure_browse_name_index(type_tree.deref());
            }
        }
        let address_space = trace_read_lock!(self.address_space);
        let type_tree = trace_read_lock!(context.type_tree);

        let namespaces = self.namespaces.read();
        for node in nodes {
            Self::translate_browse_paths(
                &address_space,
                type_tree.deref(),
                context,
                &namespaces,
                node,
            );
        }

        Ok(())
    }

    async fn register_nodes(
        &self,
        context: &RequestContext,
        nodes: &mut [&mut RegisterNodeItem],
    ) -> Result<(), StatusCode> {
        self.inner
            .register_nodes(context, &self.address_space, nodes)
            .await
    }

    async fn unregister_nodes(
        &self,
        context: &RequestContext,
        nodes: &[&NodeId],
    ) -> Result<(), StatusCode> {
        self.inner
            .unregister_nodes(context, &self.address_space, nodes)
            .await
    }

    #[cfg(feature = "query")]
    async fn query(
        &self,
        context: &RequestContext,
        request: &mut QueryRequest,
    ) -> Result<(), StatusCode> {
        let address_space = trace_read_lock!(self.address_space);
        let type_tree = context.get_type_tree_for_user();

        if request.continuation_point().is_some() {
            crate::services::query::handlers::QueryNextHandler::new(
                &address_space,
                type_tree.get(),
                context,
            )
            .execute(request)
        } else {
            crate::services::query::handlers::QueryFirstHandler::new(
                &address_space,
                type_tree.get(),
                context,
            )
            .execute(request)
        }
    }
}
