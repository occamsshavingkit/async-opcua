#![allow(dead_code)]

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crossbeam_queue::ArrayQueue;
use tokio::sync::{mpsc, oneshot, Notify};

use crate::node_manager::TypeTreeForUserStatic;

use super::{
    ring::NotificationWorkItem, session_subscriptions::PendingRefreshDrain, SessionSubscriptions,
    NOTIFICATION_RING_CAPACITY, RING_DRAIN_EVENT_CHUNK,
};

/// Commands accepted by the per-session subscription actor.
pub(crate) enum SubscriptionCommand {
    /// Run a closure against the actor-owned [`SessionSubscriptions`].
    ///
    /// Typed results are returned by having the closure capture its own
    /// [`oneshot::Sender<R>`] and send the result before returning. The command
    /// itself intentionally remains non-generic so it can live in one channel.
    LegacyCall(Box<dyn FnOnce(&mut SessionSubscriptions) + Send>),
    /// Stop the actor after all earlier commands have been handled.
    Stop,
}

pub(crate) struct SubscriptionActorHandle {
    ring: Arc<ArrayQueue<NotificationWorkItem>>,
    notify: Arc<Notify>,
    commands: mpsc::UnboundedSender<SubscriptionCommand>,
    dropped: Arc<AtomicU64>,
}

impl SubscriptionActorHandle {
    pub(crate) fn push_notification(&self, item: NotificationWorkItem) {
        if self.ring.push(item).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        self.notify.notify_one();
    }

    pub(crate) async fn legacy<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut SessionSubscriptions) -> R + Send + 'static,
    ) -> Result<R, ()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::LegacyCall(Box::new(move |subs| {
                let _ = reply_tx.send(f(subs));
            })))
            .map_err(|_| ())?;

        reply_rx.await.map_err(|_| ())
    }
}

pub(crate) struct SubscriptionActor {
    subs: SessionSubscriptions,
    ring: Arc<ArrayQueue<NotificationWorkItem>>,
    notify: Arc<Notify>,
    commands_rx: mpsc::UnboundedReceiver<SubscriptionCommand>,
    type_tree: Arc<dyn TypeTreeForUserStatic>,
    pending_refresh: Option<PendingRefreshDrain>,
}

impl SubscriptionActor {
    pub(crate) async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.commands_rx.recv() => {
                    match command {
                        Some(SubscriptionCommand::LegacyCall(f)) => f(&mut self.subs),
                        Some(SubscriptionCommand::Stop) | None => break,
                    }
                }
                _ = self.notify.notified() => {
                    self.drain_ring().await;
                }
            }
        }
    }

    async fn drain_ring(&mut self) {
        loop {
            let drained = {
                let type_tree = self.type_tree.get_type_tree();
                self.subs.drain_ring_chunk(
                    self.ring.as_ref(),
                    type_tree.get(),
                    RING_DRAIN_EVENT_CHUNK,
                    &mut self.pending_refresh,
                )
            };

            let completed_refresh = self
                .pending_refresh
                .as_ref()
                .is_some_and(PendingRefreshDrain::is_complete);
            if completed_refresh {
                self.pending_refresh = None;
            }

            if drained == 0 {
                break;
            }

            tokio::task::yield_now().await;
        }
    }
}

pub(crate) fn spawn(
    subs: SessionSubscriptions,
    type_tree: Arc<dyn TypeTreeForUserStatic>,
) -> SubscriptionActorHandle {
    assert_actor_send_bounds();

    let ring = Arc::new(ArrayQueue::new(NOTIFICATION_RING_CAPACITY));
    let notify = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicU64::new(0));
    let (commands, commands_rx) = mpsc::unbounded_channel();

    tokio::spawn(
        SubscriptionActor {
            subs,
            ring: Arc::clone(&ring),
            notify: Arc::clone(&notify),
            commands_rx,
            type_tree,
            pending_refresh: None,
        }
        .run(),
    );

    SubscriptionActorHandle {
        ring,
        notify,
        commands,
        dropped,
    }
}

fn assert_actor_send_bounds() {
    fn assert_send<T: Send>() {}

    assert_send::<SessionSubscriptions>();
}
