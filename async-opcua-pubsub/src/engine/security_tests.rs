use std::{collections::HashMap, sync::Arc, time::Duration};

use opcua_crypto::SecurityPolicy;
use opcua_server::address_space::AddressSpace;
use opcua_types::{ContextOwned, MessageSecurityMode, StatusCode, Variant};

use crate::{
    security::{ReplayWindow, SecurityGroup, SecurityKeySet, UadpSecurityCodec},
    PublisherId, UadpDataSetMessage, UadpNetworkMessage,
};

use super::super::{CandidateTokenSnapshot, PubSubEngine, ReplayGroupState, ReplayStreamIdentity};

const TEST_REPLAY_STREAM_CAPACITY: usize = 1024;
const TEST_SECURITY_GROUP_ID: &str = "replay-capacity-group";
const TEST_SECURITY_MODE: MessageSecurityMode = MessageSecurityMode::SignAndEncrypt;
const TEST_SECURITY_POLICY: SecurityPolicy = SecurityPolicy::PubSubAes256Ctr;

fn test_message(publisher_id: PublisherId, sequence_number: u16) -> UadpNetworkMessage {
    UadpNetworkMessage {
        publisher_id,
        writer_group_id: 7,
        network_message_number: sequence_number,
        sequence_number,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 10,
            sequence_number,
            timestamp: None,
            status: None,
            fields: vec![Variant::from(1.0f64)],
        }],
    }
}

fn test_key_set(token_id: u32, seed: u8) -> SecurityKeySet {
    SecurityKeySet::from_parts(
        token_id,
        vec![seed; 32],
        vec![seed + 1; 32],
        vec![seed + 2; 32],
    )
    .unwrap()
}

#[test]
fn candidate_token_change_prunes_obsolete_windows_and_empty_streams() {
    let retained_stream = ReplayStreamIdentity::new(&PublisherId::UInt16(100), 7);
    let removed_stream = ReplayStreamIdentity::new(&PublisherId::UInt16(200), 7);
    let mut state = ReplayGroupState::default();
    state.reconcile_candidate_tokens(CandidateTokenSnapshot::new(1, 2));
    state
        .streams
        .entry(retained_stream)
        .or_default()
        .insert(1, ReplayWindow::new());
    state
        .streams
        .entry(ReplayStreamIdentity::new(&PublisherId::UInt16(100), 7))
        .or_default()
        .insert(2, ReplayWindow::new());
    state
        .streams
        .entry(removed_stream)
        .or_default()
        .insert(1, ReplayWindow::new());

    state.reconcile_candidate_tokens(CandidateTokenSnapshot::new(2, 3));

    let retained_windows = state
        .streams
        .get(&ReplayStreamIdentity::new(&PublisherId::UInt16(100), 7))
        .unwrap();
    assert_eq!(retained_windows.len(), 1);
    assert!(retained_windows.contains_key(&2));
    assert!(!state
        .streams
        .contains_key(&ReplayStreamIdentity::new(&PublisherId::UInt16(200), 7)));
}

