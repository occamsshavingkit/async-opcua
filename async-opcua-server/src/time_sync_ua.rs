//! UA-based periodic time synchronization (OPC UA Time Sync CU 5505).
//!
//! [`UaHeaderTimeSyncSource`] periodically polls a configured well-known OPC
//! UA endpoint (e.g. a Discovery Server) via a session-less `GetEndpoints`
//! call (OPC 10000-4 §5.5.4) and derives an observed clock offset from the
//! server's `ResponseHeader.timestamp` (OPC 10000-4 §7.33), reusing the
//! existing client Service-call path — no new external dependency.

use std::time::Duration;

use opcua_client::ClientBuilder;
use opcua_core::sync::RwLock;
use opcua_types::DateTime;
use tracing::{debug, warn};

use crate::time_sync::{TimeSyncMechanism, TimeSyncSource, TimeSyncStatus};

/// Floor applied to a configured poll interval so a pathologically small
/// value cannot busy-loop the configured discovery endpoint.
pub(crate) const MIN_UA_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// A [`TimeSyncSource`] that periodically derives clock offset from a
/// well-known OPC UA endpoint's response header timestamps (CU 5505).
pub struct UaHeaderTimeSyncSource {
    endpoint_url: String,
    poll_interval: Duration,
    state: RwLock<TimeSyncStatus>,
}

impl UaHeaderTimeSyncSource {
    /// Create a new source polling `endpoint_url` at `poll_interval`.
    ///
    /// `poll_interval` is clamped up to [`MIN_UA_POLL_INTERVAL`] if smaller,
    /// so a caller-supplied value cannot cause the poller to hammer the
    /// configured endpoint.
    pub fn new(endpoint_url: impl Into<String>, poll_interval: Duration) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            endpoint_url: endpoint_url.into(),
            poll_interval: poll_interval.max(MIN_UA_POLL_INTERVAL),
            state: RwLock::new(TimeSyncStatus {
                mechanism: TimeSyncMechanism::UaHeaderBased,
                synchronized: false,
                last_sync: None,
                observed_skew: None,
            }),
        })
    }

    /// Perform a single poll attempt against the configured endpoint,
    /// updating shared status. Never panics: any failure is reflected as
    /// `synchronized = false` rather than propagated (FR-009).
    async fn poll_once(&self, client: &opcua_client::Client) {
        match client
            .get_server_time_via_endpoints(self.endpoint_url.as_str())
            .await
        {
            Ok(server_timestamp) => {
                let local_now = DateTime::now();
                let observed_skew = duration_between(local_now, server_timestamp);
                debug!(
                    endpoint = %self.endpoint_url,
                    ?observed_skew,
                    "UA time-sync poll succeeded"
                );
                let mut state = self.state.write();
                state.synchronized = true;
                state.last_sync = Some(server_timestamp);
                state.observed_skew = Some(observed_skew);
            }
            Err(err) => {
                warn!(
                    endpoint = %self.endpoint_url,
                    error = %err,
                    "UA time-sync poll failed; reporting not synchronized"
                );
                let mut state = self.state.write();
                state.synchronized = false;
            }
        }
    }
}

impl TimeSyncSource for UaHeaderTimeSyncSource {
    fn status(&self) -> TimeSyncStatus {
        self.state.read().clone()
    }

    fn run<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let client = ClientBuilder::new()
                .application_name("TimeSyncClient")
                .application_uri("urn:TimeSyncClient")
                .session_retry_limit(1)
                .client();

            let Ok(client) = client else {
                warn!("Failed to create a valid client for UA time-sync polling");
                return std::future::pending().await;
            };

            let mut interval = tokio::time::interval(self.poll_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;
                self.poll_once(&client).await;
            }
        })
    }
}

/// The absolute duration between two `DateTime`s, order-independent.
fn duration_between(a: DateTime, b: DateTime) -> Duration {
    let (a, b) = (a.as_chrono(), b.as_chrono());
    let delta = if a >= b { a - b } else { b - a };
    delta.to_std().unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_source_reports_not_synchronized_before_first_poll() {
        let source =
            UaHeaderTimeSyncSource::new("opc.tcp://example.invalid:4840", Duration::from_secs(60));
        let status = source.status();
        assert_eq!(status.mechanism, TimeSyncMechanism::UaHeaderBased);
        assert!(!status.synchronized);
        assert!(status.last_sync.is_none());
        assert!(status.observed_skew.is_none());
    }

    #[test]
    fn poll_interval_is_clamped_to_the_floor() {
        let source =
            UaHeaderTimeSyncSource::new("opc.tcp://example.invalid:4840", Duration::from_millis(1));
        assert_eq!(source.poll_interval, MIN_UA_POLL_INTERVAL);
    }
}
