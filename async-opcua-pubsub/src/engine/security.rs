use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_types::{Context, MessageSecurityMode, StatusCode};

use crate::{
    codec::uadp::UadpNetworkMessage,
    config::{MessageEncoding, SubscriberSecurityConfig},
    security::{SecurityGroup, SharedSecurityGroup, UadpSecurityCodec},
    PubSubConnectionConfig,
};

use super::{
    CandidateTokenSnapshot, PubSubEngine, ReplayGroupKey, ReplayGroupState, ReplayStreamIdentity,
};

struct SubscriberSecurityMaterial {
    security_group: SharedSecurityGroup,
    security_mode: MessageSecurityMode,
    security_policy: SecurityPolicy,
}

impl SubscriberSecurityMaterial {
    fn decode_candidate(
        &self,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<(UadpNetworkMessage, Option<u32>, CandidateTokenSnapshot), StatusCode> {
        let (key_sets, candidate_tokens) = {
            let security_group = self.security_group.read();
            let current = security_group.current_key_set().clone();
            let next = security_group.next_key_set().clone();
            let candidate_tokens = CandidateTokenSnapshot::new(current.token_id(), next.token_id());
            (vec![current, next], candidate_tokens)
        };
        let (message, token_id) =
            UadpSecurityCodec::with_candidates(self.security_mode, self.security_policy, key_sets)
                .decode_network_message_with_token(payload, ctx)
                .map_err(|error| error.status())?;
        Ok((message, token_id, candidate_tokens))
    }
}

pub(super) struct SubscriberSecurityProcessor {
    material: SubscriberSecurityMaterial,
    replay_group: ReplayGroupState,
}

impl SubscriberSecurityProcessor {
    pub(super) fn decode(
        &mut self,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<UadpNetworkMessage, StatusCode> {
        let (message, token_id, authenticated_candidates) =
            self.material.decode_candidate(payload, ctx)?;
        if let Some(token_id) = token_id {
            let candidate_tokens = live_candidate_tokens(
                &self.material.security_group,
                authenticated_candidates,
                token_id,
            )?;
            let stream_identity =
                ReplayStreamIdentity::new(&message.publisher_id, message.writer_group_id);
            self.replay_group
                .reconcile_candidate_tokens(candidate_tokens);
            self.replay_group
                .stream_windows_mut_or_insert(stream_identity)?
                .entry(token_id)
                .or_default()
                .check(token_id, message.sequence_number)
                .map_err(|error| error.status())?;
        }
        Ok(message)
    }
}

fn live_candidate_tokens(
    security_group: &SharedSecurityGroup,
    authenticated_candidates: CandidateTokenSnapshot,
    token_id: u32,
) -> Result<CandidateTokenSnapshot, StatusCode> {
    if !authenticated_candidates.contains(token_id) {
        return Err(StatusCode::BadSecurityChecksFailed);
    }

    let security_group = security_group.read();
    let candidate_tokens = CandidateTokenSnapshot::new(
        security_group.current_key_set().token_id(),
        security_group.next_key_set().token_id(),
    );
    if !candidate_tokens.contains(token_id) {
        return Err(StatusCode::BadSecurityChecksFailed);
    }
    Ok(candidate_tokens)
}

impl PubSubEngine {
    /// Registers a PubSub security group for publisher message signing.
    pub fn register_security_group(
        &mut self,
        security_group: SecurityGroup,
    ) -> SharedSecurityGroup {
        let group_id = security_group.group_id().to_string();
        let shared_group = Arc::new(RwLock::new(security_group));
        self.clear_replay_windows_for_security_group(&group_id);
        self.security_groups.insert(group_id, shared_group.clone());
        shared_group
    }

    /// Registers shared PubSub security group state for publisher message signing.
    pub fn register_shared_security_group(&mut self, security_group: SharedSecurityGroup) {
        let group_id = security_group.read().group_id().to_string();
        self.clear_replay_windows_for_security_group(&group_id);
        self.security_groups.insert(group_id, security_group);
    }

