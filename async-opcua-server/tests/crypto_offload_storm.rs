//! T010 [US1] `#[ignore]`'d 50-client handshake-storm test (SC-001).
//!
//! Verifies that an already-established session's read latency stays bounded
//! when >= 50 concurrent clients open secure channels against the same server.
//! The OSC (OpenSecureChannel) handshake is the most CPU-intensive pre-auth
//! operation: it requires RSA-2048 asymmetric decrypt+verify and sign+encrypt.
//! Without the offload (T005-T008 + T010A), this crypto would run on
//! the request-processing thread and starve established sessions under load.
//! T010A moved the crypto from the shared `spawn_blocking` pool to a
//! dedicated lower-priority executor (`CryptoExecutor`), so handshake RSA
//! runs at reduced OS scheduling priority relative to established-session
//! reads.
//!
//! The test measures the established session's p99 read latency under two
//! conditions:
//!   1. Baseline: no concurrent handshakes.
//!   2. Storm: >= 50 concurrent channel-opening clients.
//!
//! Assertion: storm p99 <= 2 * baseline p99 (a fixed relative bound, not an
//! absolute latency, so it cannot scale with handshake count and does not
//! depend on machine speed).
//!
//! Manual only - run pinned to a single core per repo benchmarking convention:
//!
//! ```text
//! taskset -c <core> cargo test -p async-opcua-server --release \
//!   --test crypto_offload_storm -- --ignored --nocapture \
//!   handshake_storm_established_session_latency
//! ```
//!
//! Grounding: OPC-10000-4 5.6.2 (OpenSecureChannel Service);
//!            OPC-10000-6 6.7.2 (message security);
//!            SC-001 (established-session latency under handshake storm).

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use opcua_client::{
    transport::{DefaultTransport, SecureChannelEventLoop, TransportPollResult},
    ClientBuilder, IdentityToken, Session,
};
use opcua_crypto::SecurityPolicy;
use opcua_server::{ServerBuilder, ANONYMOUS_USER_TOKEN_ID};
use opcua_types::{
    AttributeId, EndpointDescription, MessageSecurityMode, ReadValueId, StatusCode,
    TimestampsToReturn, UAString, VariableId,
};
use tokio::net::TcpListener;

/// Number of concurrent channel-opening clients in the storm.
/// The spec (SC-001) requires >= 50.
const STORM_CLIENTS: usize = 50;

/// Number of read samples collected for each measurement phase.
/// A larger sample count reduces p99 volatility from scheduling jitter.
const BASELINE_SAMPLES: usize = 200;
const STORM_SAMPLES: usize = 200;

/// Overall test timeout.
const TEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Allowed latency regression: storm p99 must be at most this factor times
/// the baseline p99.
const MAX_REGRESSION_FACTOR: f64 = 2.0;

static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

/// SC-001: Under a >= 50-client handshake storm, an established session's
/// p99 read latency must not exceed 2x its baseline p99 (no concurrent
/// handshakes).
///
/// A manually-built runtime is used (instead of `#[tokio::test]`) to cap
/// `max_blocking_threads`. With T010A the server-side crypto offload runs on
/// the dedicated `CryptoExecutor` (2 workers, lower priority) rather than the
/// shared spawn_blocking pool, so the `max_blocking_threads(4)` cap affects
/// only non-crypto spawn_blocking calls. The dedicated executor's bounded
/// queue (depth 16) provides backpressure under the storm.
#[test]
#[ignore = "manual perf test: 50-client handshake storm; pin a core with taskset -c <core> and run with --release"]
fn handshake_storm_established_session_latency() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .expect("T010: test runtime should build");
    runtime.block_on(async {
        tokio::time::timeout(TEST_TIMEOUT, run_storm_test())
            .await
            .expect("T010: storm test should complete within timeout");
    });
}

