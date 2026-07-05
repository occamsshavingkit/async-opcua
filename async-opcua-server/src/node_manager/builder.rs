//! Ergonomic builder for a [`QuickNodeManager`], a wrapper around
//! [`SimpleNodeManagerImpl`] that reduces boilerplate for the common case of
//! building a small in-memory node manager with a handful of variables and
//! objects.
//!
//! ```ignore
//! use async_opcua_server::node_manager::builder::QuickNodeManager;
//! use async_opcua_server::node_manager::NodeManagerBuilder;
//!
//! fn make_node_manager() -> impl NodeManagerBuilder {
//!     QuickNodeManager::new("urn:example")
//!         .variable("counter", 0i32).writable().add()
//!         .variable("label", "hello").add()
//!         .object("device")
//!             .add_variable("power", false).add()
//!             .add()
//! }
//! ```

use std::sync::Arc;

use opcua_types::{
    DataValue, NodeId, NumericRange, ObjectId, ObjectTypeId, ReferenceTypeId, StatusCode,
    TimestampsToReturn, VariableTypeId, Variant,
};

use crate::address_space::{
    AddressSpace, ObjectBuilder as NodeObjectBuilder, VariableBuilder as NodeVariableBuilder,
};
use crate::node_manager::memory::{InMemoryNodeManager, SimpleNodeManagerImpl};
use crate::node_manager::{DynNodeManager, NamespaceMetadata, NodeManagerBuilder, ServerContext};

/// Argument bundle handed to read callbacks registered on a [`PendingVariable`].
///
/// The simple in-memory node manager invokes read callbacks with three loose
/// arguments (index range, timestamps-to-return, max age). [`QuickNodeManager`]
/// groups them into this struct so user callbacks have a single, named
/// parameter.
#[derive(Debug, Clone)]
pub struct ReadContext {
    /// Index range being read.
    pub index_range: NumericRange,
    /// Requested timestamps for the read operation.
    pub timestamps_to_return: TimestampsToReturn,
    /// Maximum age of the value, in milliseconds. Non-finite values may be
    /// sent by clients to indicate any cached value is acceptable.
    pub max_age: f64,
}

/// Argument bundle handed to write callbacks registered on a [`PendingVariable`].
#[derive(Debug, Clone)]
pub struct WriteContext {
    /// Index range being written.
    pub index_range: NumericRange,
}

/// Read callback type used by [`PendingVariable`].
pub type ReadCallback =
    Box<dyn Fn(&ReadContext) -> Result<DataValue, StatusCode> + Send + Sync + 'static>;

/// Write callback type used by [`PendingVariable`]. The callback receives the
/// value to apply and must return `Ok(())` if the write is accepted, or an
/// appropriate [`StatusCode`] otherwise.
pub type WriteCallback =
    Box<dyn Fn(&WriteContext, Variant) -> Result<(), StatusCode> + Send + Sync + 'static>;

/// Description of a variable that will be inserted into the address space when
/// the owning [`QuickNodeManager`] is built.
pub struct PendingVariable {
    /// Browse name _and_ display name of the variable. [`QuickNodeManager`] uses
    /// the same value for both, which is the common case for simple variables.
    pub name: String,
    /// Initial value held by the variable before any callback is invoked.
    pub initial_value: Variant,
    /// Whether the variable should accept client writes. Adds the
    /// `CurrentWrite` bit to the access level.
    pub writable: bool,
    /// Optional data type override. If `None`, the data type is inferred from
    /// `initial_value`.
    pub data_type: Option<NodeId>,
    /// Optional read callback. When set, the variable value is produced by
    /// invoking this callback instead of returning the stored initial value.
    pub read_cb: Option<ReadCallback>,
    /// Optional write callback. When set, client writes are forwarded to this
    /// callback instead of being applied directly to the stored value.
    pub write_cb: Option<WriteCallback>,
}

/// Description of an object that will be inserted into the address space when
/// the owning [`QuickNodeManager`] is built.
pub struct PendingObject {
    /// Browse name _and_ display name of the object.
    pub name: String,
    /// `HasTypeDefinition` reference target for the object. Defaults to
    /// [`ObjectTypeId::BaseObjectType`].
    pub type_definition: NodeId,
    /// Child variables of this object. Each child becomes a `HasComponent`
    /// reference from the object.
    pub children: Vec<PendingVariable>,
}

