use tokio::sync::mpsc::{error::TrySendError, Sender};

pub(super) struct PayloadForwarder {
    sender: Sender<Vec<u8>>,
}

impl PayloadForwarder {
    pub(super) fn new(sender: Sender<Vec<u8>>) -> Self {
        Self { sender }
    }

    pub(super) fn forward(&self, payload: Vec<u8>) -> Result<(), TrySendError<Vec<u8>>> {
        self.sender.try_send(payload)
    }
}
