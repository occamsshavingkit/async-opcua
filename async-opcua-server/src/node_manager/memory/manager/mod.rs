use async_trait::async_trait;
#[cfg(feature = "method-call")]
use std::sync::Arc;

#[cfg(all(feature = "node-management", feature = "events"))]
use crate::node_manager::GeneralModelChangeEvent;
#[cfg(feature = "method-call")]
use crate::node_manager::MethodCall;
use crate::{
    address_space::AddressSpace,
    node_manager::{
        NamespaceMetadata, ParsedReadValueId, RegisterNodeItem, RequestContext, ServerContext,
        WriteNode,
    },
};
#[cfg(feature = "node-management")]
use crate::{
    node_manager::{AddNodeItem, AddReferenceItem, DeleteNodeItem, DeleteReferenceItem},
    rbac,
};
#[cfg(feature = "history")]
use crate::{
    node_manager::{HistoryNode, HistoryUpdateNode},
    session::continuation_points::ContinuationPoint,
};
#[cfg(feature = "subscriptions")]
use crate::{
    node_manager::{MonitoredItemRef, MonitoredItemUpdateRef},
    subscriptions::CreateMonitoredItem,
};
use opcua_core::sync::RwLock;
#[cfg(all(feature = "node-management", feature = "events"))]
use opcua_nodes::Event;
#[cfg(feature = "subscriptions")]
use opcua_types::MonitoringMode;
#[cfg(all(feature = "node-management", feature = "events"))]
use opcua_types::ObjectId;
#[cfg(any(feature = "method-call", feature = "node-management"))]
use opcua_types::Variant;
use opcua_types::{DataValue, NodeId, StatusCode, TimestampsToReturn};
#[cfg(feature = "node-management")]
use opcua_types::{ExpandedNodeId, ModelChangeStructureDataType, PermissionType};
#[cfg(feature = "history")]
use opcua_types::{
    ReadAnnotationDataDetails, ReadAtTimeDetails, ReadEventDetails, ReadProcessedDetails,
    ReadRawModifiedDetails,
};

mod browse;
#[cfg(feature = "node-management")]
mod crud;
#[cfg(feature = "node-management")]
mod references;

#[cfg(all(test, feature = "node-management"))]
mod tests;

#[cfg(feature = "node-management")]
use crud::{add_nodes_impl, delete_nodes_impl};
#[cfg(feature = "node-management")]
use references::{add_references_impl, delete_references_impl};

#[cfg(feature = "node-management")]
const MODEL_CHANGE_NODE_ADDED: u8 = 1;
#[cfg(feature = "node-management")]
const MODEL_CHANGE_NODE_DELETED: u8 = 2;
#[cfg(feature = "node-management")]
const MODEL_CHANGE_REFERENCE_ADDED: u8 = 4;
#[cfg(feature = "node-management")]
const MODEL_CHANGE_REFERENCE_DELETED: u8 = 8;

/// Callback used by the default in-memory method `Call` implementation.
#[cfg(feature = "method-call")]
pub type InMemoryMethodCallback = Arc<
    dyn Fn(&RequestContext, &[Variant]) -> Result<Vec<Variant>, StatusCode> + Send + Sync + 'static,
>;

/// Trait for constructing an [InMemoryNodeManagerImpl].
///
/// Note that this is called with the lock on the [AddressSpace] held,
/// if you try to lock it again, it will deadlock.
pub trait InMemoryNodeManagerImplBuilder {
    /// Type implementing [InMemoryNodeManagerImpl] constructed by this builder.
    type Impl: InMemoryNodeManagerImpl;

    /// Build the node manager impl.
    fn build(self, context: ServerContext, address_space: &AddressSpace) -> Self::Impl;
}

impl<T, R: InMemoryNodeManagerImpl> InMemoryNodeManagerImplBuilder for T
where
    T: FnOnce(ServerContext, &AddressSpace) -> R,
{
    type Impl = R;

    fn build(self, context: ServerContext, address_space: &AddressSpace) -> Self::Impl {
        self(context, address_space)
    }
}

#[cfg(feature = "node-management")]
fn clients_can_modify_address_space(context: &RequestContext) -> bool {
    context.info.config.limits.clients_can_modify_address_space
}

