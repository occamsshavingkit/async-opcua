use std::sync::Arc;

use opcua_core::handle::AtomicHandle;

pub(crate) struct SecureChannelState {
    pub(crate) issued: bool,
    pub(crate) renew_count: usize,
    secure_channel_id: Arc<AtomicHandle>,
    last_token_id: u32,
}

impl SecureChannelState {
    pub(crate) fn new(handle: Arc<AtomicHandle>) -> SecureChannelState {
        SecureChannelState {
            secure_channel_id: handle,
            issued: false,
            renew_count: 0,
            last_token_id: 0,
        }
    }

    pub(crate) fn create_secure_channel_id(&mut self) -> u32 {
        self.secure_channel_id.next()
    }

    pub(crate) fn create_token_id(&mut self) -> u32 {
        self.last_token_id += 1;
        self.last_token_id
    }
}
