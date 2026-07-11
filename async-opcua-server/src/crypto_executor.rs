// SPDX-License-Identifier: MPL-2.0+

//! Dedicated lower-priority executor for server-side asymmetric crypto
//! (T010A).
//!
//! Under a handshake storm the shared `spawn_blocking` pool's RSA threads
//! contend with established-session reads on the runtime workers. This
//! module provides a bounded, lower-priority dedicated worker lane so that
//! handshake crypto (OSC decrypt/verify, OSC sign/encrypt, CreateSession
//! RSA signing, ECC ephemeral keygen) runs at reduced OS scheduling priority,
//! separate from both the tokio async workers and the shared blocking pool.
//!
//! Architecture:
//! - A bounded `crossbeam_channel` (depth configurable) carries jobs from
//!   async submitters to dedicated worker threads.  The crossbeam
//!   `Receiver` is `Clone`, so each worker owns its own receiver and calls
//!   `recv()` directly — **no mutex is held across the blocking receive**.
//! - Async backpressure is provided by a `tokio::sync::Semaphore` with the
//!   same depth as the channel.  A submitter acquires a permit (`.await`)
//!   before pushing; the permit travels with the job and is released when
//!   the worker finishes.  Because the permit count equals the channel
//!   capacity, `try_send` always succeeds immediately — no blocking call
//!   on the tokio worker.
//! - Dedicated `std::thread` workers consume jobs.  Each worker lowers its
//!   own OS scheduling priority best-effort on Unix (nice +5).
//! - An `AtomicU64` counter exposes how many jobs have been dispatched,
//!   for observability and structural tests (T009).

use std::any::Any;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender, TrySendError};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
use tracing::{debug, warn};

use opcua_core::comms::crypto_offload::{CryptoOffload, CryptoOffloadError, CryptoWork};

/// A type-erased job for the worker pool.
struct CryptoJob {
    work: CryptoWork,
    reply: oneshot::Sender<Box<dyn Any + Send>>,
    /// Backpressure permit — released (via `Drop`) when the worker finishes
    /// the job, unblocking the next submitter on the async side.
    _permit: OwnedSemaphorePermit,
}

/// Dedicated lower-priority executor for server-side asymmetric crypto.
///
/// Created by the server builder. Stored in `ServerInfo` as
/// `Option<Arc<CryptoExecutor>>` and wired onto each `SecureChannel` via
/// `set_crypto_offload`. When dropped, the sender half and semaphore are
/// dropped; the bounded channel closes and workers exit after draining
/// in-flight jobs.
#[derive(Debug)]
pub struct CryptoExecutor {
    tx: Sender<CryptoJob>,
    /// Bounded backpressure: a submitter acquires a permit (`.await`) before
    /// pushing a job. The permit is stored in the job and released when the
    /// worker finishes, so the number of in-flight jobs never exceeds
    /// `queue_depth`.
    permits: Arc<Semaphore>,
    /// Worker thread `JoinHandle`s. Stored so that a future graceful-shutdown
    /// path can `join` them. Today they are simply dropped and the threads
    /// exit on their own when all senders are dropped (channel close).
    #[allow(dead_code)]
    workers: Vec<thread::JoinHandle<()>>,
    /// Total jobs dispatched (submitted to a worker). For observability
    /// and structural tests (T009 proof that crypto ran off-thread).
    jobs_dispatched: Arc<AtomicU64>,
}

impl CryptoExecutor {
    /// Create a new executor with the given worker count and bounded queue
    /// depth.
    ///
    /// Worker threads are spawned immediately and named `opcua-crypto-worker`.
    /// Each attempts to lower its OS scheduling priority on Unix (best-effort).
    pub fn new(worker_count: usize, queue_depth: usize) -> Self {
        let worker_count = worker_count.max(1);
        let depth = queue_depth.max(1);
        let (tx, rx) = crossbeam_channel::bounded::<CryptoJob>(depth);

        // crossbeam Receiver is Clone — each worker gets its own copy and
        // calls recv() directly, no mutex required.
        let rx = Arc::new(rx);
        let permits = Arc::new(Semaphore::new(depth));
        let jobs_dispatched = Arc::new(AtomicU64::new(0));

        let workers = (0..worker_count)
            .map(|i| {
                let rx = Arc::clone(&rx);
                let counter = Arc::clone(&jobs_dispatched);
                thread::Builder::new()
                    .name(format!("opcua-crypto-worker-{i}"))
                    .spawn(move || {
                        try_lower_priority();
                        worker_loop(rx, counter);
                    })
                    .expect("spawn crypto worker thread")
            })
            .collect();

        Self {
            tx,
            permits,
            workers,
            jobs_dispatched,
        }
    }

