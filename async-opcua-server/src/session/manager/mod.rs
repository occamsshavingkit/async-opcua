use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use dashmap::DashMap;
use futures::FutureExt;
use opcua_core::{comms::secure_channel::SecureChannel, trace_read_lock};
use parking_lot::RwLock;
use tokio::sync::{mpsc, Notify};

#[cfg(feature = "fota")]
use crate::fota::cleanup::cleanup_session;
#[cfg(feature = "subscriptions")]
use crate::subscriptions::SubscriptionCache;
use crate::{
    authenticator::UserToken,
    config::ANONYMOUS_USER_TOKEN_ID,
    info::ServerInfo,
    node_manager::{NodeManagers, RequestContext, RequestContextInner},
};
use opcua_types::{NodeId, StatusCode, UAString};

use super::{
    actor::{SessionActor, SessionMessage},
    audit,
    instance::Session,
};

mod expiry;
mod lifecycle;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use lifecycle::{activate_session, close_session};
pub(crate) use types::CreateSessionDraft;

const SESSION_ACTOR_QUEUE_CAPACITY: usize = 256;
const CLOSED_SESSION_TOKEN_TOMBSTONE_SECS: u64 = 300;

use types::{SessionExpiryEntry, UnactivatedSessionEntry};

// Per-server session-id allocation + per-session locale map live on `ServerInfo`
// (feature 049) so independent servers have their own session-id space and locale
// state. The counter keeps ids unique WITHIN a server, so the map does not collide.
pub(super) fn next_session_id(info: &ServerInfo) -> (NodeId, u32) {
    // Session id will be a string identifier
    let session_id = info.next_session_id.fetch_add(1, Ordering::Relaxed);
    (NodeId::new(1, session_id), session_id)
}

pub(crate) fn locale_ids_for_session(info: &ServerInfo, session_id: u32) -> Option<Vec<UAString>> {
    info.session_locale_ids
        .get(&session_id)
        .map(|entry| entry.value().clone())
}

pub(super) fn set_session_locale_ids(
    info: &ServerInfo,
    session_id: u32,
    locale_ids: &Option<Vec<UAString>>,
) {
    match locale_ids {
        Some(locale_ids) if !locale_ids.is_empty() => {
            info.session_locale_ids
                .insert(session_id, locale_ids.clone());
        }
        _ => {
            clear_session_locale_ids(info, session_id);
        }
    }
}

pub(super) fn clear_session_locale_ids(info: &ServerInfo, session_id: u32) {
    info.session_locale_ids.remove(&session_id);
}

pub(super) fn clear_session_locale_ids_for_node_id(info: &ServerInfo, session_id: &NodeId) {
    if let opcua_types::Identifier::Numeric(id) = &session_id.identifier {
        clear_session_locale_ids(info, *id);
    }
}

pub(crate) fn normalized_locale_id(locale_id: &str) -> String {
    locale_id.trim().replace('_', "-").to_ascii_lowercase()
}

pub(crate) fn locale_id_matches(supported: &str, requested: &str) -> bool {
    let supported = normalized_locale_id(supported);
    let requested = normalized_locale_id(requested);

    if supported.is_empty() || requested.is_empty() {
        return requested.is_empty();
    }
    if supported == requested {
        return true;
    }

    let supported_is_neutral = !supported.contains('-');
    let requested_is_neutral = !requested.contains('-');

    (supported_is_neutral && requested.starts_with(&format!("{supported}-")))
        || (requested_is_neutral && supported.starts_with(&format!("{requested}-")))
}

pub(crate) fn is_special_write_locale_id(locale_id: &str) -> bool {
    matches!(
        normalized_locale_id(locale_id).split('-').next(),
        Some("mul" | "qst")
    )
}

/// Manages all sessions on the server.
pub struct SessionManager {
    sessions: HashMap<NodeId, Arc<RwLock<Session>>>,
    /// O(1) lock-free lookup from authentication token to session,
    /// avoiding a linear scan of `sessions` on every request.
    auth_tokens: Arc<DashMap<NodeId, Arc<RwLock<Session>>>>,
    /// Lock-free lookup from authentication token to the session actor's
    /// message queue.
    actor_senders: Arc<DashMap<NodeId, mpsc::Sender<SessionMessage>>>,
    closed_auth_tokens: Arc<DashMap<NodeId, Instant>>,
    info: Arc<ServerInfo>,
    notify: Arc<Notify>,
    /// Per-secure-channel count of unactivated sessions (OPC 10000-4 §5.7.2).
    /// Replaces the O(sessions) linear scan in CreateSession.
    unactivated_by_channel: HashMap<u32, AtomicUsize>,
    /// Cached per-channel max response body size for O(1) refresh.
    channel_body_limits: DashMap<u32, u32>,
    /// Min-heap of session deadlines for O(log n) expiry checks.
    expiry_heap: parking_lot::Mutex<BinaryHeap<Reverse<SessionExpiryEntry>>>,
    /// Min-heap of unactivated sessions ordered by creation time for O(log n) eviction.
    unactivated_heap: parking_lot::Mutex<BinaryHeap<Reverse<UnactivatedSessionEntry>>>,
}

impl SessionManager {
    /// Create a session manager for the supplied server information and expiry notifier.
    pub fn new(info: Arc<ServerInfo>, notify: Arc<Notify>) -> Self {
        Self {
            sessions: Default::default(),
            auth_tokens: Default::default(),
            actor_senders: Default::default(),
            closed_auth_tokens: Default::default(),
            unactivated_by_channel: HashMap::new(),
            channel_body_limits: DashMap::new(),
            expiry_heap: parking_lot::Mutex::new(BinaryHeap::new()),
            unactivated_heap: parking_lot::Mutex::new(BinaryHeap::new()),
            info,
            notify,
        }
    }