async fn run_storm_test() {
    let fixture = StormFixture::start().await;

    // -- Phase 1: Baseline (no concurrent handshakes) --
    warmup_reads(&fixture.session, 10).await;

    let baseline_latencies = measure_read_latencies(&fixture.session, BASELINE_SAMPLES).await;
    let baseline_p99 = percentile(&baseline_latencies, 99.0);

    // -- Phase 2: Storm (>= 50 concurrent channel openers) --
    // Pre-build all storm clients so that synchronous file I/O (CertificateStore
    // init, cert/key loading) does not run on tokio worker threads during the
    // measurement window. Only the async OSC handshake should contribute load.
    let storm_clients = build_storm_clients(&fixture.client_pki, fixture.unique, STORM_CLIENTS);

    // Spawn storm clients on dedicated OS threads, each with its own
    // current-thread Tokio runtime. This fully isolates their inline
    // client-side OSC crypto (RSA-2048, still on the async path until
    // T013) from the measurement runtime. Per-thread runtimes give
    // each storm client its own schedulable OS thread that the OS
    // scheduler can preempt independently, rather than sharing a
    // single Tokio worker that monopolises the pinned core during
    // CPU-bound crypto bursts.
    let storm_threads = spawn_storm_clients(storm_clients, fixture.endpoint.clone());

    // Issue reads concurrently with the handshake storm. Reads stay on the
    // main test runtime so they measure server responsiveness, not storm-
    // client CPU contention.
    let storm_latencies = measure_read_latencies(&fixture.session, STORM_SAMPLES).await;
    let storm_p99 = percentile(&storm_latencies, 99.0);

    // -- Detach storm threads (bounded cleanup) --
    // Each storm worker performs one OSC handshake then holds briefly
    // (50ms) before exiting. We cannot abort OS threads, so we detach
    // them. They finish on their own within a bounded time. When the
    // fixture (server) is dropped below, any still-running handshakes
    // fail gracefully.
    drop(storm_threads);

    // -- Cleanup --
    drop(fixture);

    // -- Print results for manual inspection --
    println!(
        "T010/SC-001: baseline_p99={baseline_p99:?}, storm_p99={storm_p99:?}, \
         ratio={:.2}, storm_clients={STORM_CLIENTS}",
        storm_p99.as_secs_f64() / baseline_p99.as_secs_f64()
    );

    // -- Assertion (relative bound) --
    assert!(
        storm_p99 <= baseline_p99 * MAX_REGRESSION_FACTOR as u32,
        "T010/SC-001: Storm p99 ({storm_p99:?}) exceeds \
         {MAX_REGRESSION_FACTOR}x baseline p99 ({baseline_p99:?}). \
         Established-session latency must not scale with handshake load."
    );
}

/// Issue `count` warm-up reads to stabilize the connection.
async fn warmup_reads(session: &Session, count: usize) {
    let read_target = read_target();
    for _ in 0..count {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            session.read(
                std::slice::from_ref(&read_target),
                TimestampsToReturn::Neither,
                0.0,
            ),
        )
        .await;
    }
}

/// Measure per-read latency by issuing `count` sequential reads.
async fn measure_read_latencies(session: &Session, count: usize) -> Vec<Duration> {
    let read_target = read_target();
    let mut latencies = Vec::with_capacity(count);
    for _ in 0..count {
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            session.read(
                std::slice::from_ref(&read_target),
                TimestampsToReturn::Neither,
                0.0,
            ),
        )
        .await;
        let elapsed = start.elapsed();
        match result {
            Ok(Ok(values)) => {
                assert!(
                    !values.is_empty(),
                    "T010: Read should return at least one value"
                );
                assert!(
                    values[0].status().is_good(),
                    "T010: Read should return a good status"
                );
                latencies.push(elapsed);
            }
            Ok(Err(e)) => panic!("T010: Read failed: {e}"),
            Err(_) => panic!("T010: Read timed out after 10s"),
        }
    }
    latencies
}

/// The read target follows existing patterns (session_dispatch_lock_scope.rs):
/// VariableId::Server_ServerStatus_CurrentTime / AttributeId::Value.
fn read_target() -> ReadValueId {
    ReadValueId::new(
        VariableId::Server_ServerStatus_CurrentTime.into(),
        AttributeId::Value,
    )
}

/// Compute the p-th percentile (0 < p <= 100) from a list of durations.
fn percentile(samples: &[Duration], p: f64) -> Duration {
    assert!(!samples.is_empty(), "T010: need at least one sample");
    assert!(
        (0.0..=100.0).contains(&p),
        "T010: percentile must be in (0, 100]"
    );
    let mut sorted: Vec<Duration> = samples.to_vec();
    sorted.sort_unstable();
    let idx = ((samples.len() as f64) * p / 100.0).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

/// Pre-build `count` storm clients. This is synchronous (CertificateStore init,
/// cert/key file I/O) and must happen before the measurement window so it does
/// not consume tokio worker time during reads.
fn build_storm_clients(
    client_pki: &std::path::Path,
    unique: u128,
    count: usize,
) -> Vec<opcua_client::Client> {
    (0..count)
        .map(|i| {
            ClientBuilder::new()
                .application_name(format!("Storm Client {i}"))
                .application_uri(format!("urn:async-opcua:storm-client:{unique}:{i}"))
                .product_uri("urn:async-opcua:storm-client")
                .pki_dir(client_pki)
                .create_sample_keypair(true)
                .trust_server_certs(true)
                .session_retry_limit(0)
                .client()
                .expect("T010: storm client should build")
        })
        .collect()
}

/// Spawn storm clients on dedicated OS threads, each with its own
/// current-thread Tokio runtime. This fully isolates their inline
/// client-side OSC crypto (RSA-2048, still on the async path until
/// T013) from the measurement runtime so that the test measures
/// server responsiveness under server-side handshake load, not
/// client/test-runtime starvation. Each thread starts its handshake
/// immediately. No barrier: a natural stagger is more representative
/// of a real handshake storm and avoids an artificial burst.
fn spawn_storm_clients(
    clients: Vec<opcua_client::Client>,
    endpoint: EndpointDescription,
) -> Vec<std::thread::JoinHandle<()>> {
    clients
        .into_iter()
        .map(|mut client| {
            let endpoint = endpoint.clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("T010: storm worker runtime should build");
                runtime.block_on(async move {
                    let result = client
                        .open_secure_channel_to_endpoint_directly(
                            endpoint,
                            IdentityToken::Anonymous,
                        )
                        .await;

                    if let Ok((channel, mut channel_loop)) = result {
                        // Keep the channel briefly alive so the server-side
                        // connection task remains active during the storm,
                        // then clean up.
                        let _ = tokio::time::timeout(
                            Duration::from_millis(50),
                            poll_until_closed(&mut channel_loop),
                        )
                        .await;
                        drop(channel);
                    }
                });
            })
        })
        .collect()
}

