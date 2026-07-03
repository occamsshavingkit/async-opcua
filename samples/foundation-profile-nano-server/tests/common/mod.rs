//! Shared harness for the Nano profile benchmark tests.
//!
//! Spawns the benchmark server in-process on an ephemeral port and connects the
//! in-tree client over the policy-None endpoint.
//!
//! Test-graph unification caveat (specs/054-profile-polish/tasks.md): these tests
//! verify BEHAVIOR of the nano feature surface. Binary-absence claims are verified
//! separately by `tools/check-profile-absence.sh` against `cargo build` artifacts,
//! which ignore dev-dependencies.

use std::{env, path::PathBuf, sync::Arc, time::SystemTime};

use async_opcua_foundation_profile_nano_server as nano;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::server::ServerHandle;
use tokio::net::TcpListener;

pub struct NanoTester {
    /// Keeps the server alive for the duration of the test.
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
        "nano-profile-test-{tag}-{}-{nonce}",
        std::process::id()
    ))
}

/// Spawn the benchmark server exactly as the sample builds it.
pub async fn spawn_nano() -> NanoTester {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let builder = nano::build_server(unique_dir("pki-server")).discovery_urls(vec![url.clone()]);
    let (server, handle) = builder.build().expect("nano benchmark server should build");
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("nano benchmark server exited with error: {e}");
        }
    });

    NanoTester { handle, url }
}

/// Connect a session over SecurityPolicy None / MessageSecurityMode None.
pub async fn connect(tester: &NanoTester, token: IdentityToken) -> Arc<Session> {
    let mut client = ClientBuilder::new()
        .application_name("nano-profile-smoke-client")
        .application_uri("urn:nano-profile-smoke-client")
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
            token,
        )
        .await
        .expect("connect to nano endpoint");
    event_loop.spawn();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        session.wait_for_connection(),
    )
    .await
    .expect("session should activate within 10s");
    session
}
