use tokio::sync::mpsc;

use opcua_types::StatusCode;

/// Default PubSub datagram queue capacity.
///
/// `OperationalLimits` (Part 4) only covers service-call bounds, so the
/// PubSub datagram bound is a crate-level constant. Override per engine with
/// [`crate::PubSubEngine::set_datagram_queue_capacity`].
pub const PUBSUB_DATAGRAM_QUEUE_CAPACITY: usize = 1024;

/// Bounded queue for incoming PubSub datagrams (OPC-10000-14 §9.1.10.1).
///
/// Enforces a processing limit on received PubSub NetworkMessages. When the
/// queue is full, [`DatagramQueue::try_enqueue`] returns
/// `StatusCode::BadTooManyPublishRequests` and the caller drops the datagram
/// rather than accumulating unbounded backpressure.
#[derive(Debug)]
pub struct DatagramQueue {
    tx: mpsc::Sender<Vec<u8>>,
    capacity: usize,
}

impl DatagramQueue {
    /// Creates a new bounded datagram queue with the requested capacity.
    ///
    /// The capacity is clamped to a minimum of 1 so a misconfigured zero
    /// capacity does not reject every datagram.
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<Vec<u8>>) {
        let capacity = capacity.max(1);
        let (tx, rx) = mpsc::channel(capacity);
        (Self { tx, capacity }, rx)
    }

    /// Attempts to enqueue a datagram without blocking.
    ///
    /// Returns:
    /// - `Ok(())` when the datagram was accepted.
    /// - `Err(StatusCode::BadTooManyPublishRequests)` when the queue is full
    ///   (OPC-10000-14 §9.1.10.1 processing-limit enforcement).
    /// - `Err(StatusCode::BadNoCommunication)` when the consumer has dropped
    ///   its receiver (e.g. the engine is shutting down).
    pub fn try_enqueue(&self, payload: Vec<u8>) -> Result<(), StatusCode> {
        self.tx.try_send(payload).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => StatusCode::BadTooManyPublishRequests,
            mpsc::error::TrySendError::Closed(_) => StatusCode::BadNoCommunication,
        })
    }

    /// Returns the configured queue capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns a clone of the underlying sender for transport tasks that own
    /// their own receive loops (e.g. the MQTT broker subscriber).
    ///
    /// Callers that use this raw sender must call `try_send` and treat
    /// `TrySendError::Full` as `StatusCode::BadTooManyPublishRequests` to
    /// honour OPC-10000-14 §9.1.10.1.
    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.tx.clone()
    }
}
