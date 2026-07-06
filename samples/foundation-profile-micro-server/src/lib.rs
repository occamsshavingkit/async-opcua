//! Micro Embedded Device 2017 Server Profile benchmark.
//!
//! Targets `http://opcfoundation.org/UA-Profile/Server/MicroEmbeddedDevice2017`:
//! the Nano surface plus basic data-change subscriptions (Embedded DataChange
//! Subscription Server Facet) and at least two parallel sessions.
//! This is a footprint benchmark surface, NOT an OPC Foundation conformance claim.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc, OnceLock,
    },
};

use opcua::crypto::SecurityPolicy;
use opcua::nodes::{
    ImportedItem, ImportedReference, NodeSetImport, NodeSetNamespaceMapper, ObjectBuilder,
    ReferenceTypeBuilder, VariableBuilder,
};
use opcua::server::{
    address_space::AddressSpace,
    node_manager::{
        memory::{
            InMemoryNodeManagerBuilder, InMemoryNodeManagerImplBuilder, SimpleNodeManagerBuilder,
            SimpleNodeManagerImpl,
        },
        ServerContext,
    },
    Limits, OperationalLimits, ServerBuilder, SubscriptionLimits, ANONYMOUS_USER_TOKEN_ID,
};
use opcua::types::{
    BuildInfo, DataTypeId, DataValue, DateTime, ExtensionObject, LocalizedText,
    MessageSecurityMode, NodeId, ObjectId, ObjectTypeId, ReferenceTypeId, ServerState,
    ServerStatusDataType, UAString, VariableId, VariableTypeId,
};

/// Short profile key used for PKI directories and application URIs.
pub const PROFILE_KEY: &str = "micro";
/// Human-readable benchmark name.
pub const PROFILE_DISPLAY_NAME: &str = "Micro Embedded Device 2017 Server Profile benchmark";
/// The OPC Foundation profile URI this benchmark targets (reporting only).
pub const PROFILE_TARGET_URI: &str =
    "http://opcfoundation.org/UA-Profile/Server/MicroEmbeddedDevice2017";
/// One-line description of the served surface.
pub const PROFILE_SURFACE: &str = "Nano benchmark surface plus bounded subscription capacity";

/// NodeId of the demo counter variable the sample ticks for data-change
/// subscriptions (Monitor Value Change CU). Lives in the benchmark's demo
/// namespace (ns=1).
pub fn demo_counter_node_id() -> NodeId {
    let ns = *DEMO_NS_SERVER_INDEX.get().unwrap_or(&DEMO_NS_IN_NODESET);
    NodeId::new(ns, "demo_counter")
}

/// Micro capacity: ≥2 parallel sessions ("Session Minimum 2 Parallel"),
/// ≥1 subscription with ≥2 monitored items (Embedded DataChange facet).
pub fn profile_limits() -> Limits {
    Limits {
        max_sessions: 2,
        max_inflight_requests_per_connection: 16,
        subscriptions: SubscriptionLimits {
            max_subscriptions_per_session: 2,
            max_monitored_items_per_sub: 8,
            ..Default::default()
        },
        operational: OperationalLimits {
            max_nodes_per_read: 128,
            max_nodes_per_write: 128,
            max_nodes_per_browse: 128,
            max_monitored_items_per_call: 10,
            ..Default::default()
        },
        ..Default::default()
    }
}

const NAMESPACE_URI: &str = "http://opcfoundation.org/UA/";
const DEMO_NAMESPACE_URI: &str = "urn:async-opcua:micro-demo";
const DEMO_NS_IN_NODESET: u16 = 1;
static DEMO_NS_SERVER_INDEX: OnceLock<u16> = OnceLock::new();

struct MicroNamespace;

impl NodeSetImport for MicroNamespace {
    fn register_namespaces(&self, namespaces: &mut NodeSetNamespaceMapper) {
        // Register the demo namespace against its in-nodeset index (1) so the
        // mapper can resolve NodeIds emitted with ns=1 below.
        namespaces.add_namespace(DEMO_NAMESPACE_URI, DEMO_NS_IN_NODESET);
    }

