//! Secured UADP subscriber runtime integration tests.

use std::sync::Arc;
use std::time::Duration;

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_pubsub::{
    DataSetReaderConfig, FieldTargetConfig, PubSubConnectionConfig, PubSubEngine, PublisherId,
    ReaderGroupConfig, SecurityGroup, UadpDataSetMessage, UadpNetworkMessage, UadpSecurityCodec,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, ContextOwned, DataEncoding, DataTypeId, MessageSecurityMode, NodeId, NumericRange,
    StatusCode, TimestampsToReturn, Variant,
};

const POLICY: SecurityPolicy = SecurityPolicy::PubSubAes256Ctr;

fn target_value(space: &AddressSpace, node: &NodeId) -> Option<Variant> {
    space
        .find(node)?
        .as_node()
        .get_attribute(
            TimestampsToReturn::Neither,
            AttributeId::Value,
            &NumericRange::None,
            &DataEncoding::Binary,
        )?
        .value
}

fn address_space_with_target() -> (Arc<RwLock<AddressSpace>>, NodeId) {
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = NodeId::new(1, "SecureTarget");
    VariableBuilder::new(&target, "SecureTarget", "SecureTarget")
        .data_type(DataTypeId::Double)
        .value(Variant::Double(0.0))
        .insert(&space);
    (Arc::new(RwLock::new(space)), target)
}

fn message(sequence_number: u16) -> UadpNetworkMessage {
    UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(sequence_number as f64)],
        }],
    }
}

fn reader(target: NodeId) -> DataSetReaderConfig {
    DataSetReaderConfig {
        name: Some("secure-reader".to_string()),
        dataset_reader_id: 1,
        dataset_writer_id: 42,
        publisher_id: Some(PublisherId::UInt16(11)),
        writer_group_id: Some(7),
        network_message_number: Some(3),
        target_variables: vec![FieldTargetConfig::value(0, target)],
        ..DataSetReaderConfig::default()
    }
}

fn connection(target: NodeId, group_mode: Option<MessageSecurityMode>) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "secure-conn".to_string(),
        name: "secure-conn".to_string(),
        address: "udp://127.0.0.1:4840".to_string(),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            security_mode: group_mode,
            security_policy_uri: Some(POLICY.to_uri().to_string()),
            security_group_id: Some("line-a".to_string()),
            dataset_readers: vec![reader(target)],
        }],
    }
}

fn engine(target: NodeId, address_space: Arc<RwLock<AddressSpace>>) -> PubSubEngine {
    let mut engine = PubSubEngine::with_connections(
        address_space,
        vec![connection(
            target,
            Some(MessageSecurityMode::SignAndEncrypt),
        )],
    );
    engine
        .register_security_group(SecurityGroup::new("line-a", Duration::from_secs(3600)).unwrap());
    engine
}

#[test]
fn sign_and_encrypt_datagram_verifies_decrypts_and_applies() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let (address_space, target) = address_space_with_target();
    let mut engine = engine(target.clone(), address_space.clone());
    let secured = engine
        .encode_publisher_uadp_message(
            "line-a",
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(1),
            &ctx,
        )
        .unwrap();

    let outcome = engine
        .process_subscriber_datagram("secure-conn", &secured, &ctx)
        .unwrap();

    assert_eq!(outcome.applied_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(1.0))
    );
}

#[test]
fn tampered_secured_datagram_does_not_update_target() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let (address_space, target) = address_space_with_target();
    let mut engine = engine(target.clone(), address_space.clone());
    let mut secured = engine
        .encode_publisher_uadp_message(
            "line-a",
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(1),
            &ctx,
        )
        .unwrap();
    let last = secured.len() - 1;
    secured[last] ^= 0x01;

    assert_eq!(
        engine
            .process_subscriber_datagram("secure-conn", &secured, &ctx)
            .unwrap_err(),
        StatusCode::BadSecurityChecksFailed
    );
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        engine.subscriber_status(1).unwrap().security_failure_count,
        1
    );
}

#[test]
fn replayed_secured_datagram_does_not_update_target_twice() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let (address_space, target) = address_space_with_target();
    let mut engine = engine(target.clone(), address_space.clone());
    let secured = engine
        .encode_publisher_uadp_message(
            "line-a",
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(2),
            &ctx,
        )
        .unwrap();

    engine
        .process_subscriber_datagram("secure-conn", &secured, &ctx)
        .unwrap();
    assert_eq!(
        engine
            .process_subscriber_datagram("secure-conn", &secured, &ctx)
            .unwrap_err(),
        StatusCode::BadSecurityChecksFailed
    );

    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(2.0))
    );
    assert_eq!(
        engine.subscriber_status(1).unwrap().security_failure_count,
        1
    );
}

#[test]
fn unknown_security_token_does_not_update_target() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let (address_space, target) = address_space_with_target();
    let mut engine = engine(target.clone(), address_space.clone());
    let foreign_group = SecurityGroup::new("foreign", Duration::from_secs(3600)).unwrap();
    let secured = UadpSecurityCodec::new(
        MessageSecurityMode::SignAndEncrypt,
        POLICY,
        foreign_group.current_key_set().clone(),
    )
    .encode_network_message(&message(3), &ctx)
    .unwrap();

    assert_eq!(
        engine
            .process_subscriber_datagram("secure-conn", &secured, &ctx)
            .unwrap_err(),
        StatusCode::BadSecurityChecksFailed
    );
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
}

#[test]
fn dataset_reader_security_override_wins_over_reader_group_none() {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let (address_space, target) = address_space_with_target();
    let mut cfg = connection(target.clone(), Some(MessageSecurityMode::None));
    cfg.reader_groups[0].dataset_readers[0].security_mode =
        Some(MessageSecurityMode::SignAndEncrypt);
    cfg.reader_groups[0].dataset_readers[0].security_policy_uri = Some(POLICY.to_uri().to_string());
    cfg.reader_groups[0].dataset_readers[0].security_group_id = Some("line-a".to_string());
    let mut engine = PubSubEngine::with_connections(address_space.clone(), vec![cfg]);
    engine
        .register_security_group(SecurityGroup::new("line-a", Duration::from_secs(3600)).unwrap());
    let secured = engine
        .encode_publisher_uadp_message(
            "line-a",
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(4),
            &ctx,
        )
        .unwrap();

    engine
        .process_subscriber_datagram("secure-conn", &secured, &ctx)
        .unwrap();

    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(4.0))
    );
}

#[test]
fn heterogeneous_config_rejects_datagram_before_decode_or_state_mutation() {
    // Given: a heterogeneous connection, an existing target, and malformed UADP bytes.
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let mut config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    let mut unsecured_group = config.reader_groups[0].clone();
    unsecured_group.reader_group_id = 2;
    unsecured_group.security_mode = None;
    config.reader_groups.push(unsecured_group);
    let mut engine = PubSubEngine::with_connections(address_space.clone(), vec![config]);

    // When: the datagram enters the real subscriber processing boundary.
    let result = engine.process_subscriber_datagram("secure-conn", &[0xff], &context.context());

    // Then: configuration fails before runtime status, decode, or target mutation.
    assert_eq!(result.unwrap_err(), StatusCode::BadConfigurationError);
    assert!(engine.subscriber_status(1).is_none());
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
}
