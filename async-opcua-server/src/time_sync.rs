//! Time synchronization status reporting.
//!
//! This module defines [`TimeSyncSource`], an extensibility point that lets a
//! server report how (and whether) its clock is kept synchronized. The library
//! ships one built-in implementation, [`OsClockSource`], and a feature-gated
//! second one, `UaHeaderTimeSyncSource` (see the `time-sync-ua` feature), that
//! periodically derives an offset from a well-known OPC UA endpoint's response
//! header timestamps.
//!
//! Platform-specific mechanisms such as NTP, PTP (IEEE 1588), and gPTP
//! (IEEE 802.1AS) are intentionally not implemented in this library: OPC UA
//! treats those as dependencies on the underlying network/device stack rather
//! than something the application layer re-implements (see the UAFX
//! IA-station Facet, OPC 10000-84 §6.6.3.6, which lists gPTP time
//! synchronization as delivered by the networking layer). An operator whose
//! platform already has one of these running can implement [`TimeSyncSource`]
//! to report on it — see `async-opcua-server/examples/custom_time_sync.rs`.

use std::{future::Future, pin::Pin, time::Duration};

use opcua_types::DateTime;

/// The mechanism a [`TimeSyncSource`] represents.
///
/// Each variant corresponds to an OPC UA Foundation "Time Sync" conformance
/// unit. `Ntp`, `Ptp`, and `Gptp` are never produced by this library's
/// built-in sources — they exist so a user-supplied [`TimeSyncSource`] can
/// truthfully report which platform mechanism it is backed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSyncMechanism {
    /// Time synchronization via the host operating system's clock (CU 2478).
    OsClock,
    /// Time synchronization via the Network Time Protocol (CU 2786).
    /// Not implemented in-library; report this from a user-supplied source.
    Ntp,
    /// Time synchronization via IEEE 1588 Precision Time Protocol (CU 2479).
    /// Not implemented in-library; report this from a user-supplied source.
    Ptp,
    /// Time synchronization via IEEE 802.1AS (gPTP) (CU 2480).
    /// Not implemented in-library; report this from a user-supplied source.
    Gptp,
    /// Periodic synchronization derived from a well-known OPC UA endpoint's
    /// response header timestamps (CU 5505).
    UaHeaderBased,
    /// Any other operator-supplied mechanism, named for diagnostics.
    Custom(String),
}

/// A snapshot of a [`TimeSyncSource`]'s current state.
#[derive(Debug, Clone)]
pub struct TimeSyncStatus {
    /// The mechanism this status was produced by.
    pub mechanism: TimeSyncMechanism,
    /// Whether the source currently considers itself synchronized.
    ///
    /// `false` whenever no successful synchronization/observation has
    /// occurred yet, or the most recent attempt failed. A source MUST NOT
    /// report `true` from a never-completed or failed observation.
    pub synchronized: bool,
    /// The time of the most recent successful synchronization/observation,
    /// or `None` if none has occurred yet.
    pub last_sync: Option<DateTime>,
    /// The most recently observed clock offset magnitude, or `None` if no
    /// offset has been measured (for example, a pure OS-clock source never
    /// measures an offset against anything).
    pub observed_skew: Option<Duration>,
}

/// A source of time-synchronization status for the server.
///
/// The server always derives its timestamps from the OS clock; a
/// `TimeSyncSource` reports *how* that clock is (or is not) being kept
/// synchronized, for conformance and diagnostics. Implement this to
/// integrate a platform time-sync mechanism (NTP daemon, PTP hardware, gPTP
/// stack) that already exists outside this library — see
/// `async-opcua-server/examples/custom_time_sync.rs`.
///
/// Register an implementation via
/// [`ServerBuilder::with_time_sync_source`](crate::ServerBuilder::with_time_sync_source).
/// If none is registered, the server defaults to [`OsClockSource`].
pub trait TimeSyncSource: Send + Sync {
    /// A cheap, non-blocking snapshot of current sync status.
    ///
    /// Implementations MUST NOT block, perform I/O, or panic in this method;
    /// it reads cached/precomputed state.
    fn status(&self) -> TimeSyncStatus;

    /// An optional background task that keeps this source's status current.
    ///
    /// The server drives this once at startup in the same singleton-task slot
    /// used for discovery-server registration and mDNS advertisement (so it
    /// is never replicated across shards in sharded run mode), and simply
    /// drops the future on server shutdown — exactly like those tasks, this
    /// method is never expected to resolve on its own. Most sources need no
    /// background work and use the default implementation, which never
    /// resolves. A source that needs to poll (e.g. a UA-based source reading
    /// a remote endpoint's response header timestamps, CU 5505) overrides
    /// this.
    fn run<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}

/// The default [`TimeSyncSource`]: reports the host operating system's clock
/// as the synchronization mechanism (OPC UA Time Sync CU 2478).
///
/// This makes no claim about whether the OS clock is actually being kept
/// synchronized by the platform (e.g. via chrony, systemd-timesyncd, w32time)
/// — that is the deploying operator's responsibility, consistent with CU
/// 2478's description ("Application supports time synchronization via
/// features of a standard operating system").
#[derive(Debug, Default, Clone, Copy)]
pub struct OsClockSource;

impl TimeSyncSource for OsClockSource {
    fn status(&self) -> TimeSyncStatus {
        TimeSyncStatus {
            mechanism: TimeSyncMechanism::OsClock,
            synchronized: true,
            last_sync: Some(DateTime::now()),
            observed_skew: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_clock_source_reports_synchronized_os_clock() {
        let status = OsClockSource.status();
        assert_eq!(status.mechanism, TimeSyncMechanism::OsClock);
        assert!(status.synchronized);
        assert!(status.last_sync.is_some());
        assert!(status.observed_skew.is_none());
    }
}
