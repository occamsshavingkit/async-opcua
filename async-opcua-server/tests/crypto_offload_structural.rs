//! T009 [US1] Structural proof (load-bearing, C6): the OpenSecureChannel
//! asymmetric crypto path is actually offloaded off the async worker thread.
//!
//! On a `current_thread` runtime there is exactly one async worker thread.
//! If the server-side OSC decrypt/verify and sign/encrypt stayed inline on
//! that worker, the liveness probe stalls and the dedicated crypto
//! executor's job counter stays at zero.
//!
//! Before T010A the crypto ran on `spawn_blocking` (proven by counting
//! blocking-pool threads via `on_thread_start`). T010A moved the crypto to
//! a dedicated lower-priority executor (`CryptoExecutor`), so
//! `on_thread_start` no longer fires for crypto — instead, the executor's
//! `jobs_dispatched` counter increments. We now check both: the executor
//! processed crypto AND the liveness probe shows the async worker was
//! responsive.
//!
//! A cooperative liveness probe (`yield_now` counter + periodic heartbeat)
//! runs concurrently with the handshake to demonstrate that the single
//! async worker remains responsive while the crypto executes off-thread.
//!
//! Grounding: OPC-10000-4 5.6.2 (OpenSecureChannel Service);
//!            OPC-10000-6 6.7.2 (message security);
//!            crypto-offload-contracts.md C6.

use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use opcua_client::{transport::TransportPollResult, ClientBuilder, IdentityToken};
use opcua_crypto::SecurityPolicy;
use opcua_server::{ServerBuilder, ANONYMOUS_USER_TOKEN_ID};
use opcua_types::{MessageSecurityMode, UAString};
use tokio::net::TcpListener;

const TEST_TIMEOUT: Duration = Duration::from_secs(15);

/// C6 structural contract: on a `current_thread` runtime whose single worker
/// is kept busy by a liveness probe, a secured OSC handshake can only complete
/// if the asymmetric crypto runs off-thread. We prove offloading by checking
/// the dedicated crypto executor's `jobs_dispatched` counter — it must
/// increase during the handshake, proving the crypto was dispatched to the
/// dedicated lower-priority workers (T010A).
#[test]
fn osc_asymmetric_crypto_offloaded_to_blocking_pool() {
    let blocking_threads = Arc::new(AtomicUsize::new(0));
    let counter = blocking_threads.clone();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .on_thread_start(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })
        .build()
        .expect("T009: current_thread runtime should build");

    runtime.block_on(async {
        tokio::time::timeout(TEST_TIMEOUT, osc_offload_proof(blocking_threads))
            .await
            .expect("T009: OSC offload proof should complete within timeout");
    });
}

