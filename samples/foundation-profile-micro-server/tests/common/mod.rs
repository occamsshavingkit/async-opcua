//! Shared harness for the Micro profile benchmark tests. Mirrors the nano harness;
//! see the test-graph unification caveat in specs/054-profile-polish/tasks.md —
//! these tests are valid ONLY under an isolated
//! `cargo test -p async-opcua-foundation-profile-micro-server --features profile-tests`.

use std::{env, path::PathBuf, sync::Arc, time::SystemTime};

use async_opcua_foundation_profile_micro_server as micro;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::server::ServerHandle;
use tokio::net::TcpListener;

pub struct MicroTester {
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
        "micro-profile-test-{tag}-{}-{nonce}",
        std::process::id()
    ))
}

pub async fn spawn_micro() -> MicroTester {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let builder = micro::build_server(unique_dir("pki-server")).discovery_urls(vec![url.clone()]);
    let (server, handle) = builder
        .build()
        .expect("micro benchmark server should build");
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("micro benchmark server exited with error: {e}");
        }
    });

    MicroTester { handle, url }
}

pub async fn connect(tester: &MicroTester) -> Arc<Session> {
    let mut client = ClientBuilder::new()
        .application_name("micro-profile-smoke-client")
        .application_uri("urn:micro-profile-smoke-client")
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
        .expect("connect to micro endpoint");
    event_loop.spawn();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        session.wait_for_connection(),
    )
    .await
    .expect("session should activate within 10s");
    session
}