/// Quick node manager: an ergonomic wrapper that produces an
/// [`InMemoryNodeManager<SimpleNodeManagerImpl>`] from a small list of
/// pending variables and objects.
///
/// Use [`QuickNodeManager::new`] followed by chained calls to
/// [`QuickNodeManager::variable`] and [`QuickNodeManager::object`] to populate
/// it, then register it with a [`crate::ServerBuilder`] via
/// [`crate::ServerBuilder::with_node_manager`].
pub struct QuickNodeManager {
    /// Namespace URI for every node created by this manager. Registered in the
    /// global type tree and the address space when [`NodeManagerBuilder::build`]
    /// is called.
    pub namespace_uri: String,
    /// Top-level variables to insert under the server `Objects` folder.
    pub variables: Vec<PendingVariable>,
    /// Top-level objects to insert under the server `Objects` folder.
    pub objects: Vec<PendingObject>,
}

impl QuickNodeManager {
    /// Create a new quick node manager owning the given namespace URI.
    pub fn new(namespace_uri: &str) -> Self {
        Self {
            namespace_uri: namespace_uri.to_owned(),
            variables: Vec::new(),
            objects: Vec::new(),
        }
    }

    /// Begin defining a new top-level variable under the server's `Objects`
    /// folder. The returned [`VariableBuilder`] is used to configure the
    /// variable before it is committed with [`VariableBuilder::add`].
    pub fn variable<V: Into<Variant>>(self, name: &str, initial_value: V) -> VariableBuilder<Self> {
        VariableBuilder {
            parent: self,
            pending: PendingVariable {
                name: name.to_owned(),
                initial_value: initial_value.into(),
                writable: false,
                data_type: None,
                read_cb: None,
                write_cb: None,
            },
        }
    }

    /// Begin defining a new top-level object under the server's `Objects`
    /// folder. The returned [`ObjectBuilder`] is used to configure the object
    /// (and add child variables) before it is committed with
    /// [`ObjectBuilder::add`].
    pub fn object(self, name: &str) -> ObjectBuilder<Self> {
        ObjectBuilder {
            parent: self,
            pending: PendingObject {
                name: name.to_owned(),
                type_definition: ObjectTypeId::BaseObjectType.into(),
                children: Vec::new(),
            },
        }
    }

    /// Implementation detail shared with [`ObjectBuilder::add`]: push a
    /// finished [`PendingObject`] into `self.objects`.
    fn push_object(&mut self, object: PendingObject) {
        self.objects.push(object);
    }
}

/// Trait implemented by containers that can accept a finished [`PendingVariable`].
///
/// Implemented for [`QuickNodeManager`] (top-level variables) and for
/// [`ObjectBuilder`] (object child variables), so that
/// [`VariableBuilder::add`] can append to whichever builder produced it.
pub trait AcceptVariable {
    /// Append `variable` to this container, returning `self` for chaining.
    fn accept_variable(self, variable: PendingVariable) -> Self;
}

impl AcceptVariable for QuickNodeManager {
    fn accept_variable(mut self, variable: PendingVariable) -> Self {
        self.variables.push(variable);
        self
    }
}

impl NodeManagerBuilder for QuickNodeManager {
    fn build(self: Box<Self>, context: ServerContext) -> Arc<DynNodeManager> {
        build_quick_node_manager(*self, context)
    }
}

