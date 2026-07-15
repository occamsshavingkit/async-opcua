//! Feature 093 — Time Sync profile decision, User Story 3.
//!
//! Demonstrates integrating a platform time-synchronization mechanism (NTP,
//! IEEE 1588 PTP, or IEEE 802.1AS gPTP) that this library does not implement
//! itself. OPC UA treats these as dependencies on the underlying
//! network/device stack rather than something the OPC UA application layer
//! re-implements — see OPC 10000-84 §6.6.3.6 (UAFX IA-station Facet), which
//! lists gPTP time synchronization as delivered by the networking layer, not
//! the application.
//!
//! If your platform already runs one of these (an NTP daemon, PTP hardware
//! with kernel timestamping, a gPTP-capable network stack), implement
//! [`TimeSyncSource`] to report on it — the server never needs to speak the
//! wire protocol itself. This satisfies OPC UA Time Sync CU 2479 (PTP),
//! 2480 (gPTP), or 2786 (NTP) for your deployment.
//!
//! This example reports a fixed status for illustration; a real
//! implementation would read from whatever your platform's existing
//! mechanism already tracks (e.g. `chronyc tracking`, a PTP hardware clock
//! ioctl, or a gPTP daemon's status socket).

use std::{sync::Arc, time::Duration};

use opcua_server::{ServerBuilder, TimeSyncMechanism, TimeSyncSource, TimeSyncStatus};
use opcua_types::DateTime;

/// A minimal `TimeSyncSource` standing in for a platform NTP client. Swap
/// `TimeSyncMechanism::Ntp` for `Ptp` or `Gptp` if your platform mechanism is
/// one of those instead — the trait is the same for all three.
struct PlatformNtpSource;

impl TimeSyncSource for PlatformNtpSource {
    fn status(&self) -> TimeSyncStatus {
        // A real implementation reads this from the platform's already-running
        // NTP client rather than fabricating it.
        TimeSyncStatus {
            mechanism: TimeSyncMechanism::Ntp,
            synchronized: true,
            last_sync: Some(DateTime::now()),
            observed_skew: Some(Duration::from_millis(3)),
        }
    }
}

fn main() {
    let builder = ServerBuilder::new().with_time_sync_source(Arc::new(PlatformNtpSource));

    let status = PlatformNtpSource.status();
    println!(
        "Registered a custom TimeSyncSource: mechanism={:?}, synchronized={}, observed_skew={:?}",
        status.mechanism, status.synchronized, status.observed_skew
    );

    // `builder` is now ready for `.build()` with the rest of your server
    // configuration (endpoints, node managers, certificates, ...); omitted
    // here since this example only demonstrates the TimeSyncSource seam.
    drop(builder);
}