async fn osc_offload_proof(blocking_threads: Arc<AtomicUsize>) {
    // -- Fixture setup (follows create_session_certificate_lock_scope.rs) --
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();

    let temp_dir = tempfile::Builder::new()
        .prefix("crypto-offload-structural")
        .tempdir()
        .expect("temp dir");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    let endpoint_url = format!("opc.tcp://127.0.0.1:{port}/");

    // Basic256Sha256 / SignAndEncrypt requires RSA-2048 asymmetric
    // sign+encrypt and decrypt+verify. OPC-10000-6 6.7.2.
    let security_policy = SecurityPolicy::Basic256Sha256;
    let security_mode = MessageSecurityMode::SignAndEncrypt;

    let (server, handle) = ServerBuilder::new()
        .application_name("Crypto Offload Structural Proof")
        .application_uri(format!("urn:async-opcua:crypto-offload:{unique}"))
        .product_uri("urn:async-opcua:crypto-offload")
        .host("127.0.0.1")
        .port(port)
        .pki_dir(temp_dir.path().join("server-pki"))
        .create_sample_keypair(true)
        .trust_client_certs(true)
        .discovery_urls(vec![endpoint_url.clone()])
        .add_endpoint(
            "crypto_offload_structural",
            (
                "/",
                security_policy,
                security_mode,
                &[ANONYMOUS_USER_TOKEN_ID] as &[&str],
            ),
        )
        .build()
        .expect("server should build");

    handle.info().port.store(port, Ordering::Relaxed);

    let server_task = tokio::spawn(async move {
        let _ = server.run_with(listener).await;
    });

    let endpoint = handle
        .info()
        .endpoints(&UAString::from(endpoint_url.as_str()), &None)
        .expect("endpoints should be advertised")
        .into_iter()
        .find(|e| {
            e.security_policy_uri.as_ref() == security_policy.to_uri()
                && e.security_mode == security_mode
        })
        .expect("secured endpoint should be found");

    let mut client = ClientBuilder::new()
        .application_name("Crypto Offload Structural Proof Client")
        .application_uri(format!("urn:async-opcua:crypto-offload-client:{unique}"))
        .product_uri("urn:async-opcua:crypto-offload-client")
        .pki_dir(temp_dir.path().join("client-pki"))
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_limit(0)
        .client()
        .expect("client should build");

    // -- Liveness probe: a cooperative yield-counter and periodic heartbeat
    //    that would stall if the worker were blocked by inline crypto. --
    let yield_count = Arc::new(AtomicU64::new(0));
    let heartbeat_count = Arc::new(AtomicU64::new(0));

    let liveness_probe = {
        let yc = yield_count.clone();
        let hc = heartbeat_count.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(5));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        hc.fetch_add(1, Ordering::Relaxed);
                    }
                    _ = tokio::task::yield_now() => {
                        yc.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        })
    };

    // -- Record counters BEFORE the handshake --
    let threads_before = blocking_threads.load(Ordering::Relaxed);
    let yields_before = yield_count.load(Ordering::Relaxed);
    let heartbeats_before = heartbeat_count.load(Ordering::Relaxed);
    // T010A: count jobs dispatched by the dedicated crypto executor.
    // If no executor is configured (falling back to spawn_blocking),
    // jobs_before stays 0 and the threads-delta assertion covers the proof.
    let jobs_before = handle
        .info()
        .crypto_executor
        .as_ref()
        .map(|e| e.jobs_dispatched())
        .unwrap_or(0);

    // -- OSC handshake with real RSA-2048 asymmetric crypto --
    let handshake_result = client
        .open_secure_channel_to_endpoint_directly(endpoint, IdentityToken::Anonymous)
        .await;

    // -- Record counters AFTER the handshake --
    let threads_after = blocking_threads.load(Ordering::Relaxed);
    let yields_after = yield_count.load(Ordering::Relaxed);
    let heartbeats_after = heartbeat_count.load(Ordering::Relaxed);
    let jobs_after = handle
        .info()
        .crypto_executor
        .as_ref()
        .map(|e| e.jobs_dispatched())
        .unwrap_or(0);

    liveness_probe.abort();

    let (channel, mut channel_loop) =
        handshake_result.expect("T009: secured OSC handshake should succeed");

    let channel_poller = tokio::spawn(async move {
        loop {
            if matches!(channel_loop.poll().await, TransportPollResult::Closed(_)) {
                break;
            }
        }
    });

    // -- Cleanup --
    drop(channel);
    channel_poller.abort();
    handle.cancel();
    server_task.abort();
    drop(temp_dir);

    // -- C6 Structural assertion (load-bearing): crypto was offloaded --
    // With T010A, the crypto runs on the dedicated CryptoExecutor workers
    // (dedicated OS threads, not the tokio blocking pool). We prove
    // offloading by checking the executor's jobs_dispatched counter
    // increased during the handshake. If no executor is configured, we
    // fall back to checking that spawn_blocking threads increased.
    let jobs_delta = jobs_after - jobs_before;
    let threads_delta = threads_after - threads_before;
    assert!(
        jobs_delta > 0 || threads_delta > 0,
        "T009/C6: OSC handshake must offload asymmetric crypto off the async worker. \
         Executor jobs dispatched: before={jobs_before}, after={jobs_after} (delta={jobs_delta}). \
         Blocking-pool threads: before={threads_before}, after={threads_after} (delta={threads_delta}). \
         A zero delta on both means the crypto stayed inline on the current_thread worker."
    );

    // -- Liveness assertion: the single async worker stayed responsive --
    let yield_delta = yields_after - yields_before;
    let heartbeat_delta = heartbeats_after - heartbeats_before;
    assert!(
        yield_delta > 0 && heartbeat_delta > 0,
        "T009/C6: Liveness probe should show the async worker was available \
         during the handshake (yields: {yield_delta}, heartbeats: {heartbeat_delta})."
    );
}
