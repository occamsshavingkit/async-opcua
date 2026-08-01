//! MQTT broker reconnection and cache-drain runtime extracted from the parent
//! transport module to keep `mqtt.rs` within reviewable size.
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Outgoing, QoS};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::transport::wait_for_reconnect;

use super::{lock_cache, MessageCache, MqttBrokerAddress, MAX_CACHE_SIZE};

pub(super) const REQUEST_CHANNEL_CAPACITY: usize = 50;

type PublishMessage = (String, Vec<u8>);

pub(super) struct PublishTaskState {
    cache: MessageCache,
    current: Option<PublishMessage>,
    pending: VecDeque<PublishMessage>,
    in_flight: HashMap<u16, PublishMessage>,
    in_flight_order: VecDeque<u16>,
}

impl PublishTaskState {
    pub(super) fn new(cache: MessageCache) -> Self {
        Self {
            cache,
            current: None,
            pending: VecDeque::new(),
            in_flight: HashMap::new(),
            in_flight_order: VecDeque::new(),
        }
    }

    fn load_current(&mut self) {
        if self.current.is_none() {
            self.current = lock_cache(&self.cache).pop_front();
        }
    }

    fn enqueue_current(&mut self) {
        if let Some(message) = self.current.take() {
            self.pending.push_back(message);
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Outgoing(Outgoing::Publish(packet_identifier)) => {
                if self.in_flight.contains_key(&packet_identifier) {
                    return;
                }
                if let Some(message) = self.pending.pop_front() {
                    self.in_flight.insert(packet_identifier, message);
                    self.in_flight_order.push_back(packet_identifier);
                }
            }
            Event::Incoming(Incoming::PubAck(acknowledgement)) => {
                if self.in_flight.remove(&acknowledgement.pkid).is_some() {
                    self.in_flight_order
                        .retain(|packet_identifier| *packet_identifier != acknowledgement.pkid);
                }
            }
            Event::Incoming(_) | Event::Outgoing(_) => {}
        }
    }

    pub(super) fn restore(&mut self) {
        let mut restored = VecDeque::with_capacity(MAX_CACHE_SIZE);
        while let Some(packet_identifier) = self.in_flight_order.pop_front() {
            if let Some(message) = self.in_flight.remove(&packet_identifier) {
                Self::retain_within_budget(&mut restored, message);
            }
        }
        self.in_flight.clear();
        while let Some(message) = self.pending.pop_front() {
            Self::retain_within_budget(&mut restored, message);
        }
        if let Some(message) = self.current.take() {
            Self::retain_within_budget(&mut restored, message);
        }

        let mut queued = lock_cache(&self.cache);
        while let Some(message) = queued.pop_front() {
            Self::retain_within_budget(&mut restored, message);
        }
        *queued = restored;
    }

    fn retain_within_budget(restored: &mut VecDeque<PublishMessage>, message: PublishMessage) {
        if restored.len() < MAX_CACHE_SIZE {
            restored.push_back(message);
        }
    }
}

/// Drains the publisher cache against an MQTT broker with reconnection backoff.
///
/// Each cached enqueue is interleaved with `event_loop.poll()` via `tokio::select!`
/// so neither side can starve the other. Enqueued QoS1 messages remain locally
/// owned until their matching PUBACK and are restored in order across reconnects.
pub(super) async fn run_transport_loop(
    cancel_token: &CancellationToken,
    broker_address: &MqttBrokerAddress,
    state: &mut PublishTaskState,
) {
    let mut backoff = Duration::from_secs(1);

    loop {
        if cancel_token.is_cancelled() {
            break;
        }

        let client_id = format!("opcua-publisher-{}", uuid::Uuid::new_v4());
        let mut options = MqttOptions::new(client_id, broker_address.host(), broker_address.port());
        options.set_keep_alive(Duration::from_secs(5));

        let (client, mut event_loop) = AsyncClient::new(options, REQUEST_CHANNEL_CAPACITY);

        // Background loop draining the cache and polling MQTT
        loop {
            if cancel_token.is_cancelled() {
                return;
            }

            // Attempt to publish one item from cache
            state.load_current();

            if let Some((topic, payload)) = state.current.as_ref() {
                let publish =
                    client.publish(topic.clone(), QoS::AtLeastOnce, false, payload.clone());
                // Poll the event loop alongside each cached enqueue so neither can starve
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        return;
                    }
                    result = publish => {
                        match result {
                            Ok(()) => {
                                state.enqueue_current();
                                continue;
                            }
                            Err(_) => {
                                state.restore();
                                wait_for_reconnect(cancel_token, &mut backoff).await;
                                break;
                            }
                        }
                    }
                    result = event_loop.poll() => {
                        match result {
                            Ok(event) => {
                                state.handle_event(event);
                                backoff = Duration::from_secs(1);
                                continue;
                            }
                            Err(_) => {
                                state.restore();
                                wait_for_reconnect(cancel_token, &mut backoff).await;
                                break;
                            }
                        }
                    }
                }
            }

            // Cache is empty, poll the event loop to keep connection alive
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    return;
                }
                res = event_loop.poll() => {
                    match res {
                        Ok(event) => {
                            state.handle_event(event);
                            // Successful communication, reset backoff
                            backoff = Duration::from_secs(1);
                        }
                        Err(_) => {
                            // Connection lost, sleep and reconnect
                            state.restore();
                            wait_for_reconnect(cancel_token, &mut backoff).await;
                            break;
                        }
                    }
                }
                _ = sleep(Duration::from_millis(20)) => {
                    // Wake up to check cache again
                }
            }
        }
    }
}
