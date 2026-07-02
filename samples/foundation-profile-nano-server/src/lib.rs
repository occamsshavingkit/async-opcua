//! Nano Embedded Device 2017 Server Profile benchmark.
//!
//! Targets `http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2017`:
//! UA-TCP binary transport, SecurityPolicy None, sessions, read, and view services.
//! This is a footprint benchmark surface, NOT an OPC Foundation conformance claim.

use std::path::PathBuf;

use opcua::crypto::SecurityPolicy;
use opcua::nodes::{
    ImportedItem, ImportedReference, NodeSetImport, NodeSetNamespaceMapper, ObjectBuilder,
    ReferenceTypeBuilder, VariableBuilder,
};
use opcua::server::{
    node_manager::memory::simple_node_manager_imports, Limits, OperationalLimits, ServerBuilder,
    ServerUserToken, SubscriptionLimits, ANONYMOUS_USER_TOKEN_ID,
};
use opcua::types::{
    BuildInfo, DataTypeId, DateTime, ExtensionObject, LocalizedText, MessageSecurityMode, NodeId,
    ObjectId, ObjectTypeId, ReferenceTypeId, ServerState, ServerStatusDataType, UAString,
    VariableId, VariableTypeId,
};

/// Short profile key used for PKI directories and application URIs.
pub const PROFILE_KEY: &str = "nano";
/// Human-readable benchmark name.
pub const PROFILE_DISPLAY_NAME: &str = "Nano Embedded Device 2017 Server Profile benchmark";
/// The OPC Foundation profile URI this benchmark targets (reporting only).
pub const PROFILE_TARGET_URI: &str =
    "http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2017";
/// One-line description of the served surface.
pub const PROFILE_SURFACE: &str =
    "OPC UA TCP, SecurityPolicy None, Anonymous identity, sessions, read, and view";

/// User-name/password demo credentials. The Nano profile's Core 2017 Server Facet
/// includes the "User Token – User Name Password Server Facet" as MANDATORY, so the
/// benchmark server accepts this token (over the policy-None endpoint) in addition to
/// Anonymous.
pub const DEMO_USER_TOKEN_ID: &str = "nano_user";
/// Demo username for [`DEMO_USER_TOKEN_ID`].
pub const DEMO_USERNAME: &str = "nano-user";
/// Demo password for [`DEMO_USER_TOKEN_ID`].
pub const DEMO_PASSWORD: &str = "nano-pass";

/// Nano capacity: the profile mandates a single session ("Session Minimum 1").
pub fn profile_limits() -> Limits {
    Limits {
        max_sessions: 1,
        max_inflight_requests_per_connection: 16,
        subscriptions: SubscriptionLimits {
            max_subscriptions_per_session: 0,
            max_pending_publish_requests: 0,
            max_publish_requests_per_subscription: 0,
            max_monitored_items_per_sub: 1,
            max_notifications_per_publish: 1,
            ..Default::default()
        },
        operational: OperationalLimits {
            max_nodes_per_read: 64,
            max_nodes_per_write: 64,
            max_nodes_per_browse: 64,
            max_monitored_items_per_call: 1,
            ..Default::default()
        },
        ..Default::default()
    }
}

struct NanoNamespace;

impl NodeSetImport for NanoNamespace {
    fn register_namespaces(&self, _namespaces: &mut NodeSetNamespaceMapper) {}

    fn get_own_namespaces(&self) -> Vec<String> {
        vec![NAMESPACE_URI.to_owned()]
    }

    fn load<'a>(
        &'a self,
        _namespaces: &'a NodeSetNamespaceMapper,
    ) -> Box<dyn Iterator<Item = ImportedItem> + 'a> {
        Box::new(nano_namespace_nodes().into_iter())
    }
}

const NAMESPACE_URI: &str = "http://opcfoundation.org/UA/";

/// Build the Nano benchmark server: policy-None endpoint, anonymous identity,
/// profile-shaped limits.
pub fn build_server(pki_dir: impl Into<PathBuf>) -> ServerBuilder {
    let user_token_ids = [ANONYMOUS_USER_TOKEN_ID, DEMO_USER_TOKEN_ID];

    ServerBuilder::new()
        .application_name(format!("async-opcua {PROFILE_DISPLAY_NAME}"))
        .application_uri(format!(
            "urn:async-opcua:foundation-profile-benchmark:{PROFILE_KEY}",
        ))
        .product_uri("https://github.com/freeopcua/async-opcua")
        .pki_dir(pki_dir)
        .limits(profile_limits())
        .add_user_token(
            DEMO_USER_TOKEN_ID,
            ServerUserToken::user_pass(DEMO_USERNAME, DEMO_PASSWORD),
        )
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
        .with_node_manager(simple_node_manager_imports(
            vec![Box::new(NanoNamespace)],
            "nano-ns0",
        ))
}

fn nano_namespace_nodes() -> Vec<ImportedItem> {
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
    use std::{env, fs, time::SystemTime};

    #[test]
    fn benchmark_targets_expected_uri() {
        assert_eq!(
            PROFILE_TARGET_URI,
            "http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2017"
        );
    }

    #[tokio::test]
    async fn benchmark_server_does_not_advertise_profile_conformance() {
        let pki_dir = unique_pki_dir();
        let (_server, handle) = build_server(&pki_dir)
            .build()
            .expect("profile benchmark server should build");

        assert!(handle.info().capabilities.profiles.is_empty());

        let _ = fs::remove_dir_all(pki_dir);
    }

    fn unique_pki_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "async-opcua-foundation-profile-benchmark-{PROFILE_KEY}-{}-{nonce}",
            std::process::id()
        ))
    }
}
