use tokio::sync::mpsc;

use opcua_types::StatusCode;

/// Default PubSub datagram queue capacity.
///
/// `OperationalLimits` (Part 4) only covers service-call bounds, so the
/// PubSub datagram bound is a crate-level constant. Override per engine with
/// [`crate::PubSubEngine::set_datagram_queue_capacity`].
pub const PUBSUB_DATAGRAM_QUEUE_CAPACITY: usize = 1024;

/// Bounded internal queue for incoming PubSub datagrams.
///
/// Caps the memory used by received PubSub NetworkMessages waiting for the
/// subscriber runtime to process them. When the queue is full (the subscriber
/// cannot keep up), [`DatagramQueue::try_enqueue`] returns
/// `StatusCode::BadResourceUnavailable` and the caller drops the datagram
/// rather than accumulating unbounded backpressure. This is an internal
/// processing limit, not an OPC UA service status: Part 14 reports persistent
/// subscriber overload through the DataSetReader `PubSubState` (§6.2.1, an
/// `Error` transition) and `PubSubCommunicationFailureEventType` (§9.1.13.3),
/// not through a StatusCode on the receive path.
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
    /// - `Err(StatusCode::BadResourceUnavailable)` when the queue is full
    ///   (internal processing limit; the caller drops the datagram).
    /// - `Err(StatusCode::BadNoCommunication)` when the consumer has dropped
    ///   its receiver (e.g. the engine is shutting down).
    pub fn try_enqueue(&self, payload: Vec<u8>) -> Result<(), StatusCode> {
        self.tx.try_send(payload).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => StatusCode::BadResourceUnavailable,
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
    /// `TrySendError::Full` as `StatusCode::BadResourceUnavailable` (an
    /// internal processing limit) and drop the datagram.
    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.tx.clone()
    }
}
