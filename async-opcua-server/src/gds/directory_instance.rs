//! Instantiates a real `CertificateDirectoryType` "Directory" object (OPC UA Part 12 §7.9.2)
//! from the GDS companion NodeSet import, since that companion spec ships only the type
//! definition -- no pre-instantiated singleton, unlike `ServerConfigurationType` in the core
//! nodeset (see `specs/103-gds-pull-fix/research.md`).
//!
//! This is intentionally narrow: it instantiates exactly the object graph
//! `CertificateDirectoryType` needs (its six Mandatory methods, plus a
//! `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree built from *core*
//! namespace-0 types, which already exist in the generated nodeset), not a general-purpose
//! "instantiate any ObjectType from its Mandatory modelling rules" engine.

use tracing::warn;

use opcua_types::{Argument, DataTypeId, LocalizedText, NodeId, ObjectTypeId};

use crate::address_space::{AddressSpace, MethodBuilder, ObjectBuilder};

/// The GDS companion namespace URI (declared in the NodeSet2.xml's own `<NamespaceUris>`).
const GDS_NAMESPACE_URI: &str = "http://opcfoundation.org/UA/GDS/";

/// `CertificateDirectoryType`'s identifier in the GDS companion NodeSet2.xml. Namespace-index
/// remapping during import preserves this identifier unchanged (verified: `NodeSetNamespaceMapper`
/// only ever remaps the namespace portion of a NodeId -- see research.md).
const CERTIFICATE_DIRECTORY_TYPE_ID: u32 = 63;

/// Real NodeIds of the instantiated Directory object and its methods/subtree, resolved once at
/// server-startup wiring time.
#[derive(Debug, Clone)]
pub struct DirectoryInstanceNodeIds {
    /// The instantiated `CertificateDirectoryType` "Directory" object.
    pub directory_object_id: NodeId,
    /// Instance method NodeId.
    pub start_signing_request_id: NodeId,
    /// Instance method NodeId.
    pub start_new_key_pair_request_id: NodeId,
    /// Instance method NodeId.
    pub finish_request_id: NodeId,
    /// Instance method NodeId.
    pub get_certificate_groups_id: NodeId,
    /// Instance method NodeId.
    pub get_trust_list_id: NodeId,
    /// Instance method NodeId.
    pub get_certificate_status_id: NodeId,
    /// The `DefaultApplicationGroup` object instantiated under `CertificateGroups`.
    pub default_application_group_id: NodeId,
    /// The TrustList object under `DefaultApplicationGroup` -- what `GetTrustList` returns.
    pub default_application_group_trust_list_id: NodeId,
}

/// Resolves the namespace index the GDS companion import assigned to its own namespace URI, by
/// reverse-lookup against `AddressSpace::namespaces()`. Returns `None` if the GDS companion
/// NodeSet was never imported (feature disabled, or the XML file wasn't present locally --
/// `companion::import_gds` already warns and no-ops in that case).
fn resolve_gds_namespace(address_space: &AddressSpace) -> Option<u16> {
    address_space
        .namespaces()
        .into_iter()
        .find(|(_, uri)| uri == GDS_NAMESPACE_URI)
        .map(|(index, _)| index)
}

