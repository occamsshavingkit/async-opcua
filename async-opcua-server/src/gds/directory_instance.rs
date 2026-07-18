//! Resolves the real `CertificateDirectoryType` "Directory" object (OPC UA Part 12 §7.9.2) that
//! the GDS companion NodeSet ships as a pre-instantiated singleton (NodeId `ns=1;i=141` in the
//! source XML, remapped only in namespace by import -- verified against the real
//! `Opc.Ua.Gds.NodeSet2.xml`; see `specs/104-gds-pull-directory-fix/research.md`).
//!
//! An earlier version of this module (feature 103) incorrectly concluded no such singleton
//! existed and hand-built a parallel "Directory" object with fabricated string NodeIds. That was
//! wrong and has been corrected: the real object already carries every Mandatory method and the
//! `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree this Pull-model needs, wired
//! via `HasComponent`/`Organizes` references the plain 1:1 NodeSet import already produces --
//! nothing needs to be constructed here, only resolved and verified present.

use tracing::warn;

use opcua_types::NodeId;

use crate::address_space::AddressSpace;

/// The GDS companion namespace URI (declared in the NodeSet2.xml's own `<NamespaceUris>`).
pub(crate) const GDS_NAMESPACE_URI: &str = "http://opcfoundation.org/UA/GDS/";

/// `CertificateDirectoryType`'s identifier in the GDS companion NodeSet2.xml. Namespace-index
/// remapping during import preserves this identifier unchanged (verified: `NodeSetNamespaceMapper`
/// only ever remaps the namespace portion of a NodeId -- see research.md).
const CERTIFICATE_DIRECTORY_TYPE_ID: u32 = 63;

/// The real, companion-shipped "Directory" object instance's identifier (source XML `ns=1;i=141`).
const DIRECTORY_OBJECT_ID: u32 = 141;
/// Real instance method identifiers on the Directory object (Mandatory, Part 12 Table 74).
const START_SIGNING_REQUEST_ID: u32 = 157;
const START_NEW_KEY_PAIR_REQUEST_ID: u32 = 154;
const FINISH_REQUEST_ID: u32 = 163;
const GET_CERTIFICATE_GROUPS_ID: u32 = 508;
const GET_TRUST_LIST_ID: u32 = 204;
const GET_CERTIFICATE_STATUS_ID: u32 = 225;
/// Real `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree identifiers (§7.8).
const DEFAULT_APPLICATION_GROUP_ID: u32 = 615;
const DEFAULT_APPLICATION_GROUP_TRUST_LIST_ID: u32 = 616;
/// Real `DirectoryType`-inherited application-registry method/folder identifiers (feature 108,
/// Part 12 §6.5.4/§6.5.6-§6.5.11), re-verified directly against the real
/// `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` -- see `specs/108-gds-directory-app-registry/
/// research.md` R1.
const REGISTER_APPLICATION_ID: u32 = 146;
const QUERY_SERVERS_ID: u32 = 151;
const QUERY_APPLICATIONS_ID: u32 = 992;
const FIND_APPLICATIONS_ID: u32 = 143;
const UPDATE_APPLICATION_ID: u32 = 200;
const UNREGISTER_APPLICATION_ID: u32 = 149;
const GET_APPLICATION_ID: u32 = 216;
const APPLICATIONS_FOLDER_ID: u32 = 142;

