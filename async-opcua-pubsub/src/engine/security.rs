use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_types::{Context, MessageSecurityMode, StatusCode};

use crate::{
    codec::uadp::UadpNetworkMessage,
    security::{SecurityGroup, SharedSecurityGroup, UadpSecurityCodec},
};

use super::{CandidateTokenSnapshot, PubSubEngine, ReplayStreamIdentity};

impl PubSubEngine {
    /// Registers a PubSub security group for publisher message signing.
    pub fn register_security_group(
        &mut self,
        security_group: SecurityGroup,
    ) -> SharedSecurityGroup {
        let group_id = security_group.group_id().to_string();
        let shared_group = Arc::new(RwLock::new(security_group));
        self.replay_windows.write().remove(&group_id);
        self.security_groups.insert(group_id, shared_group.clone());
        shared_group
    }

    /// Registers shared PubSub security group state for publisher message signing.
    pub fn register_shared_security_group(&mut self, security_group: SharedSecurityGroup) {
        let group_id = security_group.read().group_id().to_string();
        self.replay_windows.write().remove(&group_id);
        self.security_groups.insert(group_id, security_group);
    }

    /// Removes a registered PubSub security group.
    pub fn remove_security_group(&mut self, group_id: &str) -> Option<SharedSecurityGroup> {
        self.replay_windows.write().remove(group_id);
        self.security_groups.remove(group_id)
    }

    /// Returns a registered PubSub security group.
    pub fn security_group(&self, group_id: &str) -> Option<SharedSecurityGroup> {
        self.security_groups.get(group_id).cloned()
    }

    /// Encodes a publisher UADP NetworkMessage using the current key for a security group.
    pub fn encode_publisher_uadp_message(
        &self,
        security_group_id: &str,
        security_mode: MessageSecurityMode,
        security_policy: SecurityPolicy,
        message: &UadpNetworkMessage,
        ctx: &Context<'_>,
    ) -> Result<Vec<u8>, StatusCode> {
        let security_group = self
            .security_groups
            .get(security_group_id)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let key_set = security_group.read().current_key_set().clone();
        UadpSecurityCodec::new(security_mode, security_policy, key_set)
            .encode_network_message(message, ctx)
            .map_err(|error| error.status())
    }

    /// Signs a publisher UADP NetworkMessage using the current key for a security group.
    pub fn sign_publisher_uadp_message(
        &self,
        security_group_id: &str,
        security_policy: SecurityPolicy,
        message: &UadpNetworkMessage,
        ctx: &Context<'_>,
    ) -> Result<Vec<u8>, StatusCode> {
        self.encode_publisher_uadp_message(
            security_group_id,
            MessageSecurityMode::Sign,
            security_policy,
            message,
            ctx,
        )
    }

    fn check_authenticated_replay(
        &self,
        security_group_id: &str,
        authenticated_candidates: CandidateTokenSnapshot,
        message: &UadpNetworkMessage,
        token_id: u32,
    ) -> Result<(), StatusCode> {
        if !authenticated_candidates.contains(token_id) {
            return Err(StatusCode::BadSecurityChecksFailed);
        }

        let security_group = self
            .security_groups
            .get(security_group_id)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let security_group = security_group.read();
        let candidate_tokens = CandidateTokenSnapshot::new(
            security_group.current_key_set().token_id(),
            security_group.next_key_set().token_id(),
        );
        if !candidate_tokens.contains(token_id) {
            return Err(StatusCode::BadSecurityChecksFailed);
        }

        let stream_identity =
            ReplayStreamIdentity::new(&message.publisher_id, message.writer_group_id);
        let mut replay_groups = self.replay_windows.write();
        let replay_group = replay_groups
            .entry(security_group_id.to_string())
            .or_default();
        replay_group.reconcile_candidate_tokens(candidate_tokens);
        let replay_result = replay_group
            .stream_windows_mut_or_insert(stream_identity)?
            .entry(token_id)
            .or_default()
            .check(token_id, message.sequence_number)
            .map_err(|error| error.status());
        drop(replay_groups);
        drop(security_group);
        replay_result
    }

    /// Decodes and verifies a subscriber UADP NetworkMessage using a security group's current key.
    pub fn decode_subscriber_uadp_message(
        &self,
        security_group_id: &str,
        security_mode: MessageSecurityMode,
        security_policy: SecurityPolicy,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<UadpNetworkMessage, StatusCode> {
        let security_group = self
            .security_groups
            .get(security_group_id)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let (key_sets, candidate_tokens) = {
            let security_group = security_group.read();
            let current = security_group.current_key_set().clone();
            let next = security_group.next_key_set().clone();
            let candidate_tokens = CandidateTokenSnapshot::new(current.token_id(), next.token_id());
            (vec![current, next], candidate_tokens)
        };
        let (message, token_id) =
            UadpSecurityCodec::with_candidates(security_mode, security_policy, key_sets)
                .decode_network_message_with_token(payload, ctx)
                .map_err(|error| error.status())?;

        if let Some(token_id) = token_id {
            self.check_authenticated_replay(
                security_group_id,
                candidate_tokens,
                &message,
                token_id,
            )?;
        }

        Ok(message)
    }
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod tests;
