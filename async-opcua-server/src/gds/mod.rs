//! Global Discovery Server (GDS) support.

#[cfg(feature = "method-call")]
use std::sync::Arc;

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use crate::{node_manager::memory::SimpleNodeManager, ServerHandle};

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use crate::node_manager::memory::CoreNodeManager;

#[cfg(feature = "method-call")]
use self::pull_methods::GdsPullMethodRegistry;

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use self::pull_methods::register_gds_pull_methods;

#[cfg(feature = "method-call")]
use self::push_methods::GdsPushRegistry;

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use self::push_methods::{register_gds_push_methods, register_gds_push_methods_with_handle};

/// Maximum entries retained by each in-memory GDS certificate-management registry.
///
/// On overflow, registries evict their oldest entry before inserting the new one. This keeps
/// sustained authorized GDS traffic from growing registry memory without bound.
pub(crate) const GDS_REGISTRY_CAPACITY: usize = 1024;

/// Filesystem cache for GDS certificate credentials.
pub mod cache;
/// Pull model method callbacks for certificate management.
pub mod pull_methods;
/// Push model method callbacks for certificate signing requests.
pub mod push_methods;

/// Registries backing the standard GDS method callbacks.
#[cfg(feature = "method-call")]
pub struct GdsMethodRegistries {
    /// Registry used by push-model (`ServerConfigurationType`) callbacks.
    pub push_methods: Arc<GdsPushRegistry>,
    /// Registry used by pull-model callbacks.
    pub pull_methods: GdsPullMethodRegistry,
}

/// Registers the standard GDS certificate management callbacks, without shutdown support for
/// the push model's `ResetToServerDefaults` (it will report `Bad_NotSupported`). Prefer
/// [`register_gds_certificate_management_methods_with_handle`] when a [`ServerHandle`] is
/// available.
///
/// `core_node_manager` must be the server's core (namespace 0) node manager -- the push model's
/// `ServerConfiguration` and its methods are standard namespace-0 nodes. `simple_node_manager` is
/// used for the (separately tracked, currently unfixed) pull-model callbacks.
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_certificate_management_methods(
    core_node_manager: &CoreNodeManager,
    simple_node_manager: &SimpleNodeManager,
) -> GdsMethodRegistries {
    GdsMethodRegistries {
        push_methods: register_gds_push_methods(core_node_manager),
        pull_methods: register_gds_pull_methods(simple_node_manager),
    }
}

/// Registers the standard GDS certificate management callbacks, wiring the push model's
/// `ResetToServerDefaults` to actually schedule a shutdown via the supplied [`ServerHandle`].
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_certificate_management_methods_with_handle(
    core_node_manager: &CoreNodeManager,
    simple_node_manager: &SimpleNodeManager,
    handle: ServerHandle,
) -> GdsMethodRegistries {
    GdsMethodRegistries {
        push_methods: register_gds_push_methods_with_handle(core_node_manager, handle),
        pull_methods: register_gds_pull_methods(simple_node_manager),
    }
}