#[cfg(feature = "node-management")]
fn authorize_node_management_permission(
    context: &RequestContext,
    address_space: &AddressSpace,
    node_id: &NodeId,
    required: PermissionType,
) -> bool {
    let Some(node) = address_space.find(node_id) else {
        return true;
    };

    rbac::decision::authorize_ctx(context, &node, required)
}

#[cfg(feature = "node-management")]
fn model_change(affected: NodeId, verb: u8) -> ModelChangeStructureDataType {
    ModelChangeStructureDataType {
        affected,
        affected_type: NodeId::null(),
        verb,
    }
}

#[cfg(all(feature = "node-management", feature = "events"))]
fn notify_model_changes(context: &RequestContext, changes: Vec<ModelChangeStructureDataType>) {
    if changes.is_empty() {
        return;
    }

    let event = GeneralModelChangeEvent::new(changes);
    let server_node_id = NodeId::from(ObjectId::Server);
    let items = std::iter::once((&event as &dyn Event, &server_node_id));
    context.subscriptions.notify_events(items);
}

#[cfg(all(feature = "node-management", not(feature = "events")))]
fn notify_model_changes(_context: &RequestContext, _changes: Vec<ModelChangeStructureDataType>) {}

#[async_trait]
#[allow(unused)]
/// Trait for user-provided implementation of the [InMemoryNodeManager](crate::node_manager::memory::InMemoryNodeManager)
pub trait InMemoryNodeManagerImpl: Send + Sync + 'static {
    /// Populate the address space.
    async fn init(&self, address_space: &AddressSpace, context: ServerContext);

    /// Name of this node manager, for debug purposes.
    fn name(&self) -> &str;

    /// Return the static list of namespaces this node manager uses.
    fn namespaces(&self) -> Vec<NamespaceMetadata>;

    /// Return whether this node should handle requests to create a node
    /// for the given parent ID. This is only called if no new node ID is
    /// requested, otherwise owns_node is called on the requested node ID.
    fn owns_server_events(&self) -> bool {
        false
    }

    /// Return `true` when this implementation can handle `method_id` even if the call object does
    /// not expose that exact method node as a component (e.g. a cross-node-manager shared method that
    /// validates its own object). Default false.
    #[cfg(feature = "method-call")]
    fn accepts_method_without_object_component(&self, _method_id: &NodeId) -> bool {
        false
    }

    /// Return `true` if a node with no requested node ID and parent `parent_id`
    /// should be created using this node manager.
    ///
    /// This does not commit to actually allowing the node to be created, it just means
    /// that no other node managers will be called to create the node.
    #[cfg(feature = "node-management")]
    fn handle_new_node(&self, parent_id: &ExpandedNodeId) -> bool {
        false
    }

    /// Perform the register nodes service. The default behavior for this service is to
    /// do nothing and pretend the nodes were registered.
    async fn register_nodes(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes: &mut [&mut RegisterNodeItem],
    ) -> Result<(), StatusCode> {
        for node in nodes {
            node.set_registered(true);
        }

        Ok(())
    }

    /// Read for variable values. Other attributes are handled by the parent
    /// node ID. This should return a list of data values with the same length
    /// and order as `nodes`.
    async fn read_values(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes: &[&ParsedReadValueId],
        max_age: f64,
        timestamps_to_return: TimestampsToReturn,
    ) -> Vec<DataValue> {
        let address_space = address_space.read();
        nodes
            .iter()
            .map(|n| address_space.read(context, n, max_age, timestamps_to_return))
            .collect()
    }

    /// Create monitored items for the Value attribute, as needed.
    /// This should, at the very least, read the current value of the nodes,
    /// and set appropriate status on the monitored item request, see
    /// default implementation.
    ///
    /// It may also begin sampling as given by the monitored item request.
    #[cfg(feature = "subscriptions")]
    async fn create_value_monitored_items(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        items: &mut [&mut &mut CreateMonitoredItem],
    ) {
        let to_read: Vec<_> = items.iter().map(|r| r.item_to_monitor()).collect();
        let values = self
            .read_values(
                context,
                address_space,
                &to_read,
                0.0,
                TimestampsToReturn::Both,
            )
            .await;

        for (value, node) in values.into_iter().zip(items.iter_mut()) {
            if value.status() != StatusCode::BadAttributeIdInvalid {
                node.set_initial_value(value);
            }
            node.set_status(StatusCode::Good);
        }
    }

    /// Create monitored items for events.
    ///
    /// This does not need to do anything.
    #[cfg(feature = "events")]
    async fn create_event_monitored_items(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        items: &mut [&mut &mut CreateMonitoredItem],
    ) {
        // This is just a no-op by default.
    }

    /// Handle the SetMonitoringMode request, to pause or resume sampling.
    ///
    /// This will only get monitored items for events or value.
    #[cfg(feature = "subscriptions")]
    async fn set_monitoring_mode(
        &self,
        context: &RequestContext,
        mode: MonitoringMode,
        items: &[&MonitoredItemRef],
    ) {
    }

    /// Handle modification of monitored items, this may adjust
    /// sampling intervals or filters, and require action to update background
    /// processes.
    #[cfg(feature = "subscriptions")]
    async fn modify_monitored_items(
        &self,
        context: &RequestContext,
        items: &[&MonitoredItemUpdateRef],
    ) {
    }

    /// Handle deletion of monitored items.
    #[cfg(feature = "subscriptions")]
    async fn delete_monitored_items(&self, context: &RequestContext, items: &[&MonitoredItemRef]) {}

    /// Perform the unregister nodes service. The default behavior for this service is to
    /// do nothing.
    async fn unregister_nodes(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes: &[&NodeId],
    ) -> Result<(), StatusCode> {
        // Again, just do nothing
        Ok(())
    }

    /// Perform the history read raw modified service. This should write results
    /// to the `nodes` list of type either `HistoryData` or `HistoryModifiedData`
    ///
    /// Nodes are verified to be readable before this is called.
    #[cfg(feature = "history")]
    async fn history_read_raw_modified(
        &self,
        context: &RequestContext,
        details: &ReadRawModifiedDetails,
        nodes: &mut [&mut &mut HistoryNode],
        timestamps_to_return: TimestampsToReturn,
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Perform the history read processed service. This should write results
    /// to the `nodes` list of type `HistoryData`.
    ///
    /// Nodes are verified to be readable before this is called.
    #[cfg(feature = "history")]
    async fn history_read_processed(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        details: &ReadProcessedDetails,
        nodes: &mut [&mut &mut HistoryNode],
        timestamps_to_return: TimestampsToReturn,
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Perform the history read processed service. This should write results
    /// to the `nodes` list of type `HistoryData`.
    ///
    /// Nodes are verified to be readable before this is called.
    #[cfg(feature = "history")]
    async fn history_read_at_time(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        details: &ReadAtTimeDetails,
        nodes: &mut [&mut &mut HistoryNode],
        timestamps_to_return: TimestampsToReturn,
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Perform the history read events service. This should write results
    /// to the `nodes` list of type `HistoryEvent`.
    ///
    /// Nodes are verified to be readable before this is called.
    #[cfg(feature = "history")]
    async fn history_read_events(
        &self,
        context: &RequestContext,
        details: &ReadEventDetails,
        nodes: &mut [&mut &mut HistoryNode],
        timestamps_to_return: TimestampsToReturn,
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Perform the history read annotations data service. This should write
    /// results to the `nodes` list of type `Annotation`.
    ///
    /// Nodes are verified to be readable before this is called.
    #[cfg(feature = "history")]
    async fn history_read_annotations(
        &self,
        context: &RequestContext,
        details: &ReadAnnotationDataDetails,
        nodes: &mut [&mut &mut HistoryNode],
        timestamps_to_return: TimestampsToReturn,
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Release a history continuation point after the service removes it from the session cache.
    #[cfg(feature = "history")]
    async fn history_release_continuation_point(
        &self,
        context: &RequestContext,
        node_id: &NodeId,
        continuation_point: &ContinuationPoint,
    ) -> Result<(), StatusCode> {
        Ok(())
    }

    /// Perform the HistoryUpdate service. This should write result
    /// status codes to the `nodes` list as appropriate.
    ///
    /// Nodes are verified to be writable before this is called.
    #[cfg(feature = "history")]
    async fn history_update(
        &self,
        context: &RequestContext,
        nodes: &mut [&mut &mut HistoryUpdateNode],
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadHistoryOperationUnsupported)
    }

    /// Perform the write service. This should write results
    /// to the `nodes_to_write` list. The default result is `BadNodeIdUnknown`
    ///
    /// Writing is left almost entirely up to the node manager impl. If you do write
    /// values you should call `context.subscriptions.notify_data_change` to trigger
    /// any monitored items subscribed to the updated values.
    async fn write(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes_to_write: &mut [&mut WriteNode],
    ) -> Result<(), StatusCode> {
        Err(StatusCode::BadServiceUnsupported)
    }

    /// Call a list of methods.
    ///
    /// The methods have already had their arguments verified to have valid length
    /// and the method is verified to exist on the given object. This should try
    /// to execute the methods, and set the result.
    #[cfg(feature = "method-call")]
    async fn call(
        &self,
        context: &RequestContext,
        _address_space: &RwLock<AddressSpace>,
        methods_to_call: &mut [&mut &mut MethodCall],
    ) -> Result<(), StatusCode> {
        for method in methods_to_call {
            let Some(callback) = self.method_callback(method.method_id()) else {
                method.set_status(StatusCode::BadNotImplemented);
                continue;
            };

            match callback(context, method.arguments()) {
                Ok(outputs) => {
                    method.set_outputs(outputs);
                    method.set_status(StatusCode::Good);
                }
                Err(status) => method.set_status(status),
            }
        }

        Ok(())
    }

    /// Return a callback for executing a method, if this implementation has one registered.
    #[cfg(feature = "method-call")]
    fn method_callback(&self, method_id: &NodeId) -> Option<InMemoryMethodCallback> {
        None
    }

    /// Add a list of nodes.
    ///
    /// This should create the nodes, or set a failed status as appropriate.
    /// If a node was created, the status should be set to Good.
    #[cfg(feature = "node-management")]
    async fn add_nodes(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes_to_add: &mut [&mut AddNodeItem],
    ) -> Result<(), StatusCode> {
        add_nodes_impl(context, address_space, nodes_to_add);
        Ok(())
    }

    /// Add a list of references.
    ///
    /// This will be given all references where the source _or_
    /// target belongs to this node manager. A reference is
    /// considered successfully added if either source_status
    /// or target_status are Good.
    ///
    /// If you want to explicitly set the reference to failed,
    /// set both source and target status. Note that it may
    /// already have been added in a different node manager, you are
    /// responsible for any cleanup if you do this.
    #[cfg(feature = "node-management")]
    async fn add_references(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        references_to_add: &mut [&mut AddReferenceItem],
    ) -> Result<(), StatusCode> {
        add_references_impl(context, address_space, references_to_add);
        Ok(())
    }

    /// Delete a list of nodes.
    ///
    /// This will be given all nodes that belong to this node manager.
    ///
    /// Typically, you also want to implement `delete_node_references` if
    /// there are other node managers that support deletes.
    #[cfg(feature = "node-management")]
    async fn delete_nodes(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes_to_delete: &mut [&mut DeleteNodeItem],
    ) -> Result<(), StatusCode> {
        delete_nodes_impl(context, address_space, nodes_to_delete);
        Ok(())
    }

    /// Delete references for the given list of nodes.
    /// The node manager should respect `delete_target_references`.
    ///
    /// This is not allowed to fail, you should make it impossible to delete
    /// nodes with immutable references.
    #[cfg(feature = "node-management")]
    async fn delete_node_references(
        &self,
        _context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        to_delete: &[&DeleteNodeItem],
    ) {
        let address_space = address_space.write();
        for item in to_delete {
            if item.status().is_good() {
                address_space
                    .delete_node_references(item.node_id(), item.delete_target_references());
            }
        }
    }

    /// Delete a list of references.
    ///
    /// This will be given all references where the source _or_
    /// target belongs to this node manager. A reference is
    /// considered successfully added if either source_status
    /// or target_status are Good.
    ///
    /// If you want to explicitly set the reference to failed,
    /// set both source and target status. Note that it may
    /// already have been deleted in a different node manager, you are
    /// responsible for any cleanup if you do this.
    #[cfg(feature = "node-management")]
    async fn delete_references(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        references_to_delete: &mut [&mut DeleteReferenceItem],
    ) -> Result<(), StatusCode> {
        delete_references_impl(context, address_space, references_to_delete);
        Ok(())
    }
}