fn build_quick_node_manager(
    manager: QuickNodeManager,
    context: ServerContext,
) -> Arc<DynNodeManager> {
    // 1. Register the namespace in the global type tree.
    let ns_index = {
        let mut type_tree = context.type_tree.write();
        let ns_index = type_tree
            .namespaces_mut()
            .add_namespace(&manager.namespace_uri);
        context.info.publish_type_tree_snapshot(&type_tree);
        ns_index
    };

    // 2. Build a fresh address space, register the namespace, and add all the
    //    pending variables and objects.
    let address_space = AddressSpace::new();
    address_space.add_namespace(&manager.namespace_uri, ns_index);

    let objects_folder: NodeId = ObjectId::ObjectsFolder.into();

    // Collect (node_id, read_cb, write_cb) for every variable that has at
    // least one callback. Variables with no callback are still added to the
    // address space but skipped here.
    let mut callbacks: Vec<(NodeId, Option<ReadCallback>, Option<WriteCallback>)> = Vec::new();

    for var in manager.variables {
        let node_id = NodeId::new(ns_index, var.name.clone());
        insert_variable(&address_space, &node_id, &var, &objects_folder, false);
        if var.read_cb.is_some() || var.write_cb.is_some() {
            callbacks.push((node_id, var.read_cb, var.write_cb));
        }
    }

    for object in manager.objects {
        let object_id = NodeId::new(ns_index, object.name.clone());
        NodeObjectBuilder::new(&object_id, object.name.as_str(), object.name.as_str())
            .organized_by(objects_folder.clone())
            .reference(
                object.type_definition.clone(),
                ReferenceTypeId::HasTypeDefinition,
                opcua_nodes::ReferenceDirection::Forward,
            )
            .insert(&address_space);

        for child in object.children {
            // Hierarchical child id so two objects can have a child with the
            // same name without colliding.
            let child_id = NodeId::new(ns_index, format!("{}.{}", object.name, child.name));
            insert_variable(&address_space, &child_id, &child, &object_id, true);
            if child.read_cb.is_some() || child.write_cb.is_some() {
                callbacks.push((child_id, child.read_cb, child.write_cb));
            }
        }
    }

    // 3. Build the SimpleNodeManagerImpl that will own the callbacks and the
    //    InMemoryNodeManager wrapper that ties it to the address space.
    let namespaces = vec![NamespaceMetadata {
        namespace_uri: manager.namespace_uri.clone(),
        namespace_index: ns_index,
        ..Default::default()
    }];
    let inner = SimpleNodeManagerImpl::new(namespaces, "quick", context.node_managers.clone());

    // 4. Register read/write callbacks. Each user callback is wrapped so the
    //    simple node manager's loose-argument API is translated into the
    //    [`ReadContext`] / [`WriteContext`] structs.
    for (node_id, read_cb, write_cb) in callbacks {
        if let Some(read_cb) = read_cb {
            inner.add_read_callback(node_id.clone(), move |index_range, ttr, max_age| {
                let ctx = ReadContext {
                    index_range: index_range.clone(),
                    timestamps_to_return: ttr,
                    max_age,
                };
                read_cb(&ctx)
            });
        }
        if let Some(write_cb) = write_cb {
            inner.add_write_callback(node_id.clone(), move |value, index_range| {
                let ctx = WriteContext {
                    index_range: index_range.clone(),
                };
                // The simple node manager's write callback returns a
                // StatusCode directly; our user callback returns a Result,
                // so map Ok to Good.
                let incoming_value = value.value.unwrap_or_default();
                match write_cb(&ctx, incoming_value) {
                    Ok(()) => StatusCode::Good,
                    Err(e) => e,
                }
            });
        }
    }

    Arc::new(InMemoryNodeManager::new(inner, address_space))
}

/// Insert a [`PendingVariable`] into the address space as a variable node.
///
/// When `as_component` is true the variable is added as a `HasComponent` child
/// of `parent` (the OPC-UA convention for object → variable relationships).
/// Otherwise it is added as an `Organizes` child of `parent` (the convention
/// for nodes organized by a folder such as `ObjectsFolder`).
fn insert_variable(
    address_space: &AddressSpace,
    node_id: &NodeId,
    var: &PendingVariable,
    parent: &NodeId,
    as_component: bool,
) {
    let mut builder = NodeVariableBuilder::new(node_id, var.name.as_str(), var.name.as_str())
        .value(var.initial_value.clone());

    let data_type = var.data_type.clone().or_else(|| {
        var.initial_value
            .data_type()
            .map(|expanded| expanded.node_id)
    });
    if let Some(dt) = data_type {
        builder = builder.data_type(dt);
    }

    if var.writable {
        builder = builder.writable();
    }

    builder = builder.has_type_definition(VariableTypeId::BaseDataVariableType);

    if as_component {
        builder.component_of(parent.clone()).insert(address_space);
    } else {
        builder.organized_by(parent.clone()).insert(address_space);
    }
}

