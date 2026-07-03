//! Embedded 2017 UA Server Profile benchmark.
//!
//! Targets `http://opcfoundation.org/UA-Profile/Server/EmbeddedUA2017`:
//! Micro + standard data-change tier (deadband, triggering),
//! GetMonitoredItems/ResendData (method call), served type system, real security.
//! This is a footprint benchmark surface, NOT an OPC Foundation conformance claim.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
};

use opcua::crypto::SecurityPolicy;
use opcua::nodes::{ImportedItem, ImportedReference, NodeSetImport, NodeSetNamespaceMapper};
use opcua::server::{
    address_space::{AddressSpace, Variable},
    node_manager::{
        memory::{
            InMemoryNodeManagerBuilder, InMemoryNodeManagerImplBuilder, SimpleNodeManagerBuilder,
            SimpleNodeManagerImpl,
        },
        ServerContext,
    },
    Limits, OperationalLimits, ServerBuilder, SubscriptionLimits, ANONYMOUS_USER_TOKEN_ID,
};
use opcua::types::{DataValue, MessageSecurityMode, NodeId};
/// Short profile key used for PKI directories and application URIs.
pub const PROFILE_KEY: &str = "embedded";
/// Human-readable benchmark name.
pub const PROFILE_DISPLAY_NAME: &str = "Embedded 2017 UA Server Profile benchmark";
/// The OPC Foundation profile URI this benchmark targets (reporting only).
pub const PROFILE_TARGET_URI: &str = "http://opcfoundation.org/UA-Profile/Server/EmbeddedUA2017";
/// One-line description of the served surface.
pub const PROFILE_SURFACE: &str =
    "Micro surface plus Basic256Sha256 secure endpoints, standard data-change tier, method call";

/// NodeId of the demo counter variable the sample ticks for data-change
/// subscriptions with deadband filtering. Lives in the benchmark's demo namespace.
/// The actual namespace index is resolved at import time; this constant is the
/// in-nodeset index used by `register_namespaces`.
const DEMO_NS_IN_NODESET: u16 = 2;

/// After import, the server assigns a namespace index. We store it here so the
/// test can discover the real NodeId.
use std::sync::OnceLock;
static DEMO_NS_SERVER_INDEX: OnceLock<u16> = OnceLock::new();

pub fn demo_counter_node_id() -> NodeId {
    let ns = *DEMO_NS_SERVER_INDEX.get().unwrap_or(&DEMO_NS_IN_NODESET);
    NodeId::new(ns, "demo_counter")
}

pub fn demo_trigger_node_id() -> NodeId {
    let ns = *DEMO_NS_SERVER_INDEX.get().unwrap_or(&DEMO_NS_IN_NODESET);
    NodeId::new(ns, "demo_trigger_target")
}

/// Embedded capacity: ≥2 subscriptions, ≥100 monitored items.
pub fn profile_limits() -> Limits {
    Limits {
        max_sessions: 4,
        max_inflight_requests_per_connection: 64,
        subscriptions: SubscriptionLimits {
            max_subscriptions_per_session: 4,
            max_pending_publish_requests: 4,
            max_publish_requests_per_subscription: 2,
            max_monitored_items_per_sub: 100,
            max_notifications_per_publish: 100,
            ..Default::default()
        },
        operational: OperationalLimits {
            max_nodes_per_read: 256,
            max_nodes_per_write: 256,
            max_nodes_per_browse: 256,
            max_monitored_items_per_call: 100,
            ..Default::default()
        },
        ..Default::default()
    }
}

struct EmbeddedDemoNamespace;

const DEMO_NAMESPACE_URI: &str = "urn:async-opcua:embedded-demo";

impl NodeSetImport for EmbeddedDemoNamespace {
    fn register_namespaces(&self, namespaces: &mut NodeSetNamespaceMapper) {
        namespaces.add_namespace(DEMO_NAMESPACE_URI, DEMO_NS_IN_NODESET);
    }

    fn get_own_namespaces(&self) -> Vec<String> {
        vec![DEMO_NAMESPACE_URI.to_owned()]
    }

    fn load<'a>(
        &'a self,
        namespaces: &'a NodeSetNamespaceMapper,
    ) -> Box<dyn Iterator<Item = ImportedItem> + 'a> {
        // Resolve the server-side namespace index.
        let server_ns = namespaces
            .get_index(DEMO_NS_IN_NODESET)
            .unwrap_or(DEMO_NS_IN_NODESET);
        let _ = DEMO_NS_SERVER_INDEX.set(server_ns);
        Box::new(demo_nodes(server_ns).into_iter())
    }
}

fn demo_nodes(ns: u16) -> Vec<ImportedItem> {
    let counter_id = NodeId::new(ns, "demo_counter");
    let trigger_id = NodeId::new(ns, "demo_trigger_target");
    let objects = NodeId::objects_folder_id();

    vec![
        ImportedItem {
            node: Variable::new(&counter_id, "demo_counter", "demo_counter", 0i32).into(),
            references: vec![ImportedReference {
                target_id: objects.clone(),
                type_id: opcua::types::ReferenceTypeId::HasComponent.into(),
                is_forward: false,
            }],
        },
        ImportedItem {
            node: Variable::new(
                &trigger_id,
                "demo_trigger_target",
                "demo_trigger_target",
                0i32,
            )
            .into(),
            references: vec![ImportedReference {
                target_id: objects,
                type_id: opcua::types::ReferenceTypeId::HasComponent.into(),
                is_forward: false,
            }],
        },
    ]
}

/// Build the Embedded benchmark server: policy-None and Basic256Sha256 endpoints,
/// profile-shaped limits, generated type system, and a ticking demo counter
/// for deadband / triggering tests.
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
        .add_endpoint(
            "basic256sha256_sign",
            (
                "/",
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::Sign,
                &user_token_ids as &[&str],
            ),
        )
        .add_endpoint(
            "basic256sha256_sign_encrypt",
            (
                "/",
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt,
                &user_token_ids as &[&str],
            ),
        )
        .create_sample_keypair(true)
        .certificate_path("own/cert.der")
        .private_key_path("private/private.pem")
        .discovery_urls(vec!["/".to_owned()])
        .with_node_manager(InMemoryNodeManagerBuilder::new(
            move |context: ServerContext,
                  address_space: &mut AddressSpace|
                  -> SimpleNodeManagerImpl {
                let builder = SimpleNodeManagerBuilder::new_imports(
                    vec![Box::new(EmbeddedDemoNamespace)],
                    "embedded-demo",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_targets_expected_uri() {
        assert_eq!(
            PROFILE_TARGET_URI,
            "http://opcfoundation.org/UA-Profile/Server/EmbeddedUA2017"
        );
    }
}