    /// Get a session by its authentication token.
    pub fn find_by_token(&self, authentication_token: &NodeId) -> Option<Arc<RwLock<Session>>> {
        let lookup_start = Instant::now();
        let session = self
            .auth_tokens
            .get(authentication_token)
            .map(|session| Arc::clone(session.value()));
        let lookup_duration_ns = lookup_start.elapsed().as_nanos() as u64;

        self.info
            .metrics
            .session_lookup_count
            .fetch_add(1, Ordering::Relaxed);
        self.info
            .metrics
            .session_lookup_duration_ns
            .fetch_add(lookup_duration_ns, Ordering::Relaxed);

        session
    }

    /// Return a snapshot of all live sessions.
    pub fn snapshot_sessions(&self) -> Vec<Arc<RwLock<Session>>> {
        self.sessions.values().map(Arc::clone).collect()
    }

    /// Register an authentication token for direct session lookup.
    pub fn register_token(&self, token: NodeId, session: Arc<RwLock<Session>>) {
        self.closed_auth_tokens.remove(&token);
        self.auth_tokens.insert(token, session);
    }

    /// Remove an authentication token from the direct session lookup registry.
    pub fn deregister_token(&self, token: &NodeId) {
        self.actor_senders.remove(token);
        self.auth_tokens.remove(token);
        self.remember_closed_token(token.clone());
    }

    /// Return true if the token belonged to a recently closed session.
    pub fn is_closed_token(&self, token: &NodeId) -> bool {
        self.prune_closed_tokens();
        self.closed_auth_tokens.contains_key(token)
    }

    fn remember_closed_token(&self, token: NodeId) {
        self.prune_closed_tokens();
        self.closed_auth_tokens.insert(token, Instant::now());
    }

    fn prune_closed_tokens(&self) {
        let cutoff = Instant::now() - Duration::from_secs(CLOSED_SESSION_TOKEN_TOMBSTONE_SECS);
        self.closed_auth_tokens
            .retain(|_, closed_at| *closed_at >= cutoff);
    }

    pub(crate) fn actor_sender(
        &self,
        authentication_token: &NodeId,
    ) -> Option<mpsc::Sender<SessionMessage>> {
        self.actor_senders
            .get(authentication_token)
            .map(|sender| sender.value().clone())
    }

    fn register_actor_sender(
        &self,
        authentication_token: NodeId,
        sender: mpsc::Sender<SessionMessage>,
    ) {
        self.actor_senders.insert(authentication_token, sender);
    }

    fn refresh_client_response_body_limit_for_channel(&self, channel: &mut SecureChannel) {
        let secure_channel_id = channel.secure_channel_id();
        if secure_channel_id == 0 {
            return;
        }

        let effective_limit = if let Some(entry) = self.channel_body_limits.get(&secure_channel_id)
        {
            *entry.value()
        } else {
            // Fallback: compute from sessions (rare — only on initial cache miss)
            let limit = self
                .sessions
                .values()
                .filter_map(|session| {
                    let session = trace_read_lock!(session);
                    let is_closed = matches!(
                        session.validate_activated(),
                        Err(StatusCode::BadSessionClosed)
                    );
                    if session.secure_channel_id() == secure_channel_id && !is_closed {
                        let limit = session.max_response_message_size();
                        (limit > 0).then_some(limit)
                    } else {
                        None
                    }
                })
                .min();
            limit.unwrap_or(0)
        };

        channel.set_client_response_body_limit(effective_limit);
    }

    fn spawn_session_actor(
        &self,
        authentication_token: NodeId,
        session: Arc<RwLock<Session>>,
        session_id_numeric: u32,
        node_managers: NodeManagers,
        #[cfg(feature = "subscriptions")] subscriptions: Arc<SubscriptionCache>,
    ) {
        let (sender, receiver) = mpsc::channel(SESSION_ACTOR_QUEUE_CAPACITY);
        self.register_actor_sender(authentication_token.clone(), sender);
        let user_roles = session.read().roles();

        let context = RequestContext {
            current_node_manager_index: 0,
            inner: Arc::new(RequestContextInner {
                session,
                session_id: session_id_numeric,
                authenticator: self.info.authenticator.clone(),
                token: UserToken(ANONYMOUS_USER_TOKEN_ID.to_string()),
                user_roles,
                type_tree: self.info.type_tree.clone(),
                type_tree_getter: self.info.type_tree_getter.clone(),
                #[cfg(feature = "subscriptions")]
                subscriptions,
                info: self.info.clone(),
            }),
        };

        let auth_tokens = Arc::clone(&self.auth_tokens);
        let actor_senders = Arc::clone(&self.actor_senders);
        let closed_auth_tokens = Arc::clone(&self.closed_auth_tokens);
        let info = self.info.clone();
        let mut actor =
            SessionActor::new(context, receiver).with_termination_cleanup(move |terminated| {
                auth_tokens.remove(&terminated.authentication_token);
                actor_senders.remove(&terminated.authentication_token);
                closed_auth_tokens.insert(terminated.authentication_token.clone(), Instant::now());
                clear_session_locale_ids_for_node_id(&info, &terminated.session_id);
                #[cfg(feature = "fota")]
                cleanup_session(&info, &terminated.session_id);
            });

        tokio::spawn(async move {
            // Catch panics so a dying actor always cleans its tokens out of
            // the lookup registries.
            match std::panic::AssertUnwindSafe(actor.run(node_managers))
                .catch_unwind()
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::debug!(%err, "session actor stopped"),
                Err(_) => actor.abort_after_panic(),
            }
        });
    }
}
