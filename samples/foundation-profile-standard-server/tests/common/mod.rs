//! Shared harness for the Standard profile benchmark tests.
//! Valid ONLY under an isolated
//! `cargo test -p async-opcua-foundation-profile-standard-server --features profile-tests`.

use std::{env, path::PathBuf, sync::Arc, time::SystemTime};

use async_opcua_foundation_profile_standard_server as standard;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::server::ServerHandle;
use tokio::net::TcpListener;

pub struct StandardTester {
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
        "standard-profile-test-{tag}-{}-{nonce}",
        std::process::id()
    ))
}

pub async fn spawn_standard() -> StandardTester {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let pki_dir = format!("./pki-standard-test-{}", std::process::id());
    let builder = standard::build_server(&pki_dir).discovery_urls(vec![url.clone()]);
    let (server, handle) = builder
        .build()
        .expect("standard benchmark server should build");
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("standard benchmark server exited with error: {e}");
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    StandardTester { handle, url }
}

pub async fn connect(tester: &StandardTester) -> Arc<Session> {
    let mut client = ClientBuilder::new()
        .application_name("standard-profile-smoke-client")
        .application_uri("urn:standard-profile-smoke-client")
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
        .expect("connect to standard endpoint");
    event_loop.spawn();
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        session.wait_for_connection(),
    )
    .await
    .expect("session should activate within 15s");
    session
}
