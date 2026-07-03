//! Standard 2017 UA Server Profile benchmark.
//!
//! Targets `http://opcfoundation.org/UA-Profile/Server/StandardUA2017`:
//! Embedded + self-registration with an LDS. X509 user tokens and session Cancel
//! are always-compiled core. Capacity CUs (≥50 sessions, ≥500 monitored items)
//! are runtime configuration.
//! This is a footprint benchmark surface, NOT an OPC Foundation conformance claim.

use std::path::PathBuf;

use opcua::crypto::SecurityPolicy;
use opcua::server::{Limits, OperationalLimits, ServerBuilder, SubscriptionLimits};

/// Short profile key used for PKI directories and application URIs.
pub const PROFILE_KEY: &str = "standard";
/// Human-readable benchmark name.
pub const PROFILE_DISPLAY_NAME: &str = "Standard 2017 UA Server Profile benchmark";
/// The OPC Foundation profile URI this benchmark targets (reporting only).
pub const PROFILE_TARGET_URI: &str = "http://opcfoundation.org/UA-Profile/Server/StandardUA2017";
/// One-line description of the served surface.
pub const PROFILE_SURFACE: &str =
    "Embedded surface plus LDS self-registration; X509 user tokens and Cancel in core";

/// Standard capacity: ≥50 sessions, ≥5 subscriptions, ≥500 monitored items.
pub fn profile_limits() -> Limits {
    Limits {
        max_sessions: 50,
        max_inflight_requests_per_connection: 256,
        subscriptions: SubscriptionLimits {
            max_subscriptions_per_session: 5,
            max_pending_publish_requests: 5,
            max_publish_requests_per_subscription: 2,
            max_monitored_items_per_sub: 500,
            max_notifications_per_publish: 500,
            ..Default::default()
        },
        operational: OperationalLimits {
            max_nodes_per_read: 1000,
            max_nodes_per_write: 1000,
            max_nodes_per_browse: 1000,
            max_monitored_items_per_call: 500,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build the Standard benchmark server: policy-None and Basic256Sha256 endpoints,
/// profile-shaped limits, generated type system.
pub fn build_server(pki_dir: impl Into<PathBuf>) -> ServerBuilder {
    let user_token_ids = [opcua::server::ANONYMOUS_USER_TOKEN_ID];

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
                opcua::types::MessageSecurityMode::None,
                &user_token_ids as &[&str],
            ),
        )
        .add_endpoint(
            "basic256sha256_sign_encrypt",
            (
                "/",
                SecurityPolicy::Basic256Sha256,
                opcua::types::MessageSecurityMode::SignAndEncrypt,
                &user_token_ids as &[&str],
            ),
        )
        .create_sample_keypair(true)
        .certificate_path("own/cert.der")
        .private_key_path("private/private.pem")
        .discovery_urls(vec!["/".to_owned()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_targets_expected_uri() {
        assert_eq!(
            PROFILE_TARGET_URI,
            "http://opcfoundation.org/UA-Profile/Server/StandardUA2017"
        );
    }
}
