//! Global Discovery Server (GDS) support.

#[cfg(feature = "method-call")]
use std::sync::Arc;

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use crate::{node_manager::memory::SimpleNodeManager, ServerHandle};

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use crate::node_manager::memory::CoreNodeManager;

#[cfg(all(
    feature = "method-call",
    feature = "generated-address-space",
    feature = "companion-gds"
))]
use self::pull_methods::GdsPullMethodRegistry;

#[cfg(feature = "method-call")]
use self::push_methods::GdsPushRegistry;

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use self::push_methods::{register_gds_push_methods, register_gds_push_methods_with_handle};

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use self::trust_list::{register_trust_list_methods, TrustListMethodHandler};

#[cfg(all(
    feature = "method-call",
    feature = "generated-address-space",
    feature = "companion-gds"
))]
use self::pull_methods::GdsPullMethodHandler;

/// Maximum entries retained by each in-memory GDS certificate-management registry.
///
/// On overflow, registries evict their oldest entry before inserting the new one. This keeps
/// sustained authorized GDS traffic from growing registry memory without bound.
pub(crate) const GDS_REGISTRY_CAPACITY: usize = 1024;

#[cfg(all(feature = "events", feature = "gds", feature = "companion-gds"))]
mod audit;

/// Compatibility re-exports for the hand-authored shared `ApplicationRecordDataType` (Part 12
/// §6.5.5) and its `TypeLoader`. Gated on `companion-gds` to preserve the server API's existing
/// feature boundary while allowing clients to use the canonical definitions from `opcua_types`.
#[cfg(feature = "companion-gds")]
pub mod application_record;
/// Filesystem cache for GDS certificate credentials.
pub mod cache;
/// Instantiates a real `CertificateDirectoryType` object from the GDS companion NodeSet import.
#[cfg(feature = "method-call")]
pub mod directory_instance;
/// Part 4 §Table 120 LIKE-operator matcher, used by the Directory application-registry query
/// methods (`QueryApplications`/`QueryServers`) -- no existing server-side evaluator to reuse.
pub(crate) mod like_match;
/// Pull model method callbacks for certificate management (`CertificateDirectoryType`, Part 12
/// §7.9). Requires the `companion-gds` feature (see [`register_gds_pull_methods_from_companion`])
/// to have any real AddressSpace nodes to dispatch against.
pub mod pull_methods;
/// Push model method callbacks for certificate signing requests.
pub mod push_methods;
/// TrustList method callbacks for the three standard CertificateGroups.
pub mod trust_list;

/// Registries backing the standard GDS Push-model method callbacks.
#[cfg(feature = "method-call")]
pub struct GdsMethodRegistries {
    /// Registry used by push-model (`ServerConfigurationType`) callbacks.
    pub push_methods: Arc<GdsPushRegistry>,
    /// Application-group handler retained for API compatibility; registration also wires the
    /// HTTPS and user-token group handlers.
    #[cfg(feature = "generated-address-space")]
    pub trust_list: Arc<TrustListMethodHandler>,
}

/// Registers the standard GDS Push-model certificate management callbacks, without shutdown
/// support for `ResetToServerDefaults` (it will report `Bad_NotSupported`). Prefer
/// [`register_gds_certificate_management_methods_with_handle`] when a [`ServerHandle`] is
/// available.
///
/// `core_node_manager` must be the server's core (namespace 0) node manager -- the Push model's
/// `ServerConfiguration` and its methods are standard namespace-0 nodes. `simple_node_manager` is
/// no longer used here (the Pull-model methods that used to be registered on it were broken --
/// see [`register_gds_pull_methods_from_companion`] for the real Pull-model surface) but remains
/// a parameter for source compatibility with existing callers.
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_certificate_management_methods(
    core_node_manager: &CoreNodeManager,
    _simple_node_manager: &SimpleNodeManager,
) -> GdsMethodRegistries {
    let push_methods = register_gds_push_methods(core_node_manager);
    GdsMethodRegistries {
        trust_list: register_trust_list_methods(core_node_manager, push_methods.clone()),
        push_methods,
    }
}

/// Registers the standard GDS Push-model certificate management callbacks, wiring
/// `ResetToServerDefaults` to actually schedule a shutdown via the supplied [`ServerHandle`].
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_certificate_management_methods_with_handle(
    core_node_manager: &CoreNodeManager,
    _simple_node_manager: &SimpleNodeManager,
    handle: ServerHandle,
) -> GdsMethodRegistries {
    let push_methods = register_gds_push_methods_with_handle(core_node_manager, handle);
    GdsMethodRegistries {
        trust_list: register_trust_list_methods(core_node_manager, push_methods.clone()),
        push_methods,
    }
}