    fn get_own_namespaces(&self) -> Vec<String> {
        vec![NAMESPACE_URI.to_owned(), DEMO_NAMESPACE_URI.to_owned()]
    }

    fn load<'a>(
        &'a self,
        namespaces: &'a NodeSetNamespaceMapper,
    ) -> Box<dyn Iterator<Item = ImportedItem> + 'a> {
        let server_ns = namespaces
            .get_index(DEMO_NS_IN_NODESET)
            .unwrap_or(DEMO_NS_IN_NODESET);
        let _ = DEMO_NS_SERVER_INDEX.set(server_ns);
        Box::new(
            micro_namespace_nodes()
                .into_iter()
                .chain(std::iter::once(demo_counter_variable(server_ns))),
        )
    }
}

/// Build the Micro benchmark server: policy-None endpoint, anonymous identity,
/// profile-shaped limits, minimal ns0 address space, and a ticking demo counter
/// variable that drives data-change subscriptions.
pub fn build_server(pki_dir: impl Into<PathBuf>) -> ServerBuilder {
    let user_token_ids = [ANONYMOUS_USER_TOKEN_ID];
    let counter_node = demo_counter_node_id();
    let counter = Arc::new(AtomicI32::new(0));

    ServerBuilder::new()
        .application_name(format!("async-opcua {PROFILE_DISPLAY_NAME}"))
        .application_uri(format!(
            "urn:async-opcua:foundation-profile-benchmark:{PROFILE_KEY}",
        ))
        .product_uri("https://github.com/freeopcua/async-opcua")
        .pki_dir(pki_dir)
        .limits(profile_limits())
        .add_endpoint(
            "none",
            (
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                &user_token_ids as &[&str],
            ),
        )
        .discovery_urls(vec!["/".to_owned()])
        .with_node_manager(InMemoryNodeManagerBuilder::new(
            move |context: ServerContext,
                  address_space: &AddressSpace|
                  -> SimpleNodeManagerImpl {
                let builder = SimpleNodeManagerBuilder::new_imports(
                    vec![Box::new(MicroNamespace)],
                    "micro-ns0",
                );
                let inner = builder.build(context, address_space);
                let c = counter.clone();
                inner.add_read_callback(counter_node.clone(), move |_, _, _| {
                    Ok(DataValue::new_now(c.fetch_add(1, Ordering::Relaxed)))
                });
                inner
            },
        ))
}

fn micro_namespace_nodes() -> Vec<ImportedItem> {
    let start_time = DateTime::now();
    let server_status = ServerStatusDataType {
        start_time,
        current_time: DateTime::now(),
        state: ServerState::Running,
        build_info: BuildInfo {
            product_uri: UAString::from("https://github.com/freeopcua/async-opcua"),
            product_name: UAString::from(PROFILE_DISPLAY_NAME),
            ..Default::default()
        },
        seconds_till_shutdown: 0,
        shutdown_reason: LocalizedText::null(),
    };

    vec![
        reference_type(
            ReferenceTypeId::HierarchicalReferences,
            "HierarchicalReferences",
            ReferenceTypeId::References,
            true,
        ),
        reference_type(
            ReferenceTypeId::Organizes,
            "Organizes",
            ReferenceTypeId::HierarchicalReferences,
            false,
        ),
        folder(ObjectId::RootFolder, "Root", None),
        folder(
            ObjectId::ObjectsFolder,
            "Objects",
            Some(ObjectId::RootFolder.into()),
        ),
        folder(
            ObjectId::TypesFolder,
            "Types",
            Some(ObjectId::RootFolder.into()),
        ),
        folder(
            ObjectId::ViewsFolder,
            "Views",
            Some(ObjectId::RootFolder.into()),
        ),
        folder(
            ObjectId::ObjectTypesFolder,
            "ObjectTypes",
            Some(ObjectId::TypesFolder.into()),
        ),
        object(ObjectId::Server, "Server", ObjectId::ObjectsFolder),
        variable(
            VariableId::Server_ServerStatus,
            "ServerStatus",
            DataTypeId::ServerStatusDataType,
            ExtensionObject::from_message(server_status),
            VariableTypeId::BaseDataVariableType,
            ObjectId::Server.into(),
        ),
        variable(
            VariableId::Server_ServerStatus_StartTime,
            "StartTime",
            DataTypeId::UtcTime,
            start_time,
            VariableTypeId::PropertyType,
            VariableId::Server_ServerStatus.into(),
        ),
        variable(
            VariableId::Server_ServerStatus_CurrentTime,
            "CurrentTime",
            DataTypeId::UtcTime,
            DateTime::now(),
            VariableTypeId::PropertyType,
            VariableId::Server_ServerStatus.into(),
        ),
        variable(
            VariableId::Server_ServerStatus_State,
            "State",
            DataTypeId::ServerState,
            ServerState::Running as i32,
            VariableTypeId::PropertyType,
            VariableId::Server_ServerStatus.into(),
        ),
    ]
}

