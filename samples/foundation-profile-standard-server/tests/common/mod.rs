//! Shared harness for the Standard profile benchmark tests.
//! Valid ONLY under an isolated
//! `cargo test -p async-opcua-foundation-profile-standard-server --features profile-tests`.

use std::{
    env,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::SystemTime,
};

use async_opcua_foundation_profile_standard_server as standard;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::server::ServerHandle;
use tokio::net::TcpListener;

static SPAWN_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let count = SPAWN_COUNTER.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "standard-profile-test-{tag}-{}-{nonce}-{count}",
        std::process::id()
    ))
}

pub async fn spawn_standard() -> StandardTester {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let pki_dir = unique_dir("pki-server");
    let builder = standard::build_server(pki_dir).discovery_urls(vec![url.clone()]);
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

pub struct LdsPeer {
    pub url: String,
    pub handle: ServerHandle,
}

impl Drop for LdsPeer {
    fn drop(&mut self) {
        self.handle.cancel();
    }
}

pub async fn spawn_lds_peer() -> LdsPeer {
    use opcua::crypto::SecurityPolicy;
    use opcua::types::MessageSecurityMode;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind LDS ephemeral port");
    let addr = listener.local_addr().expect("LDS local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let user_token_ids = &[opcua::server::ANONYMOUS_USER_TOKEN_ID] as &[&str];

    let builder = opcua::server::ServerBuilder::new()
        .application_name("LDS Peer")
        .application_uri("urn:async-opcua:lds-peer")
        .product_uri("https://github.com/freeopcua/async-opcua")
        .pki_dir(unique_dir("pki-lds"))
        .create_sample_keypair(true)
        .certificate_path("own/cert.der")
        .private_key_path("private/private.pem")
        .discovery_urls(vec![url.clone()])
        .trust_client_certs(true)
        .add_endpoint(
            "none",
            (
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                user_token_ids,
            ),
        )
        .add_endpoint(
            "basic256sha256_sign_encrypt",
            (
                "/",
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt,
                user_token_ids,
            ),
        );

    let (server, handle) = builder.build().expect("LDS server should build");
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("LDS peer exited with error: {e}");
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    LdsPeer { url, handle }
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

#[allow(dead_code)]
pub async fn spawn_standard_with_lds_registration(lds_url: &str) -> StandardTester {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let pki_dir = unique_dir("pki-server");
    let builder = standard::build_server(pki_dir)
        .discovery_urls(vec![url.clone()])
        .discovery_server_url(lds_url);
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

/// Spawns a standard profile server with X509 user token support.
/// Builds the server from scratch (not via build_server()) to avoid
/// endpoint-path duplication with different user token ID sets.
#[allow(dead_code)]
pub async fn spawn_standard_with_x509() -> StandardTester {
    use opcua::crypto::SecurityPolicy;
    use opcua::server::ServerUserToken;
    use opcua::types::MessageSecurityMode;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let pki_dir = unique_dir("pki-server");

    let user_token_ids: &[&str] = &[opcua::server::ANONYMOUS_USER_TOKEN_ID, "x509"];

    let x509_cert_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../async-opcua/tests/x509/user_cert.der");

    let builder = opcua::server::ServerBuilder::new()
        .application_name("async-opcua Standard 2017 UA Server (x509 test)")
        .application_uri("urn:async-opcua:foundation-profile-benchmark:standard")
        .product_uri("https://github.com/freeopcua/async-opcua")
        .pki_dir(pki_dir)
        .limits(standard::profile_limits())
        .create_sample_keypair(true)
        .certificate_path("own/cert.der")
        .private_key_path("private/private.pem")
        .trust_client_certs(true)
        .add_user_token("x509", ServerUserToken::x509("x509", &x509_cert_path))
        .discovery_urls(vec![url.clone()])
        .add_endpoint(
            "none",
            (
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                user_token_ids,
            ),
        )
        .add_endpoint(
            "basic256sha256_sign_encrypt",
            (
                "/",
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt,
                user_token_ids,
            ),
        );

    let (server, handle) = builder
        .build()
        .expect("standard benchmark server with x509 should build");
    tokio::task::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            eprintln!("standard benchmark server exited with error: {e}");
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    StandardTester { handle, url }
}
