//! Micro Embedded Device 2017 Server Profile benchmark.
//!
//! Targets `http://opcfoundation.org/UA-Profile/Server/MicroEmbeddedDevice2017`:
//! the Nano surface plus basic data-change subscriptions (Embedded DataChange
//! Subscription Server Facet) and at least two parallel sessions.
//! This is a footprint benchmark surface, NOT an OPC Foundation conformance claim.

use std::path::PathBuf;

use opcua::crypto::SecurityPolicy;
use opcua::server::{
    Limits, OperationalLimits, ServerBuilder, SubscriptionLimits, ANONYMOUS_USER_TOKEN_ID,
};
use opcua::types::{MessageSecurityMode, NodeId};

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
/// subscriptions (Monitor Value Change CU). Served from the sample's demo
/// namespace; wired up in the profile rework (T026).
pub fn demo_counter_node_id() -> NodeId {
    NodeId::new(1, "demo_counter")
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

/// Build the Micro benchmark server: policy-None endpoint, anonymous identity,
/// profile-shaped limits.
pub fn build_server(pki_dir: impl Into<PathBuf>) -> ServerBuilder {
    let user_token_ids = [ANONYMOUS_USER_TOKEN_ID];

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