/// Chainable builder for a single variable. Generic over the parent builder `P`
/// so it can be returned by both [`QuickNodeManager::variable`] (where
/// `P = QuickNodeManager`) and [`ObjectBuilder::add_variable`] (where
/// `P = ObjectBuilder<QuickNodeManager>`).
pub struct VariableBuilder<P> {
    parent: P,
    pending: PendingVariable,
}

impl<P> VariableBuilder<P> {
    /// Mark this variable as writable by clients.
    pub fn writable(mut self) -> Self {
        self.pending.writable = true;
        self
    }

    /// Override the data type. When unset, the data type is inferred from the
    /// initial value at build time.
    pub fn data_type<T: Into<NodeId>>(mut self, data_type: T) -> Self {
        self.pending.data_type = Some(data_type.into());
        self
    }

    /// Register a read callback. When set, the variable's stored value is
    /// ignored at read time and the value returned by `cb` is sent to clients.
    pub fn read_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(&ReadContext) -> Result<DataValue, StatusCode> + Send + Sync + 'static,
    {
        self.pending.read_cb = Some(Box::new(cb));
        self
    }

    /// Register a write callback. When set, client writes are forwarded to
    /// `cb` instead of being applied to the stored value.
    pub fn write_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(&WriteContext, Variant) -> Result<(), StatusCode> + Send + Sync + 'static,
    {
        self.pending.write_cb = Some(Box::new(cb));
        self
    }
}

impl<P: AcceptVariable> VariableBuilder<P> {
    /// Commit this variable, returning the parent builder for further
    /// configuration.
    pub fn add(self) -> P {
        let VariableBuilder { parent, pending } = self;
        parent.accept_variable(pending)
    }
}

/// Chainable builder for a single object. Generic over the parent builder `P`,
/// which in practice is [`QuickNodeManager`].
pub struct ObjectBuilder<P> {
    parent: P,
    pending: PendingObject,
}

impl<P> ObjectBuilder<P> {
    /// Override the object's `HasTypeDefinition` reference. Defaults to
    /// [`ObjectTypeId::BaseObjectType`].
    pub fn type_definition<T: Into<NodeId>>(mut self, type_definition: T) -> Self {
        self.pending.type_definition = type_definition.into();
        self
    }

    /// Begin defining a child variable on this object. The returned
    /// [`VariableBuilder`] is configured the same way as a top-level variable;
    /// [`VariableBuilder::add`] returns this [`ObjectBuilder`] for further
    /// chaining.
    pub fn add_variable<V: Into<Variant>>(
        self,
        name: &str,
        initial_value: V,
    ) -> VariableBuilder<Self> {
        VariableBuilder {
            parent: self,
            pending: PendingVariable {
                name: name.to_owned(),
                initial_value: initial_value.into(),
                writable: false,
                data_type: None,
                read_cb: None,
                write_cb: None,
            },
        }
    }
}

impl AcceptVariable for ObjectBuilder<QuickNodeManager> {
    fn accept_variable(mut self, variable: PendingVariable) -> Self {
        self.pending.children.push(variable);
        self
    }
}