    /// Number of jobs dispatched since the executor was created.
    pub fn jobs_dispatched(&self) -> u64 {
        self.jobs_dispatched.load(Ordering::Relaxed)
    }
}

fn worker_loop(rx: Arc<Receiver<CryptoJob>>, counter: Arc<AtomicU64>) {
    loop {
        // Blocking receive on a cloned crossbeam Receiver — no lock held.
        // Multiple workers can wait concurrently; the channel internally
        // distributes jobs to one receiver at a time.
        match rx.recv() {
            Ok(job) => {
                counter.fetch_add(1, Ordering::Relaxed);
                let CryptoJob {
                    work,
                    reply,
                    _permit,
                } = job;
                match catch_unwind(AssertUnwindSafe(work)) {
                    Ok(result) => {
                        // Best-effort reply: if the caller dropped the future
                        // (e.g. connection closed), the receiver is gone.
                        let _ = reply.send(result);
                    }
                    Err(_) => {
                        warn!(
                            "crypto worker job panicked; worker thread continues \
                             to process subsequent jobs"
                        );
                        // Drop `reply` without sending so the oneshot receiver
                        // observes `Err` → CryptoOffloadError::Panic.
                    }
                }
                // `_permit` is dropped here (both success and panic paths),
                // releasing the semaphore permit and unblocking the next
                // submitter.
            }
            Err(_) => {
                debug!("crypto worker exiting (channel closed)");
                break;
            }
        }
    }
}

impl CryptoOffload for CryptoExecutor {
    fn execute_boxed(
        &self,
        work: CryptoWork,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn Any + Send>, CryptoOffloadError>> + Send>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let tx = self.tx.clone();
        let permits = Arc::clone(&self.permits);

        Box::pin(async move {
            // Async backpressure: acquire a permit before pushing. This
            // .await suspends the caller when too many jobs are in flight,
            // providing the same bounded backpressure the spec requires.
            let permit = permits
                .acquire_owned()
                .await
                .map_err(|_| CryptoOffloadError::Closed)?;

            let job = CryptoJob {
                work,
                reply: reply_tx,
                _permit: permit,
            };

            // try_send is non-blocking. Because the semaphore depth equals
            // the channel capacity, the permit guarantees a free slot, so
            // Full is unreachable under correct usage.
            match tx.try_send(job) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => return Err(CryptoOffloadError::Closed),
                Err(TrySendError::Disconnected(_)) => return Err(CryptoOffloadError::Closed),
            }

            match reply_rx.await {
                Ok(result) => Ok(result),
                Err(_) => Err(CryptoOffloadError::Panic),
            }
        })
    }
}

/// Best-effort: lower the OS scheduling priority of the current thread so
/// handshake RSA/ECC crypto does not contend with established-session reads
/// on the tokio workers.
///
/// Uses `extern "C"` FFI to avoid adding the `libc` crate as a dependency.
/// If the syscall fails (e.g. insufficient permissions), a warning is
/// logged and the worker continues at default priority.
#[cfg(unix)]
fn try_lower_priority() {
    extern "C" {
        fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    }
    // PRIO_PROCESS = 0, who = 0 means "current thread" on Linux >= 2.6.12.
    const PRIO_PROCESS: i32 = 0;
    // Nice value 5 = modestly lower priority (range: -20 highest .. 19 lowest).
    const TARGET_NICE: i32 = 5;

    let rc = unsafe { setpriority(PRIO_PROCESS, 0, TARGET_NICE) };
    if rc != 0 {
        warn!(
            "Could not lower priority of crypto worker thread (target nice={TARGET_NICE}); \
             continuing at default priority"
        );
    } else {
        debug!("Crypto worker thread nice set to {TARGET_NICE}");
    }
}

#[cfg(not(unix))]
fn try_lower_priority() {
    // No standard mechanism on non-Unix; priority separation is Unix/Linux
    // best-effort only.
}