/// Instantiates the `CertificateDirectoryType` "Directory" object graph in `address_space`,
/// assuming the GDS companion NodeSet has already been imported into it (via
/// `companion::import_gds`). Fails closed (returns `None`, logs a warning) if the companion type
/// isn't actually present -- never panics.
pub fn instantiate_certificate_directory(
    address_space: &AddressSpace,
) -> Option<DirectoryInstanceNodeIds> {
    let Some(gds_ns) = resolve_gds_namespace(address_space) else {
        warn!(
            "GDS companion NodeSet not imported (namespace '{}' not found) -- \
             Pull-model Certificate Directory will not be instantiated",
            GDS_NAMESPACE_URI
        );
        return None;
    };

    let certificate_directory_type_id = NodeId::new(gds_ns, CERTIFICATE_DIRECTORY_TYPE_ID);
    if address_space.find(&certificate_directory_type_id).is_none() {
        warn!(
            "CertificateDirectoryType ({certificate_directory_type_id}) not found in the \
             imported GDS companion NodeSet -- Pull-model Certificate Directory will not be \
             instantiated"
        );
        return None;
    }

    let directory_object_id = NodeId::new(gds_ns, "Directory");
    ObjectBuilder::new(&directory_object_id, "Directory", "Directory")
        .has_type_definition(certificate_directory_type_id)
        .organized_by(NodeId::from(opcua_types::ObjectId::ObjectsFolder))
        .insert(address_space);

    let start_signing_request_id = insert_method(
        address_space,
        gds_ns,
        &directory_object_id,
        "StartSigningRequest",
        &[
            argument("ApplicationId", DataTypeId::NodeId),
            argument("CertificateGroupId", DataTypeId::NodeId),
            argument("CertificateTypeId", DataTypeId::NodeId),
            argument("CertificateRequest", DataTypeId::ByteString),
        ],
        &[argument("RequestId", DataTypeId::NodeId)],
    );
    let start_new_key_pair_request_id = insert_method(
        address_space,
        gds_ns,
        &directory_object_id,
        "StartNewKeyPairRequest",
        &[
            argument("ApplicationId", DataTypeId::NodeId),
            argument("CertificateGroupId", DataTypeId::NodeId),
            argument("CertificateTypeId", DataTypeId::NodeId),
            argument("SubjectName", DataTypeId::String),
            array_argument("DomainNames", DataTypeId::String),
            argument("PrivateKeyFormat", DataTypeId::String),
            argument("PrivateKeyPassword", DataTypeId::String),
        ],
        &[argument("RequestId", DataTypeId::NodeId)],
    );
    let finish_request_id = insert_method(
        address_space,
        gds_ns,
        &directory_object_id,
        "FinishRequest",
        &[
            argument("ApplicationId", DataTypeId::NodeId),
            argument("RequestId", DataTypeId::NodeId),
        ],
        &[
            argument("Certificate", DataTypeId::ByteString),
            argument("PrivateKey", DataTypeId::ByteString),
            array_argument("IssuerCertificates", DataTypeId::ByteString),
        ],
    );
    let get_certificate_groups_id = insert_method(
        address_space,
        gds_ns,
        &directory_object_id,
        "GetCertificateGroups",
        &[argument("ApplicationId", DataTypeId::NodeId)],
        &[array_argument("CertificateGroupIds", DataTypeId::NodeId)],
    );
    let get_trust_list_id = insert_method(
        address_space,
        gds_ns,
        &directory_object_id,
        "GetTrustList",
        &[
            argument("ApplicationId", DataTypeId::NodeId),
            argument("CertificateGroupId", DataTypeId::NodeId),
        ],
        &[argument("TrustListId", DataTypeId::NodeId)],
    );
    let get_certificate_status_id = insert_method(
        address_space,
        gds_ns,
        &directory_object_id,
        "GetCertificateStatus",
        &[
            argument("ApplicationId", DataTypeId::NodeId),
            argument("CertificateGroupId", DataTypeId::NodeId),
            argument("CertificateTypeId", DataTypeId::NodeId),
        ],
        &[argument("UpdateRequired", DataTypeId::Boolean)],
    );

    // CertificateGroups / DefaultApplicationGroup / TrustList subtree -- built from *core*
    // namespace-0 types (CertificateGroupFolderType/CertificateGroupType/TrustListType already
    // exist in the generated nodeset; only CertificateDirectoryType itself is companion-specific).
    let certificate_groups_id = NodeId::new(gds_ns, "Directory.CertificateGroups");
    ObjectBuilder::new(
        &certificate_groups_id,
        "CertificateGroups",
        "CertificateGroups",
    )
    .has_type_definition(ObjectTypeId::CertificateGroupFolderType)
    .organized_by(directory_object_id.clone())
    .insert(address_space);

    let default_application_group_id = NodeId::new(
        gds_ns,
        "Directory.CertificateGroups.DefaultApplicationGroup",
    );
    ObjectBuilder::new(
        &default_application_group_id,
        "DefaultApplicationGroup",
        "DefaultApplicationGroup",
    )
    .has_type_definition(ObjectTypeId::CertificateGroupType)
    .organized_by(certificate_groups_id.clone())
    .insert(address_space);

    let default_application_group_trust_list_id = NodeId::new(
        gds_ns,
        "Directory.CertificateGroups.DefaultApplicationGroup.TrustList",
    );
    ObjectBuilder::new(
        &default_application_group_trust_list_id,
        "TrustList",
        "TrustList",
    )
    .has_type_definition(ObjectTypeId::TrustListType)
    .component_of(default_application_group_id.clone())
    .insert(address_space);

    Some(DirectoryInstanceNodeIds {
        directory_object_id,
        start_signing_request_id,
        start_new_key_pair_request_id,
        finish_request_id,
        get_certificate_groups_id,
        get_trust_list_id,
        get_certificate_status_id,
        default_application_group_id,
        default_application_group_trust_list_id,
    })
}

