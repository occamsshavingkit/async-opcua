//! Shared harness for the Embedded profile benchmark tests. Mirrors the micro
//! harness but connects over Basic256Sha256 Sign&Encrypt; see the test-graph
//! unification caveat in specs/054-profile-polish/tasks.md — these tests are
//! valid ONLY under an isolated
//! `cargo test -p async-opcua-foundation-profile-embedded-server --features profile-tests`.

use std::{env, path::PathBuf, sync::Arc, time::SystemTime};

use async_opcua_foundation_profile_embedded_server as embedded;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::server::ServerHandle;
use tokio::net::TcpListener;

pub struct EmbeddedTester {
    #[allow(dead_code)]
    pub handle: ServerHandle,
    pub url: String,
}

fn unique_dir(tag: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!(
        "embedded-profile-test-{tag}-{}-{nonce}",
        std::process::id()
    ))
}

pub async fn spawn_embedded() -> EmbeddedTester {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    // Use a stable relative PKI dir for cert generation (same pattern as the
    // integration test suite). The create_sample_keypair(true) flag regenerates
    // if missing.
    let pki_dir = format!("./pki-embedded-test-{}", std::process::id());
    let builder = embedded::build_server(&pki_dir).discovery_urls(vec![url.clone()]);
    let (server, handle) = builder
        .build()
        .expect("embedded benchmark server should build");
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("embedded benchmark server exited with error: {e}");
        }
    });

    // Give the server a moment to generate its self-signed certificate.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    EmbeddedTester { handle, url }
}

pub async fn connect_secure(tester: &EmbeddedTester) -> Arc<Session> {
    let mut client = ClientBuilder::new()
        .application_name("embedded-profile-smoke-client")
        .application_uri("urn:embedded-profile-smoke-client")
        .pki_dir(unique_dir("pki-client"))
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_limit(1)
        .client()
        .expect("client should build");

    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                tester.url.as_str(),
                opcua::crypto::SecurityPolicy::Basic256Sha256.to_str(),
                opcua::types::MessageSecurityMode::SignAndEncrypt,
            ),
            IdentityToken::Anonymous,
        )
        .await
        .expect("connect to embedded endpoint with Basic256Sha256");
    event_loop.spawn();
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        session.wait_for_connection(),
    )
    .await
    .expect("session should activate within 15s");
    session
}

pub async fn connect_none(tester: &EmbeddedTester) -> Arc<Session> {
    let mut client = ClientBuilder::new()
        .application_name("embedded-profile-smoke-client")
        .application_uri("urn:embedded-profile-smoke-client")
        .pki_dir(unique_dir("pki-client"))
        .create_sample_keypair(false)
        .trust_server_certs(true)
        .session_retry_limit(1)
        .client()
        .expect("client should build");

    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                tester.url.as_str(),
                opcua::crypto::SecurityPolicy::None.to_str(),
                opcua::types::MessageSecurityMode::None,
            ),
            IdentityToken::Anonymous,
        )
        .await
        .expect("connect to embedded endpoint with policy None");
    event_loop.spawn();
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        session.wait_for_connection(),
    )
    .await
    .expect("session should activate within 15s");
    session
}