/// Real NodeIds of the Directory object and its methods/subtree, resolved once at server-startup
/// wiring time.
#[derive(Debug, Clone)]
pub struct DirectoryInstanceNodeIds {
    /// The real, companion-shipped `CertificateDirectoryType` "Directory" object.
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
    /// The real `DefaultApplicationGroup` object under the Directory's `CertificateGroups`.
    pub default_application_group_id: NodeId,
    /// The real TrustList object under `DefaultApplicationGroup` -- what `GetTrustList` returns.
    pub default_application_group_trust_list_id: NodeId,
    /// Instance method NodeId (feature 108).
    pub register_application_id: NodeId,
    /// Instance method NodeId (feature 108, deprecated per Part 12 §6.5.11).
    pub query_servers_id: NodeId,
    /// Instance method NodeId (feature 108).
    pub query_applications_id: NodeId,
    /// Instance method NodeId (feature 108).
    pub find_applications_id: NodeId,
    /// Instance method NodeId (feature 108).
    pub update_application_id: NodeId,
    /// Instance method NodeId (feature 108).
    pub unregister_application_id: NodeId,
    /// Instance method NodeId (feature 108).
    pub get_application_id: NodeId,
    /// The real `Applications` folder under the Directory object (feature 108).
    pub applications_folder_id: NodeId,
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

/// Resolves the real `CertificateDirectoryType` "Directory" object and its Mandatory
/// method/subtree NodeIds in `address_space`, assuming the GDS companion NodeSet has already been
/// imported into it (via `companion::import_gds`). Fails closed (returns `None`, logs a warning)
/// if any expected node isn't actually present -- never panics.
pub fn instantiate_certificate_directory(
    address_space: &AddressSpace,
) -> Option<DirectoryInstanceNodeIds> {
    let Some(gds_ns) = resolve_gds_namespace(address_space) else {
        warn!(
            "GDS companion NodeSet not imported (namespace '{}' not found) -- \
             Pull-model Certificate Directory will not be wired",
            GDS_NAMESPACE_URI
        );
        return None;
    };

    let certificate_directory_type_id = NodeId::new(gds_ns, CERTIFICATE_DIRECTORY_TYPE_ID);
    if address_space.find(&certificate_directory_type_id).is_none() {
        warn!(
            "CertificateDirectoryType ({certificate_directory_type_id}) not found in the \
             imported GDS companion NodeSet -- Pull-model Certificate Directory will not be wired"
        );
        return None;
    }

    let ids = DirectoryInstanceNodeIds {
        directory_object_id: NodeId::new(gds_ns, DIRECTORY_OBJECT_ID),
        start_signing_request_id: NodeId::new(gds_ns, START_SIGNING_REQUEST_ID),
        start_new_key_pair_request_id: NodeId::new(gds_ns, START_NEW_KEY_PAIR_REQUEST_ID),
        finish_request_id: NodeId::new(gds_ns, FINISH_REQUEST_ID),
        get_certificate_groups_id: NodeId::new(gds_ns, GET_CERTIFICATE_GROUPS_ID),
        get_trust_list_id: NodeId::new(gds_ns, GET_TRUST_LIST_ID),
        get_certificate_status_id: NodeId::new(gds_ns, GET_CERTIFICATE_STATUS_ID),
        default_application_group_id: NodeId::new(gds_ns, DEFAULT_APPLICATION_GROUP_ID),
        default_application_group_trust_list_id: NodeId::new(
            gds_ns,
            DEFAULT_APPLICATION_GROUP_TRUST_LIST_ID,
        ),
        register_application_id: NodeId::new(gds_ns, REGISTER_APPLICATION_ID),
        query_servers_id: NodeId::new(gds_ns, QUERY_SERVERS_ID),
        query_applications_id: NodeId::new(gds_ns, QUERY_APPLICATIONS_ID),
        find_applications_id: NodeId::new(gds_ns, FIND_APPLICATIONS_ID),
        update_application_id: NodeId::new(gds_ns, UPDATE_APPLICATION_ID),
        unregister_application_id: NodeId::new(gds_ns, UNREGISTER_APPLICATION_ID),
        get_application_id: NodeId::new(gds_ns, GET_APPLICATION_ID),
        applications_folder_id: NodeId::new(gds_ns, APPLICATIONS_FOLDER_ID),
    };

    for id in [
        &ids.directory_object_id,
        &ids.start_signing_request_id,
        &ids.start_new_key_pair_request_id,
        &ids.finish_request_id,
        &ids.get_certificate_groups_id,
        &ids.get_trust_list_id,
        &ids.get_certificate_status_id,
        &ids.default_application_group_id,
        &ids.default_application_group_trust_list_id,
        &ids.register_application_id,
        &ids.query_servers_id,
        &ids.query_applications_id,
        &ids.find_applications_id,
        &ids.update_application_id,
        &ids.unregister_application_id,
        &ids.get_application_id,
        &ids.applications_folder_id,
    ] {
        if address_space.find(id).is_none() {
            warn!(
                "Expected node {id} not found in the imported GDS companion NodeSet -- \
                 Pull-model Certificate Directory will not be wired"
            );
            return None;
        }
    }

    Some(ids)
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
    fn resolves_the_real_directory_object_when_companion_xml_is_present() {
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
        let ids = ids.expect("companion XML is present, resolution should succeed");

        let guard = rw.read();
        let gds_ns = resolve_gds_namespace(&guard).expect("gds namespace should resolve");

        // Resolves the real, companion-shipped object -- not a hand-built stand-in.
        assert_eq!(ids.directory_object_id, NodeId::new(gds_ns, 141u32));
        assert_eq!(ids.start_signing_request_id, NodeId::new(gds_ns, 157u32));
        assert_eq!(
            ids.start_new_key_pair_request_id,
            NodeId::new(gds_ns, 154u32)
        );
        assert_eq!(ids.finish_request_id, NodeId::new(gds_ns, 163u32));
        assert_eq!(ids.get_certificate_groups_id, NodeId::new(gds_ns, 508u32));
        assert_eq!(ids.get_trust_list_id, NodeId::new(gds_ns, 204u32));
        assert_eq!(ids.get_certificate_status_id, NodeId::new(gds_ns, 225u32));
        assert_eq!(
            ids.default_application_group_id,
            NodeId::new(gds_ns, 615u32)
        );
        assert_eq!(
            ids.default_application_group_trust_list_id,
            NodeId::new(gds_ns, 616u32)
        );
        assert_eq!(ids.register_application_id, NodeId::new(gds_ns, 146u32));
        assert_eq!(ids.query_servers_id, NodeId::new(gds_ns, 151u32));
        assert_eq!(ids.query_applications_id, NodeId::new(gds_ns, 992u32));
        assert_eq!(ids.find_applications_id, NodeId::new(gds_ns, 143u32));
        assert_eq!(ids.update_application_id, NodeId::new(gds_ns, 200u32));
        assert_eq!(ids.unregister_application_id, NodeId::new(gds_ns, 149u32));
        assert_eq!(ids.get_application_id, NodeId::new(gds_ns, 216u32));
        assert_eq!(ids.applications_folder_id, NodeId::new(gds_ns, 142u32));

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
        assert!(guard.find(&ids.register_application_id).is_some());
        assert!(guard.find(&ids.query_servers_id).is_some());
        assert!(guard.find(&ids.query_applications_id).is_some());
        assert!(guard.find(&ids.find_applications_id).is_some());
        assert!(guard.find(&ids.update_application_id).is_some());
        assert!(guard.find(&ids.unregister_application_id).is_some());
        assert!(guard.find(&ids.get_application_id).is_some());
        assert!(guard.find(&ids.applications_folder_id).is_some());
    }

    /// Regression guard for the exact bug this feature fixes: feature 103 built a *second*,
    /// hand-constructed "Directory" object at `NodeId::new(gds_ns, "Directory")` alongside the
    /// real one at `NodeId::new(gds_ns, 141)`. Confirms only the real object exists now -- no
    /// code path in this module constructs a node at the old fabricated string identifier.
    #[test]
    fn does_not_construct_a_duplicate_directory_object() {
        if !xml_present() {
            eprintln!(
                "skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally"
            );
            return;
        }

        let address_space = AddressSpace::new();
        let rw = parking_lot::RwLock::new(address_space);
        crate::companion::import_gds(&rw);

        {
            let guard = rw.read();
            instantiate_certificate_directory(&guard)
                .expect("companion XML is present, resolution should succeed");
        }

        let guard = rw.read();
        let gds_ns = resolve_gds_namespace(&guard).expect("gds namespace should resolve");

        assert!(
            guard.find(&NodeId::new(gds_ns, "Directory")).is_none(),
            "no node should exist at the old fabricated string-identifier NodeId this feature \
             removed the construction of"
        );
        assert!(guard.find(&NodeId::new(gds_ns, 141u32)).is_some());
    }

    #[test]
    fn fails_closed_when_companion_xml_was_never_imported() {
        let address_space = AddressSpace::new();
        assert!(instantiate_certificate_directory(&address_space).is_none());
    }
}
