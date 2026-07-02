//! Nano Embedded Device 2017 Server Profile benchmark.
//!
//! Targets `http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2017`:
//! UA-TCP binary transport, SecurityPolicy None, sessions, read, and view services.
//! This is a footprint benchmark surface, NOT an OPC Foundation conformance claim.

use std::path::PathBuf;

use opcua::crypto::SecurityPolicy;
use opcua::server::{
    Limits, OperationalLimits, ServerBuilder, SubscriptionLimits, ANONYMOUS_USER_TOKEN_ID,
};
use opcua::types::MessageSecurityMode;

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

/// Build the Nano benchmark server: policy-None endpoint, anonymous identity,
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