impl ObjectBuilder<QuickNodeManager> {
    /// Commit this object, returning the parent [`QuickNodeManager`] for
    /// further configuration.
    pub fn add(self) -> QuickNodeManager {
        let ObjectBuilder {
            mut parent,
            pending,
        } = self;
        parent.push_object(pending);
        parent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        address_space::NodeType,
        authenticator::UserToken,
        builder::ServerBuilder,
        identity_token::IdentityToken,
        node_manager::{
            memory::InMemoryNodeManagerImpl, NodeManagerCore, NodeManagersRef, ParsedReadValueId,
            RequestContext, RequestContextInner, WriteNode,
        },
        session::{instance::Session, manager::SessionManager},
        ServerStatusWrapper,
    };
    use opcua_core::sync::RwLock;
    use opcua_nodes::AccessLevel;
    use opcua_types::{
        ApplicationDescription, AttributeId, BuildInfo, ByteString, DataEncoding, DataValue,
        DiagnosticBits, MessageSecurityMode, NodeClass, NumericRange, TimestampsToReturn, UAString,
        WriteValue,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tokio::sync::Notify;

    fn make_server_context() -> ServerContext {
        let (_server, handle) = ServerBuilder::new_anonymous("quick-node-manager-test")
            .build()
            .expect("test server should build");
        let info = handle.info().clone();
        let session_manager = Arc::new(RwLock::new(SessionManager::new(
            info.clone(),
            Arc::new(Notify::new()),
        )));
        ServerContext {
            node_managers: NodeManagersRef::new_empty(),
            session_manager,
            #[cfg(feature = "subscriptions")]
            subscriptions: handle.subscriptions().clone(),
            info: info.clone(),
            authenticator: info.authenticator.clone(),
            type_tree: info.type_tree.clone(),
            type_tree_getter: info.type_tree_getter.clone(),
            status: Arc::new(ServerStatusWrapper::new(
                BuildInfo::default(),
                #[cfg(feature = "subscriptions")]
                handle.subscriptions().clone(),
            )),
        }
    }

    fn make_request_context() -> RequestContext {
        let (_server, handle) = ServerBuilder::new_anonymous("quick-node-manager-test")
            .build()
            .expect("test server should build");
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
            IdentityToken::Anonymous(opcua_types::AnonymousIdentityToken {
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
                #[cfg(feature = "subscriptions")]
                subscriptions: handle.subscriptions().clone(),
                info,
            }),
        }
    }

    fn build_and_downcast(
        qnm: QuickNodeManager,
    ) -> Arc<InMemoryNodeManager<SimpleNodeManagerImpl>> {
        let ctx = make_server_context();
        let nm: Arc<DynNodeManager> = Box::new(qnm).build(ctx);
        nm.into_any_arc()
            .downcast::<InMemoryNodeManager<SimpleNodeManagerImpl>>()
            .expect("should be InMemoryNodeManager<SimpleNodeManagerImpl>")
    }

    fn get_ns_index(nm: &InMemoryNodeManager<SimpleNodeManagerImpl>, uri: &str) -> u16 {
        nm.namespaces()
            .iter()
            .find(|(_, ns_uri)| ns_uri.as_str() == uri)
            .map(|(idx, _)| *idx)
            .expect("namespace should be registered")
    }

    #[tokio::test]
    async fn single_variable_created() {
        let qnm = QuickNodeManager::new("test-ns")
            .variable("Counter", 42u32)
            .add();
        let nm = build_and_downcast(qnm);
        let ns = get_ns_index(&nm, "test-ns");
        let node_id = NodeId::new(ns, "Counter");

        assert!(
            nm.owns_node(&node_id),
            "node manager should own the Counter variable"
        );

        let addr = nm.address_space().read();
        let node = addr
            .find_node(&node_id)
            .expect("variable should exist in address space");
        assert_eq!(node.as_node().node_class(), NodeClass::Variable);
    }

    #[tokio::test]
    async fn writable_variable() {
        let qnm = QuickNodeManager::new("ns-writable")
            .variable("WriteTarget", 0i32)
            .writable()
            .add();
        let nm = build_and_downcast(qnm);
        let ns = get_ns_index(&nm, "ns-writable");
        let node_id = NodeId::new(ns, "WriteTarget");

        assert!(nm.owns_node(&node_id));

        let addr = nm.address_space().read();
        let node = addr
            .find(&node_id)
            .expect("variable should exist in address space");
        if let NodeType::Variable(v) = &*node {
            assert!(
                v.access_level().contains(AccessLevel::CURRENT_WRITE),
                "access level should include CurrentWrite"
            );
            assert!(
                v.user_access_level().contains(AccessLevel::CURRENT_WRITE),
                "user access level should include CurrentWrite"
            );
        } else {
            panic!(
                "expected a Variable node, got {:?}",
                node.as_node().node_class()
            );
        }
    }