    /// Removes a registered PubSub security group.
    pub fn remove_security_group(&mut self, group_id: &str) -> Option<SharedSecurityGroup> {
        self.clear_replay_windows_for_security_group(group_id);
        self.security_groups.remove(group_id)
    }

    /// Returns a registered PubSub security group.
    pub fn security_group(&self, group_id: &str) -> Option<SharedSecurityGroup> {
        self.security_groups.get(group_id).cloned()
    }

    fn clear_replay_windows_for_security_group(&self, group_id: &str) {
        self.replay_windows
            .write()
            .retain(|key, _| key.security_group_id() != group_id);
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
        replay_group_key: ReplayGroupKey,
        authenticated_candidates: CandidateTokenSnapshot,
        message: &UadpNetworkMessage,
        token_id: u32,
    ) -> Result<(), StatusCode> {
        let security_group = self
            .security_groups
            .get(replay_group_key.security_group_id())
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let candidate_tokens =
            live_candidate_tokens(security_group, authenticated_candidates, token_id)?;

        let stream_identity =
            ReplayStreamIdentity::new(&message.publisher_id, message.writer_group_id);
        let mut replay_groups = self.replay_windows.write();
        let replay_group = replay_groups.entry(replay_group_key).or_default();
        replay_group.reconcile_candidate_tokens(candidate_tokens);
        let replay_result = replay_group
            .stream_windows_mut_or_insert(stream_identity)?
            .entry(token_id)
            .or_default()
            .check(token_id, message.sequence_number)
            .map_err(|error| error.status());
        drop(replay_groups);
        replay_result
    }

    pub(super) fn prepare_subscriber_security_processor(
        &self,
        connection: &PubSubConnectionConfig,
    ) -> Result<Option<SubscriberSecurityProcessor>, StatusCode> {
        let Some(security) = connection.validated_subscriber_security()? else {
            return Ok(None);
        };
        if connection
            .reader_groups
            .iter()
            .flat_map(|group| group.dataset_readers.iter())
            .any(|reader| reader.message_encoding == MessageEncoding::Json)
        {
            return Err(StatusCode::BadNotSupported);
        }

        let security_policy = SecurityPolicy::from_uri(&security.security_policy_uri);
        if security_policy == SecurityPolicy::Unknown {
            return Err(StatusCode::BadSecurityChecksFailed);
        }
        let security_group = self
            .security_groups
            .get(&security.security_group_id)
            .cloned()
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        Ok(Some(SubscriberSecurityProcessor {
            material: SubscriberSecurityMaterial {
                security_group,
                security_mode: security.security_mode,
                security_policy,
            },
            replay_group: ReplayGroupState::default(),
        }))
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
        self.decode_subscriber_uadp_message_scoped(
            ReplayGroupKey::global(security_group_id),
            security_mode,
            security_policy,
            payload,
            ctx,
        )
    }

    pub(super) fn decode_connection_subscriber_uadp_message(
        &self,
        connection_id: &str,
        security: &SubscriberSecurityConfig,
        security_policy: SecurityPolicy,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<UadpNetworkMessage, StatusCode> {
        self.decode_subscriber_uadp_message_scoped(
            ReplayGroupKey::connection(&security.security_group_id, connection_id),
            security.security_mode,
            security_policy,
            payload,
            ctx,
        )
    }

    fn decode_subscriber_uadp_message_scoped(
        &self,
        replay_group_key: ReplayGroupKey,
        security_mode: MessageSecurityMode,
        security_policy: SecurityPolicy,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<UadpNetworkMessage, StatusCode> {
        let security_group = self
            .security_groups
            .get(replay_group_key.security_group_id())
            .cloned()
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let material = SubscriberSecurityMaterial {
            security_group,
            security_mode,
            security_policy,
        };
        let (message, token_id, candidate_tokens) = material.decode_candidate(payload, ctx)?;

        if let Some(token_id) = token_id {
            self.check_authenticated_replay(
                replay_group_key,
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
