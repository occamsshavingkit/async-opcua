//! Feature 093 — Time Sync profile decision. Covers CU 2478 (OS-based, always
//! on), CU 3802 (configurable clock skew), and CU 5505 (UA-based periodic
//! sync via `UaHeaderTimeSyncSource`, `time-sync-ua` feature).
//!
//! Grounding: OPC-10000-4 §5.5.4 (GetEndpoints), §7.33 (ResponseHeader
//! timestamp).

use super::utils::Tester;
use opcua::server::TimeSyncMechanism;

#[tokio::test]
async fn default_server_reports_os_clock_synchronized() {
    // CU 2478: with no TimeSyncSource configured, the server defaults to
    // OsClockSource and reports synchronized OS-clock status out of the box.
    let tester = Tester::new_default_server(true).await;
    let status = tester.handle.info().time_sync_source.status();

    assert_eq!(status.mechanism, TimeSyncMechanism::OsClock);
    assert!(status.synchronized);
    assert!(status.last_sync.is_some());
    assert!(status.observed_skew.is_none());
}

#[cfg(feature = "time-sync-ua")]
mod ua_header_based {
    use std::time::Duration;

    use opcua::server::UaHeaderTimeSyncSource;

    use super::super::utils::default_server;
    use super::*;

    #[tokio::test]
    async fn poller_observes_skew_against_a_reachable_endpoint() {
        // CU 5505: two local server instances. The second polls the first's
        // GetEndpoints (OPC-10000-4 §5.5.4) and derives an offset from
        // ResponseHeader.timestamp (§7.33).
        let well_known = Tester::new_default_server(true).await;
        let source = UaHeaderTimeSyncSource::new(well_known.endpoint(), Duration::from_secs(1));

        let poller = Tester::new(default_server().with_time_sync_source(source), true).await;

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut status = poller.handle.info().time_sync_source.status();
        while !status.synchronized && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            status = poller.handle.info().time_sync_source.status();
        }

        assert_eq!(status.mechanism, TimeSyncMechanism::UaHeaderBased);
        assert!(
            status.synchronized,
            "poller should observe a successful sync against a reachable endpoint within 10s"
        );
        assert!(status.last_sync.is_some());
        assert!(
            status.observed_skew.is_some(),
            "a successful poll must report an observed skew"
        );
    }

    #[tokio::test]
    async fn poller_reports_not_synchronized_against_an_unreachable_endpoint() {
        // FR-009: a failed poll must report synchronized = false, never panic,
        // and must not fabricate a stale "synchronized" reading.
        let source = UaHeaderTimeSyncSource::new("opc.tcp://127.0.0.1:1/", Duration::from_secs(1));

        let poller = Tester::new(default_server().with_time_sync_source(source), true).await;

        // Give the poller a couple of intervals to attempt (and fail) a poll.
        tokio::time::sleep(Duration::from_secs(3)).await;

        let status = poller.handle.info().time_sync_source.status();
        assert_eq!(status.mechanism, TimeSyncMechanism::UaHeaderBased);
        assert!(
            !status.synchronized,
            "an unreachable endpoint must never be reported as synchronized"
        );
    }

    #[tokio::test]
    async fn observed_skew_and_configured_tolerance_are_jointly_retrievable() {
        // FR-006 / US2 Acceptance Scenario 3: ties CU 5505 to CU 3802 — a
        // caller must be able to read both the configured tolerance and the
        // poller's observed skew from the same ServerInfo, and determine
        // whether the excess is visible.
        let well_known = Tester::new_default_server(true).await;
        let source = UaHeaderTimeSyncSource::new(well_known.endpoint(), Duration::from_secs(1));

        let mut builder = default_server().with_time_sync_source(source);
        builder.config_mut().max_acceptable_clock_skew_ns = 2_000_000_000;
        let poller = Tester::new(builder, true).await;

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut status = poller.handle.info().time_sync_source.status();
        while !status.synchronized && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            status = poller.handle.info().time_sync_source.status();
        }
        assert!(status.synchronized, "precondition: poller must sync first");

        let tolerance = poller.handle.info().config.max_acceptable_clock_skew();
        let observed = status
            .observed_skew
            .expect("a synchronized status must carry an observed skew");

        assert_eq!(tolerance, Duration::from_millis(2000));
        // Two co-located loopback servers should be well within a 2s
        // tolerance; this asserts the comparison itself is meaningful (not
        // trivially satisfied by an absent value).
        assert!(
            observed < tolerance,
            "observed skew {observed:?} should be within the configured tolerance {tolerance:?} \
             for two co-located loopback servers"
        );
    }
}