/// Poll a `SecureChannelEventLoop` until it reports `Closed`.
async fn poll_until_closed(channel_loop: &mut SecureChannelEventLoop<DefaultTransport>) {
    loop {
        if matches!(channel_loop.poll().await, TransportPollResult::Closed(_)) {
            break;
        }
    }
}

/// Test fixture: server + one established session on a secured endpoint.
struct StormFixture {
    handle: opcua_server::ServerHandle,
    session: Arc<Session>,
    endpoint: EndpointDescription,
    event_loop_task: tokio::task::JoinHandle<StatusCode>,
    server_task: tokio::task::JoinHandle<()>,
    client_pki: std::path::PathBuf,
    unique: u128,
    _temp_dir: tempfile::TempDir,
}

impl StormFixture {
    async fn start() -> Self {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();

        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("crypto-offload-storm-{id}-"))
            .tempdir()
            .expect("T010: temp dir");
        let server_pki = temp_dir.path().join("server-pki");
        let client_pki = temp_dir.path().join("client-pki");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("T010: listener should bind");
        let port = listener.local_addr().expect("T010: listener addr").port();
        let endpoint_url = format!("opc.tcp://127.0.0.1:{port}/");

        // Basic256Sha256 / SignAndEncrypt requires RSA-2048 asymmetric
        // sign+encrypt and decrypt+verify. OPC-10000-6 6.7.2.
        let security_policy = SecurityPolicy::Basic256Sha256;
        let security_mode = MessageSecurityMode::SignAndEncrypt;

        let (server, handle) = ServerBuilder::new()
            .application_name("Crypto Offload Storm Test")
            .application_uri(format!("urn:async-opcua:crypto-offload-storm:{unique}"))
            .product_uri("urn:async-opcua:crypto-offload-storm")
            .host("127.0.0.1")
            .port(port)
            .pki_dir(&server_pki)
            .create_sample_keypair(true)
            .trust_client_certs(true)
            .discovery_urls(vec![endpoint_url.clone()])
            .add_endpoint(
                "crypto_offload_storm",
                (
                    "/",
                    security_policy,
                    security_mode,
                    &[ANONYMOUS_USER_TOKEN_ID] as &[&str],
                ),
            )
            .build()
            .expect("T010: server should build");

        handle.info().port.store(port, Ordering::Relaxed);

        let server_task = tokio::spawn(async move {
            let _ = server.run_with(listener).await;
        });

        // Wait for the server to start accepting connections.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Get the endpoint description from the server's advertised endpoints.
        let endpoint = handle
            .info()
            .endpoints(&UAString::from(endpoint_url.as_str()), &None)
            .expect("T010: endpoints should be advertised")
            .into_iter()
            .find(|e| {
                e.security_policy_uri.as_ref() == security_policy.to_uri()
                    && e.security_mode == security_mode
            })
            .expect("T010: secured endpoint should be found");

        // Establish the primary session on the secured endpoint.
        let mut primary_client = ClientBuilder::new()
            .application_name("Crypto Offload Storm Primary Client")
            .application_uri(format!(
                "urn:async-opcua:crypto-offload-storm-primary:{unique}"
            ))
            .product_uri("urn:async-opcua:crypto-offload-storm-primary")
            .pki_dir(&client_pki)
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_limit(0)
            .client()
            .expect("T010: primary client should build");

        let (session, event_loop) = primary_client
            .connect_to_matching_endpoint(
                (
                    endpoint_url.as_str(),
                    security_policy.to_str(),
                    security_mode,
                ),
                IdentityToken::Anonymous,
            )
            .await
            .expect("T010: primary session should connect to secured endpoint");

        let event_loop_task = event_loop.spawn();

        tokio::time::timeout(Duration::from_secs(30), session.wait_for_connection())
            .await
            .expect("T010: primary session should become connected");

        Self {
            handle,
            session,
            endpoint,
            event_loop_task,
            server_task,
            client_pki,
            unique,
            _temp_dir: temp_dir,
        }
    }
}

impl Drop for StormFixture {
    fn drop(&mut self) {
        self.handle.cancel();
        self.event_loop_task.abort();
        self.server_task.abort();
    }
}
