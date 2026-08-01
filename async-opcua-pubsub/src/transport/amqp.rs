use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;

mod address;
mod publisher;
#[cfg(test)]
mod tests;

pub(crate) use address::parse_amqp_address;

const MAX_CACHE_SIZE: usize = 1000;
const DEFAULT_AMQP_PORT: u16 = 5672;
const DEFAULT_ROUTING_KEY: &str = "opcua.telemetry";
const DEFAULT_EXCHANGE: &str = "";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AmqpAddressSettings {
    broker_url: String,
    routing_key: String,
}

/// Cache of pending (routing key, payload) messages awaiting (re)publication.
type MessageCache = Arc<Mutex<VecDeque<(String, Vec<u8>)>>>;

fn lock_cache(cache: &MessageCache) -> std::sync::MutexGuard<'_, VecDeque<(String, Vec<u8>)>> {
    match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn push_cached_message(cache: &MessageCache, routing_key: String, payload: Vec<u8>) {
    let mut cache = lock_cache(cache);
    if cache.len() >= MAX_CACHE_SIZE {
        let _ = cache.pop_front();
    }
    cache.push_back((routing_key, payload));
}

/// AMQP implementation of `PubSubPublisher` with reconnection, backoff, and local cache.
pub struct AmqpPublisher {
    address_space: Arc<RwLock<AddressSpace>>,
    cache: MessageCache,
}

impl AmqpPublisher {
    /// Creates a new `AmqpPublisher` with the given AddressSpace reference.
    pub fn new(address_space: Arc<RwLock<AddressSpace>>) -> Self {
        Self {
            address_space,
            cache: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Instantly queues a message in the local bounded cache.
    pub fn publish_immediate(&self, routing_key: String, payload: Vec<u8>) {
        push_cached_message(&self.cache, routing_key, payload);
    }

    #[doc(hidden)]
    pub fn cached_message_count(&self) -> usize {
        lock_cache(&self.cache).len()
    }

    #[doc(hidden)]
    pub fn pop_cached_message(&self) -> Option<(String, Vec<u8>)> {
        lock_cache(&self.cache).pop_front()
    }
}
