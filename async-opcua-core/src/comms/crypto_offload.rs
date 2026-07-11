// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0+

//! Dedicated crypto offload seam for asymmetric handshake crypto (T010A).
//!
//! When a [`CryptoOffload`] executor is attached to a `SecureChannel`, the
//! OpenSecureChannel asymmetric crypto (decrypt+verify, sign+encrypt) and
//! the CreateSession server-signature / ECC keygen run on that executor's
//! dedicated workers instead of the shared `spawn_blocking` pool. This gives
//! the deployment a scheduling-priority seam: handshake RSA/ECC can be run
//! at lower OS priority than the tokio workers that serve established
//! sessions, so a handshake storm cannot starve already-connected clients.
//!
//! The trait uses type erasure (`Box<dyn Any>`) for the result so it is
//! dyn-compatible and can be stored as `Option<Arc<dyn CryptoOffload>>`. The
//! generic [`execute_offloaded`] helper handles the erasure/downcast, so call
//! sites remain type-safe and ergonomic.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;

/// Error returned when a crypto offload job cannot complete.
///
/// Either the executor was shut down ([`CryptoOffloadError::Closed`]) or the
/// worker task panicked while running the closure
/// ([`CryptoOffloadError::Panic`]). Callers map this to
/// `BadInternalError` — the same status code a `JoinError` from
/// `spawn_blocking` would produce — so specific crypto faults are never
/// masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoOffloadError {
    /// The executor's queue was closed (all senders dropped / shutdown).
    Closed,
    /// The worker panicked or was cancelled while running the closure.
    Panic,
}

/// A type-erased closure that produces a type-erased result.
pub type CryptoWork = Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>;

/// A dedicated executor for CPU-bound asymmetric crypto work.
///
/// Implementations SHOULD:
/// - use a bounded queue so that backpressure is applied under load;
/// - run worker threads at lower OS scheduling priority (best-effort);
/// - not hold locks across the closure execution.
///
/// The trait is `Send + Sync + 'static` so it can be stored as
/// `Option<Arc<dyn CryptoOffload>>` on `SecureChannel`.  It uses type-erased
/// closures/results to remain dyn-compatible (no generic methods).
pub trait CryptoOffload: Send + Sync + std::fmt::Debug + 'static {
    /// Submit `work` to the executor and return a future that resolves to
    /// the type-erased result (or a [`CryptoOffloadError`]).
    #[allow(clippy::type_complexity)]
    fn execute_boxed(
        &self,
        work: CryptoWork,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn Any + Send>, CryptoOffloadError>> + Send>>;
}

/// Run `f` on the given executor, or fall back to `spawn_blocking` when
/// `executor` is `None`.
///
/// This helper lets call sites stay unchanged whether or not a dedicated
/// executor is configured: the `None` arm preserves the pre-T010A default
/// (Tokio's shared blocking pool), and the `Some(executor)` arm routes to
/// the dedicated lower-priority workers.
///
/// The inner result type `T` is typically `Result<_, {StatusCode,Error}>`,
/// so callers do `execute_offloaded(executor, move || { ... }).await?` and
/// the double-`Result` collapses via the outer `?`.
pub async fn execute_offloaded<F, T>(
    executor: Option<&dyn CryptoOffload>,
    f: F,
) -> Result<T, CryptoOffloadError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match executor {
        Some(exec) => {
            let erased: CryptoWork = Box::new(move || Box::new(f()) as Box<dyn Any + Send>);
            let result = exec.execute_boxed(erased).await?;
            // Downcast back to T. A failure here indicates a bug in the
            // executor implementation (it returned a different type than
            // the closure produced), not a runtime condition. Map to
            // Panic to surface the error without panicking.
            match result.downcast::<T>() {
                Ok(typed) => Ok(*typed),
                Err(_) => Err(CryptoOffloadError::Panic),
            }
        }
        None => match tokio::task::spawn_blocking(f).await {
            Ok(result) => Ok(result),
            Err(join_err) => {
                if join_err.is_cancelled() {
                    Err(CryptoOffloadError::Closed)
                } else {
                    Err(CryptoOffloadError::Panic)
                }
            }
        },
    }
}
