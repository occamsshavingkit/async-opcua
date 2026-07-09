#![allow(dead_code)]

#[cfg(feature = "subscriptions-standard")]
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crossbeam_queue::ArrayQueue;
use tokio::sync::{mpsc, oneshot, Notify};

use crate::node_manager::TypeTreeForUserStatic;

use super::{
    pool::NotificationBuffer, ring::NotificationWorkItem, PendingPublish, PersistentSessionKey,
    SessionSubscriptions, SubscriptionCleanup, NOTIFICATION_RING_CAPACITY, RING_DRAIN_BUDGET,
};
#[cfg(feature = "events")]
use super::{session_subscriptions::PendingRefreshDrain, RING_DRAIN_EVENT_CHUNK};
use crate::node_manager::MonitoredItemUpdateRef;
use crate::subscriptions::subscription::TickReason;
use crate::subscriptions::NonAckedPublish;
#[cfg(feature = "subscriptions-standard")]
use opcua_types::NodeId;
#[cfg(all(feature = "generated-address-space", feature = "diagnostics"))]
use opcua_types::SubscriptionDiagnosticsDataType;
use opcua_types::{
    CreateSubscriptionRequest, DataValue, DateTimeUtc, DiagnosticBits, MonitoredItemCreateResult,
    MonitoredItemModifyRequest, MonitoringMode, RepublishRequest, SetPublishingModeRequest,
    StatusCode, TimestampsToReturn,
};

use crate::{
    info::ServerInfo, node_manager::MonitoredItemRef, subscriptions::subscription::Subscription,
};
use opcua_core::RepublishResponseShared;
use std::time::{Duration, Instant};

/// Commands accepted by the per-session subscription actor.
#[allow(clippy::type_complexity)]
pub(crate) enum SubscriptionCommand {
    EnqueuePublish {
        now: DateTimeUtc,
        now_instant: Instant,
        request: PendingPublish,
        response: oneshot::Sender<()>,
    },
    /// Stop the actor after all earlier commands have been handled.
    Stop,

    // Management operations
    CreateSubscription {
        request: CreateSubscriptionRequest,
        info: Arc<ServerInfo>,
        response: oneshot::Sender<Result<opcua_types::CreateSubscriptionResponse, StatusCode>>,
    },
    ModifySubscription {
        request: opcua_types::ModifySubscriptionRequest,
        info: Arc<ServerInfo>,
        response: oneshot::Sender<Result<opcua_types::ModifySubscriptionResponse, StatusCode>>,
    },
    DeleteSubscriptions {
        ids: Vec<u32>,
        response: oneshot::Sender<Vec<(StatusCode, Vec<MonitoredItemRef>)>>,
    },
    SetPublishingModeRq {
        request: opcua_types::SetPublishingModeRequest,
        response: oneshot::Sender<Result<opcua_types::SetPublishingModeResponse, StatusCode>>,
    },
    RepublishRq {
        request: RepublishRequest,
        response: oneshot::Sender<Result<RepublishResponseShared, StatusCode>>,
    },

    // Monitored item operations
    CreateMonitoredItems {
        sub_id: u32,
        requests: Vec<crate::subscriptions::CreateMonitoredItem>,
        response: oneshot::Sender<Result<Vec<MonitoredItemCreateResult>, StatusCode>>,
    },
    ModifyMonitoredItems {
        sub_id: u32,
        info: Arc<ServerInfo>,
        timestamps_to_return: TimestampsToReturn,
        requests: Vec<MonitoredItemModifyRequest>,
        #[cfg(feature = "subscriptions-standard")]
        eu_ranges: HashMap<u32, (f64, f64)>,
        diagnostic_bits: DiagnosticBits,
        response: oneshot::Sender<Result<Vec<MonitoredItemUpdateRef>, StatusCode>>,
    },
    DeleteMonitoredItems {
        sub_id: u32,
        items: Vec<u32>,
        response: oneshot::Sender<Result<Vec<(StatusCode, MonitoredItemRef)>, StatusCode>>,
    },
    SetMonitoringMode {
        sub_id: u32,
        mode: MonitoringMode,
        items: Vec<u32>,
        response: oneshot::Sender<Result<Vec<(StatusCode, MonitoredItemRef)>, StatusCode>>,
    },
    #[cfg(feature = "subscriptions-standard")]
    SetTriggering {
        sub_id: u32,
        triggering_item_id: u32,
        links_to_add: Vec<u32>,
        links_to_remove: Vec<u32>,
        response: oneshot::Sender<Result<(Vec<StatusCode>, Vec<StatusCode>), StatusCode>>,
    },

