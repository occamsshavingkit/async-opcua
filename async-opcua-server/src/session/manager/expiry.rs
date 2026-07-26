use std::cmp::Reverse;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use opcua_core::{trace_read_lock, trace_write_lock};
use opcua_types::NodeId;
use tracing::info;

#[cfg(feature = "fota")]
use crate::fota::cleanup::cleanup_session;

use super::{clear_session_locale_ids, types::SessionExpiryEntry, SessionManager};

impl SessionManager {
    pub(crate) fn expire_session(&mut self, id: &NodeId) {
        let Some(session) = self.sessions.remove(id) else {
            return;
        };
        {
            let session = trace_read_lock!(&session);
            let channel_id = session.secure_channel_id();
            if !session.is_activated() {
                if let Some(counter) = self.unactivated_by_channel.get(&channel_id) {
                    counter.fetch_sub(1, Ordering::Release);
                }
            }
            self.channel_body_limits.remove(&channel_id);
        }
        #[cfg(feature = "diagnostics")]
        {
            self.info
                .diagnostics
                .set_current_session_count(self.sessions.len() as u32);
            self.info.diagnostics.inc_session_timeout_count();
        }

        info!(
            "Session {id} has expired, removing it from the session map. Subscriptions will remain until they individually expire"
        );

        let (token, session_id_numeric) = {
            let session = trace_read_lock!(session);
            (
                session.authentication_token.clone(),
                session.session_id_numeric(),
            )
        };
        self.deregister_token(&token);
        clear_session_locale_ids(&self.info, session_id_numeric);

        let mut session = trace_write_lock!(session);
        session.close();
        drop(session);
        #[cfg(feature = "fota")]
        cleanup_session(&self.info, id);
    }

    #[cfg(feature = "fota")]
    pub(crate) fn cleanup_fota_for_secure_channel(&self, secure_channel_id: u32) {
        let session_ids = self
            .sessions
            .iter()
            .filter_map(|(id, session)| {
                let session = trace_read_lock!(session);
                (session.secure_channel_id() == secure_channel_id).then(|| id.clone())
            })
            .collect::<Vec<_>>();

        for session_id in session_ids {
            cleanup_session(&self.info, &session_id);
        }
    }

    pub(crate) fn check_session_expiry(&self) -> (Instant, Vec<NodeId>) {
        let now = Instant::now();
        let default_expiry = now + Duration::from_millis(self.info.config.max_session_timeout_ms);
        let mut expired = Vec::new();
        let mut next_expiry = default_expiry;

        let mut heap = self.expiry_heap.lock();
        loop {
            let next = heap.pop();
            let Some(Reverse(SessionExpiryEntry {
                deadline,
                session_id,
            })) = next
            else {
                break;
            };
            if deadline > now {
                next_expiry = next_expiry.min(deadline);
                heap.push(Reverse(SessionExpiryEntry {
                    deadline,
                    session_id,
                }));
                break;
            }
            let Some(session) = self.sessions.get(&session_id) else {
                continue;
            };
            let Some(session) = session.try_read() else {
                continue;
            };
            let session_deadline = session.deadline();
            if !session.is_activated() {
                let unactivated_deadline = session.created_at()
                    + Duration::from_millis(self.info.config.limits.unactivated_session_timeout_ms);
                if session_deadline.min(unactivated_deadline) > now {
                    heap.push(Reverse(SessionExpiryEntry {
                        deadline: session_deadline.min(unactivated_deadline),
                        session_id,
                    }));
                    next_expiry = next_expiry.min(session_deadline.min(unactivated_deadline));
                    continue;
                }
            } else if session_deadline > now {
                heap.push(Reverse(SessionExpiryEntry {
                    deadline: session_deadline,
                    session_id,
                }));
                next_expiry = next_expiry.min(session_deadline);
                continue;
            }
            expired.push(session_id);
        }

        (next_expiry, expired)
    }
}
