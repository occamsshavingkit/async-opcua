//! MQTT broker reconnection and cache-drain runtime extracted from the parent
//! transport module to keep `mqtt.rs` within reviewable size.
use std::time::Duration;

use rumqttc::{AsyncClient, MqttOptions, QoS};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use super::{lock_cache, MessageCache};

/// Drains the publisher cache against an MQTT broker with reconnection backoff.
///
/// Each cached enqueue is interleaved with `event_loop.poll()` via `tokio::select!`
/// so neither side can starve the other. Any popped item that fails to enqueue is
/// restored to the cache front, preserving message order across reconnects.
pub(super) async fn run_transport_loop(
    cancel_token: &CancellationToken,
    host: &str,
    port: u16,
    cache: &MessageCache,
) {
    let mut backoff = Duration::from_secs(1);

    loop {
        if cancel_token.is_cancelled() {
            break;
        }

        let client_id = format!("opcua-publisher-{}", uuid::Uuid::new_v4());
        let mut options = MqttOptions::new(client_id, host, port);
        options.set_keep_alive(Duration::from_secs(5));

        let (client, mut event_loop) = AsyncClient::new(options, 50);

        // Background loop draining the cache and polling MQTT
        loop {
            if cancel_token.is_cancelled() {
                return;
            }

            // Attempt to publish one item from cache
            let mut next_item = None;
            {
                let mut cache_lock = lock_cache(cache);
                if let Some((topic, payload)) = cache_lock.pop_front() {
                    next_item = Some((topic, payload));
                }
            }

            if let Some((topic, payload)) = next_item {
                // Poll the event loop alongside each cached enqueue so neither can starve
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        let mut cache_lock = lock_cache(cache);
                        cache_lock.push_front((topic, payload));
                        return;
                    }
                    result = client.publish(
                        topic.clone(),
                        QoS::AtLeastOnce,
                        false,
                        payload.clone(),
                    ) => {
                        if result.is_err() {
                            {
                                let mut cache_lock = lock_cache(cache);
                                cache_lock.push_front((topic, payload));
                            }
                            wait_for_reconnect(cancel_token, &mut backoff).await;
                            break;
                        }
                        continue;
                    }
                    result = event_loop.poll() => {
                        {
                            let mut cache_lock = lock_cache(cache);
                            cache_lock.push_front((topic, payload));
                        }

                        match result {
                            Ok(_) => {
                                backoff = Duration::from_secs(1);
                                continue;
                            }
                            Err(_) => {
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
                        Ok(_) => {
                            // Successful communication, reset backoff
                            backoff = Duration::from_secs(1);
                        }
                        Err(_) => {
                            // Connection lost, sleep and reconnect
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

async fn wait_for_reconnect(cancel_token: &CancellationToken, backoff: &mut Duration) {
    tokio::select! {
        _ = cancel_token.cancelled() => {}
        _ = sleep(*backoff) => {
            *backoff = std::cmp::min(*backoff * 2, Duration::from_secs(60));
        }
    }
}