    // Read-only queries
    SubscriptionIds {
        response: oneshot::Sender<Vec<u32>>,
    },
    MonitoredItemRefs {
        response: oneshot::Sender<Vec<MonitoredItemRef>>,
    },
    SubscriptionAndItemData {
        response: oneshot::Sender<(Vec<u32>, Vec<MonitoredItemRef>)>,
    },
    MonitoredItemCount {
        sub_id: u32,
        response: oneshot::Sender<Option<usize>>,
    },
    #[cfg(feature = "subscriptions-standard")]
    MonitoredItemNodeIds {
        sub_id: u32,
        ids: Vec<u32>,
        response: oneshot::Sender<Result<HashMap<u32, NodeId>, StatusCode>>,
    },
    AvailableSequenceNumbers {
        sub_id: u32,
        response: oneshot::Sender<Option<Vec<u32>>>,
    },
    #[cfg(all(feature = "generated-address-space", feature = "diagnostics"))]
    SubscriptionDiagnostics {
        response: oneshot::Sender<Vec<SubscriptionDiagnosticsDataType>>,
    },
    #[cfg(all(feature = "generated-address-space", feature = "diagnostics"))]
    SessionDiagnosticsSummary {
        response: oneshot::Sender<crate::subscriptions::SessionSubscriptionDiagnosticsSummary>,
    },

    // State mutation
    UpdateOwner {
        key: PersistentSessionKey,
        type_tree_for_user: Arc<dyn TypeTreeForUserStatic>,
        response: oneshot::Sender<()>,
    },
    ApplyRevalidatedValues {
        values: Vec<(MonitoredItemRef, DataValue)>,
        response: oneshot::Sender<()>,
    },
    MarkTransferring {
        sub_id: u32,
        response: oneshot::Sender<Result<(), StatusCode>>,
    },
    CloneForTransfer {
        sub_id: u32,
        response: oneshot::Sender<Option<(Subscription, Vec<NonAckedPublish>)>>,
    },
    InsertForTransfer {
        sub: Subscription,
        notifs: Vec<NonAckedPublish>,
        send_initial_values: bool,
        sub_id: u32,
        response: oneshot::Sender<Result<(), StatusCode>>,
    },
    UserTokenMatches {
        key: PersistentSessionKey,
        response: oneshot::Sender<bool>,
    },
    // Internal helpers for node manager
    GetSubscriptionItemIds {
        sub_id: u32,
        response: oneshot::Sender<Result<(Vec<u32>, Vec<u32>), StatusCode>>,
    },
    ResendData {
        sub_id: u32,
        response: oneshot::Sender<Result<(), StatusCode>>,
    },
    CompleteTransfer {
        sub_id: u32,
        next_seq: u32,
        timestamp: opcua_types::DateTime,
        response: oneshot::Sender<()>,
    },
    /// Run an arbitrary read against a session's subscriptions.
    /// Exists for integration tests that need deep access to
    /// subscription internals.
    WithSessionSubscriptions {
        session_id: u32,
        f: Box<dyn FnOnce(&mut SessionSubscriptions) + Send>,
        response: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
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

    pub(super) async fn enqueue_publish_request(
        &self,
        now: DateTimeUtc,
        now_instant: Instant,
        request: PendingPublish,
    ) -> Result<(), ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::EnqueuePublish {
                now,
                now_instant,
                request,
                response,
            })
            .map_err(|_| ())?;

