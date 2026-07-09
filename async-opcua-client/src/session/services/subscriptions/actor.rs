//! Client subscription state actor (US9 — P2 actor migration).

use crate::session::services::subscriptions::Subscription;
use opcua_types::SubscriptionAcknowledgement;
use std::time::Duration;

use super::state::SubscriptionState;

pub(crate) enum SubscriptionActorMessage {
    AddSubscription {
        subscription: Subscription,
        response: tokio::sync::oneshot::Sender<()>,
    },
    DeleteSubscription {
        id: u32,
        response: tokio::sync::oneshot::Sender<Option<Subscription>>,
    },
    ModifySubscription {
        subscription_id: u32,
        publishing_interval: Duration,
        lifetime_count: u32,
        max_keep_alive_count: u32,
        max_notifications_per_publish: u32,
        priority: u8,
        response: tokio::sync::oneshot::Sender<()>,
    },
    TakeAcknowledgements {
        response: tokio::sync::oneshot::Sender<Vec<SubscriptionAcknowledgement>>,
    },
    SetPublishingMode {
        subscription_id: u32,
        publishing_enabled: bool,
        response: tokio::sync::oneshot::Sender<()>,
    },
}

pub(crate) struct SubscriptionActor {
    state: SubscriptionState,
    rx: tokio::sync::mpsc::UnboundedReceiver<SubscriptionActorMessage>,
}

impl SubscriptionActor {
    pub(crate) fn spawn(
        state: SubscriptionState,
    ) -> tokio::sync::mpsc::UnboundedSender<SubscriptionActorMessage> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move { SubscriptionActor { state, rx }.run().await });
        tx
    }

    async fn run(&mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                SubscriptionActorMessage::AddSubscription {
                    subscription,
                    response,
                } => {
                    self.state.add_subscription(subscription);
                    let _ = response.send(());
                }
                SubscriptionActorMessage::DeleteSubscription { id, response } => {
                    let removed = self.state.delete_subscription(id);
                    let _ = response.send(removed);
                }
                SubscriptionActorMessage::ModifySubscription {
                    subscription_id,
                    publishing_interval,
                    lifetime_count,
                    max_keep_alive_count,
                    max_notifications_per_publish,
                    priority,
                    response,
                } => {
                    self.state.modify_subscription(
                        subscription_id,
                        publishing_interval,
                        lifetime_count,
                        max_keep_alive_count,
                        max_notifications_per_publish,
                        priority,
                    );
                    let _ = response.send(());
                }
                SubscriptionActorMessage::TakeAcknowledgements { response } => {
                    let acks = self.state.take_acknowledgements();
                    let _ = response.send(acks);
                }
                SubscriptionActorMessage::SetPublishingMode {
                    subscription_id,
                    publishing_enabled,
                    response,
                } => {
                    self.state
                        .set_publishing_mode(&[subscription_id], publishing_enabled);
                    let _ = response.send(());
                }
            }
        }
    }
}
