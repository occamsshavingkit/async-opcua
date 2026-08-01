//! Verifies secured UADP replay isolation across authenticated stream identities.

use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_pubsub::{
    engine::PubSubEngine, security::UadpSecurityCodec, PublisherId, SecurityGroup,
    UadpDataSetMessage, UadpNetworkMessage,
};
use opcua_server::address_space::AddressSpace;
use opcua_types::{ContextOwned, MessageSecurityMode, StatusCode, Variant};

const SECURITY_GROUP_ID: &str = "shared-security-group";
const MODE: MessageSecurityMode = MessageSecurityMode::SignAndEncrypt;
const POLICY: SecurityPolicy = SecurityPolicy::PubSubAes256Ctr;
const SEQUENCE_NUMBER: u16 = 1;

fn address_space() -> Arc<RwLock<AddressSpace>> {
    Arc::new(RwLock::new(AddressSpace::new()))
}

fn message(publisher_id: PublisherId, writer_group_id: u16) -> UadpNetworkMessage {
    UadpNetworkMessage {
        publisher_id,
        writer_group_id,
        network_message_number: 0,
        sequence_number: SEQUENCE_NUMBER,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 10,
            sequence_number: SEQUENCE_NUMBER,
            timestamp: None,
            status: None,
            fields: vec![Variant::from(1.0f64)],
        }],
    }
}

#[test]
fn secured_replay_windows_are_isolated_by_authenticated_publisher_id() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut engine = PubSubEngine::new(address_space());
    let group = SecurityGroup::new(SECURITY_GROUP_ID, Duration::from_secs(3600)).unwrap();
    let key_set = group.current_key_set().clone();
    engine.register_security_group(group);
    let codec = UadpSecurityCodec::new(MODE, POLICY, key_set);

    let first_message = message(PublisherId::UInt16(100), 7);
    let second_message = message(PublisherId::UInt16(200), 7);
    let first_payload = codec.encode_network_message(&first_message, &ctx).unwrap();
    let second_payload = codec.encode_network_message(&second_message, &ctx).unwrap();

    let first = engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &first_payload, &ctx)
        .expect("first authenticated publisher stream must be accepted");
    assert_eq!(first, first_message);

    let second = engine.decode_subscriber_uadp_message(
        SECURITY_GROUP_ID,
        MODE,
        POLICY,
        &second_payload,
        &ctx,
    );
    assert!(
        second.is_ok(),
        "same sequence from a different authenticated publisher must be accepted independently, got {second:?}"
    );
    assert_eq!(second.unwrap(), second_message);

    let first_replay = engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &first_payload, &ctx)
        .unwrap_err();
    assert_eq!(first_replay, StatusCode::BadSecurityChecksFailed);
    let second_replay = engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &second_payload, &ctx)
        .unwrap_err();
    assert_eq!(second_replay, StatusCode::BadSecurityChecksFailed);
}

#[test]
fn secured_replay_windows_are_isolated_by_authenticated_writer_group_id() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut engine = PubSubEngine::new(address_space());
    let group = SecurityGroup::new(SECURITY_GROUP_ID, Duration::from_secs(3600)).unwrap();
    let key_set = group.current_key_set().clone();
    engine.register_security_group(group);
    let codec = UadpSecurityCodec::new(MODE, POLICY, key_set);

    let first_message = message(PublisherId::UInt16(100), 7);
    let second_message = message(PublisherId::UInt16(100), 8);
    let first_payload = codec.encode_network_message(&first_message, &ctx).unwrap();
    let second_payload = codec.encode_network_message(&second_message, &ctx).unwrap();

    let first = engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &first_payload, &ctx)
        .expect("first authenticated writer-group stream must be accepted");
    assert_eq!(first, first_message);

    let second = engine.decode_subscriber_uadp_message(
        SECURITY_GROUP_ID,
        MODE,
        POLICY,
        &second_payload,
        &ctx,
    );
    assert!(
        second.is_ok(),
        "same sequence from a different authenticated writer group must be accepted independently, got {second:?}"
    );
    assert_eq!(second.unwrap(), second_message);

    let first_replay = engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &first_payload, &ctx)
        .unwrap_err();
    assert_eq!(first_replay, StatusCode::BadSecurityChecksFailed);
    let second_replay = engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &second_payload, &ctx)
        .unwrap_err();
    assert_eq!(second_replay, StatusCode::BadSecurityChecksFailed);
}

#[test]
fn secured_replay_windows_reject_replays_after_alternating_candidate_tokens() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut engine = PubSubEngine::new(address_space());
    let group = SecurityGroup::new(SECURITY_GROUP_ID, Duration::from_secs(3600)).unwrap();
    let current_key_set = group.current_key_set().clone();
    let next_key_set = group.next_key_set().clone();
    engine.register_security_group(group);

    let network_message = message(PublisherId::UInt16(100), 7);
    let current_payload = UadpSecurityCodec::new(MODE, POLICY, current_key_set)
        .encode_network_message(&network_message, &ctx)
        .unwrap();
    let next_payload = UadpSecurityCodec::new(MODE, POLICY, next_key_set)
        .encode_network_message(&network_message, &ctx)
        .unwrap();

    engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &current_payload, &ctx)
        .expect("current-token sequence 1 must be accepted once");
    engine
        .decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &next_payload, &ctx)
        .expect("next-token sequence 1 must be accepted once");

    let current_replay = engine.decode_subscriber_uadp_message(
        SECURITY_GROUP_ID,
        MODE,
        POLICY,
        &current_payload,
        &ctx,
    );
    let next_replay =
        engine.decode_subscriber_uadp_message(SECURITY_GROUP_ID, MODE, POLICY, &next_payload, &ctx);

    assert_eq!(current_replay, Err(StatusCode::BadSecurityChecksFailed));
    assert_eq!(next_replay, Err(StatusCode::BadSecurityChecksFailed));
}