    #[tokio::test]
    async fn multiple_variables() {
        let qnm = QuickNodeManager::new("ns-multi")
            .variable("Var1", 10u16)
            .add()
            .variable("Var2", 20u16)
            .add()
            .variable("Var3", 30u16)
            .add();
        let nm = build_and_downcast(qnm);
        let ns = get_ns_index(&nm, "ns-multi");

        for name in ["Var1", "Var2", "Var3"] {
            let node_id = NodeId::new(ns, name);
            assert!(nm.owns_node(&node_id), "node manager should own {}", name);
            let addr = nm.address_space().read();
            assert!(
                addr.find_node(&node_id).is_some(),
                "{} should exist in address space",
                name
            );
        }
    }

    #[tokio::test]
    async fn object_with_children() {
        let qnm = QuickNodeManager::new("ns-obj")
            .object("Folder")
            .add_variable("ChildVar", 99i64)
            .add()
            .add();
        let nm = build_and_downcast(qnm);
        let ns = get_ns_index(&nm, "ns-obj");

        let object_id = NodeId::new(ns, "Folder");
        let child_id = NodeId::new(ns, "Folder.ChildVar");

        assert!(nm.owns_node(&object_id), "should own the Folder object");
        assert!(
            nm.owns_node(&child_id),
            "should own the Folder.ChildVar variable"
        );

        let addr = nm.address_space().read();
        let object_node = addr
            .find_node(&object_id)
            .expect("Folder object should exist");
        assert_eq!(object_node.as_node().node_class(), NodeClass::Object);

        let child_node = addr.find_node(&child_id).expect("ChildVar should exist");
        assert_eq!(child_node.as_node().node_class(), NodeClass::Variable);
    }

    #[tokio::test]
    async fn custom_read_callback() {
        let read_called = Arc::new(AtomicBool::new(false));
        let called = read_called.clone();

        let qnm = QuickNodeManager::new("ns-readcb")
            .variable("Dynamic", 0u32)
            .read_callback(move |_ctx| {
                called.store(true, Ordering::SeqCst);
                Ok(DataValue::new_now(99u32))
            })
            .add();
        let nm = build_and_downcast(qnm);
        let ns = get_ns_index(&nm, "ns-readcb");
        let node_id = NodeId::new(ns, "Dynamic");

        let ctx = make_request_context();
        let parsed = ParsedReadValueId {
            node_id: node_id.clone(),
            attribute_id: AttributeId::Value,
            index_range: NumericRange::None,
            data_encoding: DataEncoding::Binary,
        };

        let values = nm
            .inner()
            .read_values(
                &ctx,
                nm.address_space(),
                &[&parsed],
                0.0,
                TimestampsToReturn::Neither,
            )
            .await;

        assert_eq!(values.len(), 1);
        assert!(
            read_called.load(Ordering::SeqCst),
            "read callback should have been invoked"
        );

        let val = values.into_iter().next().unwrap();
        if let Some(v) = val.value {
            assert_eq!(v, Variant::from(99u32));
        } else {
            panic!("expected a value from read callback");
        }
    }

    #[tokio::test]
    async fn custom_write_callback() {
        let write_called = Arc::new(AtomicBool::new(false));
        let called = write_called.clone();

        let qnm = QuickNodeManager::new("ns-writecb")
            .variable("WritableDynamic", 0u32)
            .writable()
            .write_callback(move |_ctx, val| {
                called.store(true, Ordering::SeqCst);
                assert_eq!(val, Variant::from(77u32));
                Ok(())
            })
            .add();
        let nm = build_and_downcast(qnm);
        let ns = get_ns_index(&nm, "ns-writecb");
        let node_id = NodeId::new(ns, "WritableDynamic");

        let ctx = make_request_context();

        let mut write_node = WriteNode::new(
            WriteValue {
                node_id: node_id.clone(),
                attribute_id: AttributeId::Value as u32,
                index_range: NumericRange::None,
                value: DataValue::new_now(77u32),
            },
            DiagnosticBits::empty(),
        );

        nm.inner()
            .write(&ctx, nm.address_space(), &mut [&mut write_node])
            .await
            .expect("write should succeed");

        assert!(
            write_called.load(Ordering::SeqCst),
            "write callback should have been invoked"
        );
    }
}