#[test]
fn authenticated_unseen_stream_is_rejected_at_capacity_without_evicting_established_state() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let address_space = Arc::new(opcua_core::sync::RwLock::new(AddressSpace::new()));
    let mut engine = PubSubEngine::new(address_space);
    let group = SecurityGroup::new(TEST_SECURITY_GROUP_ID, Duration::from_secs(3600)).unwrap();
    let key_set = group.current_key_set().clone();
    let token_id = key_set.token_id();
    engine.register_security_group(group);
    let codec = UadpSecurityCodec::new(TEST_SECURITY_MODE, TEST_SECURITY_POLICY, key_set);

    let established_first = test_message(PublisherId::UInt16(1), 1);
    let established_next = test_message(PublisherId::UInt16(1), 2);
    let unseen = test_message(PublisherId::UInt16(2), 1);
    let established_first_payload = codec
        .encode_network_message(&established_first, &ctx)
        .unwrap();
    let established_next_payload = codec
        .encode_network_message(&established_next, &ctx)
        .unwrap();
    let unseen_payload = codec.encode_network_message(&unseen, &ctx).unwrap();

    engine
        .decode_subscriber_uadp_message(
            TEST_SECURITY_GROUP_ID,
            TEST_SECURITY_MODE,
            TEST_SECURITY_POLICY,
            &established_first_payload,
            &ctx,
        )
        .expect("the established authenticated stream must seed replay state");

    {
        let mut replay_groups = engine.replay_windows.write();
        let replay_group = replay_groups.get_mut(TEST_SECURITY_GROUP_ID).unwrap();
        // inv: before offset k, replay state contains the established stream plus k unique seeded streams.
        // term: publisher_offset increases by one toward the fixed capacity-minus-one bound.
        for publisher_offset in 0..(TEST_REPLAY_STREAM_CAPACITY - 1) {
            let publisher_id = u32::try_from(publisher_offset).unwrap();
            let identity = ReplayStreamIdentity::new(&PublisherId::UInt32(publisher_id), 99);
            let previous = replay_group
                .streams
                .insert(identity, HashMap::from([(token_id, ReplayWindow::new())]));
            assert!(previous.is_none());
        }
        assert_eq!(replay_group.streams.len(), TEST_REPLAY_STREAM_CAPACITY);
    }

    engine
        .decode_subscriber_uadp_message(
            TEST_SECURITY_GROUP_ID,
            TEST_SECURITY_MODE,
            TEST_SECURITY_POLICY,
            &established_next_payload,
            &ctx,
        )
        .expect("an established stream must remain usable at capacity");

    let unseen_result = engine.decode_subscriber_uadp_message(
        TEST_SECURITY_GROUP_ID,
        TEST_SECURITY_MODE,
        TEST_SECURITY_POLICY,
        &unseen_payload,
        &ctx,
    );
    assert_eq!(unseen_result, Err(StatusCode::BadResourceUnavailable));

    let established_replay = engine.decode_subscriber_uadp_message(
        TEST_SECURITY_GROUP_ID,
        TEST_SECURITY_MODE,
        TEST_SECURITY_POLICY,
        &established_next_payload,
        &ctx,
    );
    assert_eq!(established_replay, Err(StatusCode::BadSecurityChecksFailed));
    assert_eq!(
        engine
            .replay_windows
            .read()
            .get(TEST_SECURITY_GROUP_ID)
            .unwrap()
            .streams
            .len(),
        TEST_REPLAY_STREAM_CAPACITY
    );
}

#[test]
fn stale_authenticated_token_is_rejected_after_rotation_prunes_it() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let address_space = Arc::new(opcua_core::sync::RwLock::new(AddressSpace::new()));
    let mut engine = PubSubEngine::new(address_space);
    let key_a = test_key_set(1, 1);
    let key_b = test_key_set(2, 11);
    let stale_candidates = CandidateTokenSnapshot::new(key_a.token_id(), key_b.token_id());
    let group = SecurityGroup::with_key_sets(
        TEST_SECURITY_GROUP_ID,
        key_a.clone(),
        key_b.clone(),
        Duration::from_secs(3600),
    )
    .unwrap();
    let shared_group = Arc::new(opcua_core::sync::RwLock::new(group));
    engine.register_shared_security_group(shared_group.clone());

    let seed_message = test_message(PublisherId::UInt16(1), 1);
    let stale_message = test_message(PublisherId::UInt16(1), 2);
    let encoder = UadpSecurityCodec::new(TEST_SECURITY_MODE, TEST_SECURITY_POLICY, key_a.clone());
    let seed_payload = encoder.encode_network_message(&seed_message, &ctx).unwrap();
    let stale_payload = encoder
        .encode_network_message(&stale_message, &ctx)
        .unwrap();
    let decoder = UadpSecurityCodec::with_candidates(
        TEST_SECURITY_MODE,
        TEST_SECURITY_POLICY,
        vec![key_a, key_b],
    );
    let (seed_message, seed_token_id) = decoder
        .decode_network_message_with_token(&seed_payload, &ctx)
        .unwrap();
    let (stale_message, stale_token_id) = decoder
        .decode_network_message_with_token(&stale_payload, &ctx)
        .unwrap();
    let seed_token_id = seed_token_id.unwrap();
    let stale_token_id = stale_token_id.unwrap();

    engine
        .check_authenticated_replay(
            TEST_SECURITY_GROUP_ID,
            stale_candidates,
            &seed_message,
            seed_token_id,
        )
        .unwrap();
    shared_group.write().rotate_key_sets();
    let live_candidates = {
        let group = shared_group.read();
        CandidateTokenSnapshot::new(
            group.current_key_set().token_id(),
            group.next_key_set().token_id(),
        )
    };
    engine
        .replay_windows
        .write()
        .get_mut(TEST_SECURITY_GROUP_ID)
        .unwrap()
        .reconcile_candidate_tokens(live_candidates);

    let result = engine.check_authenticated_replay(
        TEST_SECURITY_GROUP_ID,
        stale_candidates,
        &stale_message,
        stale_token_id,
    );

    assert_eq!(result, Err(StatusCode::BadSecurityChecksFailed));
    assert!(engine
        .replay_windows
        .read()
        .get(TEST_SECURITY_GROUP_ID)
        .unwrap()
        .streams
        .is_empty());
}