/// Imports the GDS companion NodeSet (if `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` is
/// present locally -- see `schemas/companion/README.md`), instantiates a real
/// `CertificateDirectoryType` "Directory" object, and registers the Pull-model method callbacks
/// (Part 12 §7.9) against it on `core_node_manager`.
///
/// This is an explicit, opt-in call (not wired into any `ServerBuilder` default), matching this
/// project's existing companion-spec operator model. Returns `None` (logging a warning, never
/// panicking) if the companion NodeSet wasn't actually importable -- the server otherwise
/// continues exactly as if this function had never been called.
///
/// `server_handle` must belong to the same server as `core_node_manager`. Call this during server
/// startup, before [`crate::Server::run`] begins serving concurrent requests, so the imported type
/// metadata can replace the startup snapshot used by subscription event filters.
#[cfg(all(
    feature = "method-call",
    feature = "generated-address-space",
    feature = "companion-gds"
))]
pub fn register_gds_pull_methods_from_companion(
    core_node_manager: &CoreNodeManager,
    server_handle: &ServerHandle,
) -> Option<Arc<GdsPullMethodHandler>> {
    let Some(handle_core_node_manager) = server_handle
        .node_managers()
        .get_of_type::<CoreNodeManager>()
    else {
        tracing::warn!(
            "cannot register GDS companion methods: server handle has no CoreNodeManager"
        );
        return None;
    };
    if !Arc::ptr_eq(
        core_node_manager.address_space(),
        handle_core_node_manager.address_space(),
    ) {
        tracing::warn!(
            "cannot register GDS companion methods: server handle belongs to a different server"
        );
        return None;
    }

    // The address space's own namespace table (what the companion import's "next free index"
    // allocation consults) and the type tree's namespace table (e.g. populated by
    // `DiagnosticsNodeManager` registering the server's own application namespace at startup) are
    // two independently-maintained tables that can disagree about which indices are taken --
    // reconcile before importing, so the companion import can't allocate an index (e.g. 1) the
    // type tree already claims for something else, which would otherwise make
    // `Server_NamespaceArray` silently misreport one of the two colliding namespaces to clients.
    {
        let type_tree = server_handle.type_tree().read();
        let address_space = core_node_manager.address_space().read();
        let existing = address_space.namespaces();
        for (uri, index) in type_tree.namespaces().known_namespaces() {
            if let Some(existing_uri) = existing.get(index) {
                if existing_uri != uri {
                    tracing::warn!(
                        index,
                        address_space_uri = %existing_uri,
                        type_tree_uri = %uri,
                        "cannot register GDS companion methods with conflicting namespace indices"
                    );
                    return None;
                }
                continue;
            }
            if let Some((existing_index, _)) = existing
                .iter()
                .find(|(_, existing_uri)| *existing_uri == uri)
            {
                tracing::warn!(
                    address_space_index = existing_index,
                    type_tree_index = index,
                    namespace_uri = %uri,
                    "cannot register GDS companion methods with conflicting namespace indices"
                );
                return None;
            }
        }
        for (uri, index) in type_tree.namespaces().known_namespaces() {
            if !existing.contains_key(index) {
                address_space.add_namespace(uri, *index);
            }
        }
    }

    crate::companion::import_gds(core_node_manager.address_space());
    // `import_gds` adds the GDS namespace to the shared address space, but
    // `CoreNodeManager`'s namespace cache (used by `owns_node`, and therefore Call/Read/Write/
    // Browse dispatch) was snapshotted at construction time, before this import ran -- refresh it
    // so the newly-instantiated Directory object's namespace is recognized as owned.
    core_node_manager.refresh_namespaces();

    {
        let mut type_tree = server_handle.type_tree().write();
        let address_space = core_node_manager.address_space().read();
        let mut namespaces = type_tree.namespaces().known_namespaces().clone();
        namespaces.extend(
            address_space
                .namespaces()
                .into_iter()
                .map(|(index, uri)| (uri, index)),
        );
        *type_tree.namespaces_mut() = opcua_types::NamespaceMap::new_full(namespaces);
        address_space.load_into_type_tree(&mut type_tree);
        server_handle.info().publish_type_tree_snapshot(&type_tree);
    }

    let directory = {
        let address_space = core_node_manager.address_space().read();
        directory_instance::instantiate_certificate_directory(&address_space)?
    };

    let registry = GdsPullMethodRegistry::default();
    let handler = Arc::new(GdsPullMethodHandler::new(registry, directory));
    pull_methods::register_pull_method_callbacks(core_node_manager, handler.clone());

    Some(handler)
}