fn insert_method(
    address_space: &AddressSpace,
    gds_ns: u16,
    parent_id: &NodeId,
    name: &str,
    input_args: &[Argument],
    output_args: &[Argument],
) -> NodeId {
    let method_id = NodeId::new(gds_ns, format!("Directory.{name}"));
    let input_args_id = NodeId::new(gds_ns, format!("Directory.{name}.InputArguments"));
    let output_args_id = NodeId::new(gds_ns, format!("Directory.{name}.OutputArguments"));

    let mut builder = MethodBuilder::new(&method_id, name, name).component_of(parent_id.clone());
    if !input_args.is_empty() {
        builder = builder.input_args(address_space, &input_args_id, input_args);
    }
    if !output_args.is_empty() {
        builder = builder.output_args(address_space, &output_args_id, output_args);
    }
    builder.insert(address_space);

    method_id
}

fn argument(name: &str, data_type: DataTypeId) -> Argument {
    Argument {
        name: name.into(),
        data_type: data_type.into(),
        value_rank: -1,
        array_dimensions: None,
        description: LocalizedText::null(),
    }
}

fn array_argument(name: &str, data_type: DataTypeId) -> Argument {
    Argument {
        name: name.into(),
        data_type: data_type.into(),
        value_rank: 1,
        array_dimensions: Some(vec![0]),
        description: LocalizedText::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xml_present() -> bool {
        std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml"
        ))
        .exists()
    }

    #[test]
    fn instantiates_a_real_directory_object_when_companion_xml_is_present() {
        if !xml_present() {
            eprintln!(
                "skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally"
            );
            return;
        }

        let address_space = AddressSpace::new();
        let rw = parking_lot::RwLock::new(address_space);
        crate::companion::import_gds(&rw);

        let ids = {
            let guard = rw.read();
            instantiate_certificate_directory(&guard)
        };
        let ids = ids.expect("companion XML is present, instantiation should succeed");

        let guard = rw.read();
        assert!(guard.find(&ids.directory_object_id).is_some());
        assert!(guard.find(&ids.start_signing_request_id).is_some());
        assert!(guard.find(&ids.start_new_key_pair_request_id).is_some());
        assert!(guard.find(&ids.finish_request_id).is_some());
        assert!(guard.find(&ids.get_certificate_groups_id).is_some());
        assert!(guard.find(&ids.get_trust_list_id).is_some());
        assert!(guard.find(&ids.get_certificate_status_id).is_some());
        assert!(guard.find(&ids.default_application_group_id).is_some());
        assert!(guard
            .find(&ids.default_application_group_trust_list_id)
            .is_some());
    }

    #[test]
    fn fails_closed_when_companion_xml_was_never_imported() {
        let address_space = AddressSpace::new();
        assert!(instantiate_certificate_directory(&address_space).is_none());
    }
}