fn reference_type(
    node_id: ReferenceTypeId,
    browse_name: &str,
    super_type: ReferenceTypeId,
    is_abstract: bool,
) -> ImportedItem {
    ImportedItem {
        node: ReferenceTypeBuilder::new(&node_id.into(), browse_name, browse_name)
            .is_abstract(is_abstract)
            .build()
            .into(),
        references: vec![inverse_reference(
            super_type.into(),
            ReferenceTypeId::HasSubtype,
        )],
    }
}

fn folder(node_id: ObjectId, browse_name: &str, parent: Option<NodeId>) -> ImportedItem {
    let mut references = vec![forward_reference(
        ObjectTypeId::FolderType.into(),
        ReferenceTypeId::HasTypeDefinition,
    )];
    if let Some(parent) = parent {
        references.push(inverse_reference(parent, ReferenceTypeId::Organizes));
    }
    ImportedItem {
        node: ObjectBuilder::new(&node_id.into(), browse_name, browse_name)
            .build()
            .into(),
        references,
    }
}

fn object(node_id: ObjectId, browse_name: &str, parent: ObjectId) -> ImportedItem {
    ImportedItem {
        node: ObjectBuilder::new(&node_id.into(), browse_name, browse_name)
            .build()
            .into(),
        references: vec![
            forward_reference(
                ObjectTypeId::ServerType.into(),
                ReferenceTypeId::HasTypeDefinition,
            ),
            inverse_reference(parent.into(), ReferenceTypeId::Organizes),
        ],
    }
}

fn variable(
    node_id: VariableId,
    browse_name: &str,
    data_type: DataTypeId,
    value: impl Into<opcua::types::Variant>,
    type_definition: VariableTypeId,
    parent: NodeId,
) -> ImportedItem {
    ImportedItem {
        node: VariableBuilder::new(&node_id.into(), browse_name, browse_name)
            .data_type(data_type)
            .value(value)
            .build()
            .into(),
        references: vec![
            forward_reference(type_definition.into(), ReferenceTypeId::HasTypeDefinition),
            inverse_reference(parent, ReferenceTypeId::HasComponent),
        ],
    }
}

fn demo_counter_variable(namespace: u16) -> ImportedItem {
    let node_id = NodeId::new(namespace, "demo_counter");
    ImportedItem {
        node: VariableBuilder::new(&node_id, "demo_counter", "demo_counter")
            .data_type(DataTypeId::Int32)
            .value(0i32)
            .build()
            .into(),
        references: vec![
            forward_reference(
                VariableTypeId::BaseDataVariableType.into(),
                ReferenceTypeId::HasTypeDefinition,
            ),
            inverse_reference(
                ObjectId::ObjectsFolder.into(),
                ReferenceTypeId::HasComponent,
            ),
        ],
    }
}

fn forward_reference(target_id: NodeId, type_id: ReferenceTypeId) -> ImportedReference {
    ImportedReference {
        target_id,
        type_id: type_id.into(),
        is_forward: true,
    }
}

fn inverse_reference(target_id: NodeId, type_id: ReferenceTypeId) -> ImportedReference {
    ImportedReference {
        target_id,
        type_id: type_id.into(),
        is_forward: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_targets_expected_uri() {
        assert_eq!(
            PROFILE_TARGET_URI,
            "http://opcfoundation.org/UA-Profile/Server/MicroEmbeddedDevice2017"
        );
    }
}