        recv.await.map_err(|_| ())
    }

    pub(crate) fn stop(&self) {
        let _ = self.commands.send(SubscriptionCommand::Stop);
    }

    // --- Management operation send methods ---

    pub(crate) async fn create_subscription(
        &self,
        request: CreateSubscriptionRequest,
        info: Arc<ServerInfo>,
    ) -> Result<Result<opcua_types::CreateSubscriptionResponse, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::CreateSubscription {
                request,
                info,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn modify_subscription(
        &self,
        request: opcua_types::ModifySubscriptionRequest,
        info: Arc<ServerInfo>,
    ) -> Result<Result<opcua_types::ModifySubscriptionResponse, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::ModifySubscription {
                request,
                info,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn delete_subscriptions(
        &self,
        ids: Vec<u32>,
    ) -> Result<Vec<(StatusCode, Vec<MonitoredItemRef>)>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::DeleteSubscriptions { ids, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn set_publishing_mode(
        &self,
        request: SetPublishingModeRequest,
    ) -> Result<Result<opcua_types::SetPublishingModeResponse, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::SetPublishingModeRq { request, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn republish(
        &self,
        request: RepublishRequest,
    ) -> Result<Result<RepublishResponseShared, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::RepublishRq { request, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    // --- Monitored item operation send methods ---

    pub(crate) async fn create_monitored_items(
        &self,
        sub_id: u32,
        requests: Vec<crate::subscriptions::CreateMonitoredItem>,
    ) -> Result<Result<Vec<MonitoredItemCreateResult>, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::CreateMonitoredItems {
                sub_id,
                requests,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn modify_monitored_items(
        &self,
        sub_id: u32,
        info: Arc<ServerInfo>,
        timestamps_to_return: TimestampsToReturn,
        requests: Vec<MonitoredItemModifyRequest>,
        #[cfg(feature = "subscriptions-standard")] eu_ranges: HashMap<u32, (f64, f64)>,
        diagnostic_bits: DiagnosticBits,
    ) -> Result<Result<Vec<MonitoredItemUpdateRef>, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::ModifyMonitoredItems {
                sub_id,
                info,
                timestamps_to_return,
                requests,
                #[cfg(feature = "subscriptions-standard")]
                eu_ranges,
                diagnostic_bits,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn delete_monitored_items(
        &self,
        sub_id: u32,
        items: Vec<u32>,
    ) -> Result<Result<Vec<(StatusCode, MonitoredItemRef)>, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::DeleteMonitoredItems {
                sub_id,
                items,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn set_monitoring_mode(
        &self,
        sub_id: u32,
        mode: MonitoringMode,
        items: Vec<u32>,
    ) -> Result<Result<Vec<(StatusCode, MonitoredItemRef)>, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::SetMonitoringMode {
                sub_id,
                mode,
                items,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    #[cfg(feature = "subscriptions-standard")]
    pub(crate) async fn set_triggering(
        &self,
        sub_id: u32,
        triggering_item_id: u32,
        links_to_add: Vec<u32>,
        links_to_remove: Vec<u32>,
    ) -> Result<Result<(Vec<StatusCode>, Vec<StatusCode>), StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::SetTriggering {
                sub_id,
                triggering_item_id,
                links_to_add,
                links_to_remove,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    // --- Query send methods ---

    pub(crate) async fn subscription_ids(&self) -> Result<Vec<u32>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::SubscriptionIds { response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn monitored_item_refs(&self) -> Result<Vec<MonitoredItemRef>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::MonitoredItemRefs { response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn subscription_and_item_data(
        &self,
    ) -> Result<(Vec<u32>, Vec<MonitoredItemRef>), ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::SubscriptionAndItemData { response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn monitored_item_count(&self, sub_id: u32) -> Result<Option<usize>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::MonitoredItemCount { sub_id, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    #[cfg(feature = "subscriptions-standard")]
    pub(crate) async fn monitored_item_node_ids(
        &self,
        sub_id: u32,
        ids: Vec<u32>,
    ) -> Result<Result<HashMap<u32, NodeId>, StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::MonitoredItemNodeIds {
                sub_id,
                ids,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn available_sequence_numbers(
        &self,
        sub_id: u32,
    ) -> Result<Option<Vec<u32>>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::AvailableSequenceNumbers { sub_id, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    #[cfg(all(feature = "generated-address-space", feature = "diagnostics"))]
    pub(crate) async fn subscription_diagnostics(
        &self,
    ) -> Result<Vec<SubscriptionDiagnosticsDataType>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::SubscriptionDiagnostics { response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    // --- State mutation send methods ---

    pub(crate) async fn update_owner(
        &self,
        key: PersistentSessionKey,
        type_tree_for_user: Arc<dyn TypeTreeForUserStatic>,
    ) -> Result<(), ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::UpdateOwner {
                key,
                type_tree_for_user,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn apply_revalidated_values(
        &self,
        values: Vec<(MonitoredItemRef, DataValue)>,
    ) -> Result<(), ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::ApplyRevalidatedValues { values, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn mark_transferring(
        &self,
        sub_id: u32,
    ) -> Result<Result<(), StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::MarkTransferring { sub_id, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn clone_for_transfer(
        &self,
        sub_id: u32,
    ) -> Result<Option<(Subscription, Vec<NonAckedPublish>)>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::CloneForTransfer { sub_id, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn insert_for_transfer(
        &self,
        sub: Subscription,
        notifs: Vec<NonAckedPublish>,
        send_initial_values: bool,
        sub_id: u32,
    ) -> Result<Result<(), StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::InsertForTransfer {
                sub,
                notifs,
                send_initial_values,
                sub_id,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn user_token_matches(&self, key: PersistentSessionKey) -> Result<bool, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::UserTokenMatches { key, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn get_subscription_item_ids(
        &self,
        sub_id: u32,
    ) -> Result<Result<(Vec<u32>, Vec<u32>), StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::GetSubscriptionItemIds { sub_id, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn resend_data(&self, sub_id: u32) -> Result<Result<(), StatusCode>, ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::ResendData { sub_id, response })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn complete_transfer(
        &self,
        sub_id: u32,
        next_seq: u32,
        timestamp: opcua_types::DateTime,
    ) -> Result<(), ()> {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::CompleteTransfer {
                sub_id,
                next_seq,
                timestamp,
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }

    pub(crate) async fn with_session_subscriptions<F>(&self, f: F) -> Result<(), ()>
    where
        F: FnOnce(&mut SessionSubscriptions) + Send + 'static,
    {
        let (response, recv) = oneshot::channel();
        self.commands
            .send(SubscriptionCommand::WithSessionSubscriptions {
                session_id: 0,
                f: Box::new(f),
                response,
            })
            .map_err(|_| ())?;
        recv.await.map_err(|_| ())
    }
}

pub(crate) struct SubscriptionActor {
    session_id: u32,
    subs: SessionSubscriptions,
    ring: Arc<ArrayQueue<NotificationWorkItem>>,
    notify: Arc<Notify>,
    commands_rx: mpsc::UnboundedReceiver<SubscriptionCommand>,
    cleanup_tx: mpsc::UnboundedSender<SubscriptionCleanup>,
    /// Per-user TypeTree provider. The actor stores the provider, not a live read context,
    /// so default reads can use published snapshots and custom static getters stay in effect.
    type_tree_for_user: Arc<dyn TypeTreeForUserStatic>,
    #[cfg(feature = "events")]
    pending_refresh: Option<PendingRefreshDrain>,
}

impl SubscriptionActor {
    pub(crate) async fn run(mut self) {
        let sleep = tokio::time::sleep_until(Self::next_sleep_deadline(&self.subs));
        tokio::pin!(sleep);

        loop {
            sleep.as_mut().reset(Self::next_sleep_deadline(&self.subs));

            tokio::select! {
                command = self.commands_rx.recv() => {
                    match command {
                        // --- Management operations ---
                        Some(SubscriptionCommand::CreateSubscription { request, info, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.create_subscription(&request, &info);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::ModifySubscription { request, info, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.modify_subscription(&request, &info);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::DeleteSubscriptions { ids, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.delete_subscriptions(&ids);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::SetPublishingModeRq { request, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.set_publishing_mode(&request);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::RepublishRq { request, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.republish(&request);
                            let _ = response.send(result);
                        }

                        // --- Monitored item operations ---
                        Some(SubscriptionCommand::CreateMonitoredItems { sub_id, requests, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.create_monitored_items(sub_id, &requests);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::ModifyMonitoredItems {
                            sub_id,
                            info,
                            timestamps_to_return,
                            requests,
                            #[cfg(feature = "subscriptions-standard")]
                            eu_ranges,
                            diagnostic_bits,
                            response,
                        }) => {
                            self.drain_ring().await;
                            let type_tree_for_user = self.subs.type_tree_for_user();
                            let type_tree = type_tree_for_user.get_type_tree();
                            let result = self.subs.modify_monitored_items(
                                sub_id,
                                &info,
                                timestamps_to_return,
                                requests,
                                #[cfg(feature = "subscriptions-standard")]
                                eu_ranges.into_iter().collect(),
                                type_tree.get(),
                                diagnostic_bits,
                            );
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::DeleteMonitoredItems { sub_id, items, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.delete_monitored_items(sub_id, &items);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::SetMonitoringMode { sub_id, mode, items, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.set_monitoring_mode(sub_id, mode, items);
                            let _ = response.send(result);
                        }
                        #[cfg(feature = "subscriptions-standard")]
                        Some(SubscriptionCommand::SetTriggering { sub_id, triggering_item_id, links_to_add, links_to_remove, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.set_triggering(
                                sub_id,
                                triggering_item_id,
                                links_to_add,
                                links_to_remove,
                            );
                            let _ = response.send(result);
                        }

                        // --- Read-only queries ---
                        Some(SubscriptionCommand::SubscriptionIds { response }) => {
                            self.drain_ring().await;
                            let result = self.subs.subscription_ids();
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::MonitoredItemRefs { response }) => {
                            self.drain_ring().await;
                            let result = self.subs.monitored_item_refs();
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::SubscriptionAndItemData { response }) => {
                            self.drain_ring().await;
                            let result = (self.subs.subscription_ids(), self.subs.monitored_item_refs());
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::MonitoredItemCount { sub_id, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.get_monitored_item_count(sub_id);
                            let _ = response.send(result);
                        }
                        #[cfg(feature = "subscriptions-standard")]
                        Some(SubscriptionCommand::MonitoredItemNodeIds { sub_id, ids, response }) => {
                            self.drain_ring().await;
                            let result: Result<_, StatusCode> = self.subs.monitored_item_node_ids(sub_id, &ids).map(|m| m.into_iter().collect());
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::AvailableSequenceNumbers { sub_id, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.available_sequence_numbers(sub_id);
                            let _ = response.send(result);
                        }
                        #[cfg(all(feature = "generated-address-space", feature = "diagnostics"))]
                        Some(SubscriptionCommand::SubscriptionDiagnostics { response }) => {
                            self.drain_ring().await;
                            let result = crate::subscriptions::session_subscription_diagnostics(&mut self.subs);
                            let _ = response.send(result);
                        }
                        #[cfg(all(feature = "generated-address-space", feature = "diagnostics"))]
                        Some(SubscriptionCommand::SessionDiagnosticsSummary { response }) => {
                            self.drain_ring().await;
                            let result = crate::subscriptions::session_subscription_diagnostics_summary(&mut self.subs);
                            let _ = response.send(result);
                        }

                        // --- State mutation ---
                        Some(SubscriptionCommand::UpdateOwner { key, type_tree_for_user, response }) => {
                            self.drain_ring().await;
                            self.subs.update_owner(key, type_tree_for_user);
                            let _ = response.send(());
                        }
                        Some(SubscriptionCommand::ApplyRevalidatedValues { values, response }) => {
                            self.drain_ring().await;
                            self.subs.apply_revalidated_values(values);
                            let _ = response.send(());
                        }
                        Some(SubscriptionCommand::MarkTransferring { sub_id, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.mark_transferring(sub_id);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::CloneForTransfer { sub_id, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.clone_for_transfer(sub_id);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::InsertForTransfer { sub, notifs, send_initial_values, sub_id, response }) => {
                            self.drain_ring().await;
                            let result = (|| -> Result<(), StatusCode> {
                                if let Err((e, _, _)) = self.subs.insert(sub, notifs) {
                                    return Err(e);
                                }
                                if send_initial_values {
                                    if let Some(sub) = self.subs.get_mut(sub_id) {
                                        sub.set_resend_data();
                                    }
                                }
                                Ok(())
                            })();
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::UserTokenMatches { key, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.user_token().is_equivalent_for_transfer(&key);
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::GetSubscriptionItemIds { sub_id, response }) => {
                            self.drain_ring().await;
                            let result = self.subs.get(sub_id).map_or(
                                Err(StatusCode::BadSubscriptionIdInvalid),
                                |sub| {
                                    let (ids, handles): (Vec<_>, Vec<_>) =
                                        sub.items().map(|i| (i.id(), i.client_handle())).unzip();
                                    Ok((ids, handles))
                                },
                            );
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::ResendData { sub_id, response }) => {
                            self.drain_ring().await;
                            let result = self.subs
                                .get_mut(sub_id)
                                .map(|sub| {
                                    sub.set_resend_data();
                                    Ok(())
                                })
                                .unwrap_or(Err(StatusCode::BadSubscriptionIdInvalid));
                            let _ = response.send(result);
                        }
                        Some(SubscriptionCommand::CompleteTransfer { sub_id, next_seq, timestamp, response }) => {
                            self.drain_ring().await;
                            let _ = self.subs.remove(sub_id);
                            self.subs.queue_status_change(
                                sub_id,
                                next_seq,
                                timestamp,
                                StatusCode::GoodSubscriptionTransferred,
                            );
                            let _ = response.send(());
                        }

                        Some(SubscriptionCommand::EnqueuePublish { now, now_instant, request, response }) => {
                            self.drain_ring().await;
                            let mut buffer = NotificationBuffer::new();
                            self.subs.enqueue_publish_request(&now, now_instant, request);
                            loop {
                                let more_notifications = self.subs.has_more_notifications()
                                    && self.subs.has_queued_publish_request();
                                if !more_notifications {
                                    break;
                                }
                                let _ = self.subs.tick(
                                    &now,
                                    now_instant,
                                    TickReason::ReceivePublishRequest,
                                    &mut buffer,
                                );
                            }
                            let _ = response.send(());
                        }
                        Some(SubscriptionCommand::WithSessionSubscriptions { f, response, .. }) => {
                            self.drain_ring().await;
                            f(&mut self.subs);
                            let _ = response.send(());
                        }
                        Some(SubscriptionCommand::Stop) | None => break,
                    }
                }
                _ = self.notify.notified() => {
                    self.drain_ring().await;
                    let now = DateTimeUtc::from(chrono::Utc::now());
                    let now_instant = std::time::Instant::now();
                    let mut buffer = NotificationBuffer::new();
                    self.subs.process_notification_wake(
                        &now,
                        now_instant,
                        &mut buffer,
                    );
                }
                _ = &mut sleep => {
                    self.drain_ring().await;
                    let now = DateTimeUtc::from(chrono::Utc::now());
                    let now_instant = std::time::Instant::now();
                    let mut buffer = NotificationBuffer::new();
                    let removed = self.subs.tick(
                        &now,
                        now_instant,
                        TickReason::TickTimerFired,
                        &mut buffer,
                    );
                    let ready = self.subs.is_ready_to_delete();
                    if !removed.is_empty() || ready {
                        let session = (!removed.is_empty()).then(|| self.subs.session().clone());
                        let _ = self.cleanup_tx.send(SubscriptionCleanup {
                            session_id: self.session_id,
                            session,
                            removed_subscriptions: removed,
                            ready_to_delete: ready,
                        });
                    }
                }
            }
        }
    }

    fn next_sleep_deadline(subs: &SessionSubscriptions) -> tokio::time::Instant {
        subs.next_tick_deadline()
            .map(tokio::time::Instant::from_std)
            .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(3600))
    }

    async fn drain_ring(&mut self) {
        let mut processed = 0;
        loop {
            if processed >= RING_DRAIN_BUDGET {
                tokio::task::yield_now().await;
                processed = 0;
            }

            let drained = {
                #[cfg(feature = "events")]
                {
                    // Keep the read context scoped to this synchronous drain chunk.
                    let type_tree = self.type_tree_for_user.get_type_tree();
                    self.subs.drain_ring_chunk(
                        self.ring.as_ref(),
                        type_tree.get(),
                        RING_DRAIN_EVENT_CHUNK,
                        &mut self.pending_refresh,
                    )
                }
                #[cfg(not(feature = "events"))]
                {
                    self.subs.drain_ring_chunk(self.ring.as_ref())
                }
            };

            #[cfg(feature = "events")]
            let completed_refresh = self
                .pending_refresh
                .as_ref()
                .is_some_and(PendingRefreshDrain::is_complete);
            #[cfg(feature = "events")]
            if completed_refresh {
                self.pending_refresh = None;
            }

            if drained == 0 {
                break;
            }

            processed += drained;
            tokio::task::yield_now().await;
        }
    }
}

pub(crate) fn spawn(
    session_id: u32,
    subs: SessionSubscriptions,
    type_tree_for_user: Arc<dyn TypeTreeForUserStatic>,
    cleanup_tx: mpsc::UnboundedSender<SubscriptionCleanup>,
) -> SubscriptionActorHandle {
    assert_actor_send_bounds();

    let ring = Arc::new(ArrayQueue::new(NOTIFICATION_RING_CAPACITY));
    let notify = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicU64::new(0));
    let (commands, commands_rx) = mpsc::unbounded_channel();

    tokio::spawn(
        SubscriptionActor {
            session_id,
            subs,
            ring: Arc::clone(&ring),
            notify: Arc::clone(&notify),
            commands_rx,
            cleanup_tx,
            type_tree_for_user,
            #[cfg(feature = "events")]
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
