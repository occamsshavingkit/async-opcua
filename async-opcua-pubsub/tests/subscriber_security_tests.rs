//! Secured UADP subscriber runtime integration tests.

use std::sync::Arc;
use std::time::{Duration, Instant};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_pubsub::{
    decode_and_apply, DataSetReaderConfig, DataSetReaderKey, FieldTargetConfig,
    PubSubConnectionConfig, PubSubEngine, PublisherId, ReaderGroupConfig, SecurityGroup,
    SubscriberError, SubscriberRuntime, UadpDataSetMessage, UadpNetworkMessage, UadpSecurityCodec,
};
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, BinaryEncodable, ContextOwned, DataEncoding, DataTypeId, MessageSecurityMode,
    NodeId, NumericRange, StatusCode, TimestampsToReturn, Variant,
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

fn plaintext_datagram(sequence_number: u16, ctx: &opcua_types::Context<'_>) -> Vec<u8> {
    message(sequence_number).encode_to_vec(ctx)
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
    connection_with_id("secure-conn", target, group_mode)
}

fn connection_with_id(
    connection_id: &str,
    target: NodeId,
    group_mode: Option<MessageSecurityMode>,
) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
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

#[test]
fn security_failure_is_charged_only_to_the_owning_connection_key() {
    // Given two secured connections whose readers share the same numeric id.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = NodeId::new(1, "FirstSecureTarget");
    let second_target = NodeId::new(1, "SecondSecureTarget");
    for target in [&first_target, &second_target] {
        VariableBuilder::new(target, "SecureTarget", "SecureTarget")
            .data_type(DataTypeId::Double)
            .value(Variant::Double(0.0))
            .insert(&space);
    }
    let address_space = Arc::new(RwLock::new(space));
    let mut engine = PubSubEngine::with_connections(
        address_space,
        vec![
            connection_with_id(
                "first-secure",
                first_target,
                Some(MessageSecurityMode::SignAndEncrypt),
            ),
            connection_with_id(
                "second-secure",
                second_target,
                Some(MessageSecurityMode::SignAndEncrypt),
            ),
        ],
    );
    engine.register_security_group(
        SecurityGroup::new("line-a", Duration::from_secs(3600))
            .expect("security group fixture should be valid"),
    );
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut secured = engine
        .encode_publisher_uadp_message(
            "line-a",
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(1),
            &ctx,
        )
        .expect("secured fixture should encode");
    let last = secured.len() - 1;
    secured[last] ^= 0x01;

    // When the tampered datagram enters only the first connection.
    assert_eq!(
        engine
            .process_subscriber_datagram("first-secure", &secured, &ctx)
            .expect_err("tampered datagram should fail verification"),
        StatusCode::BadSecurityChecksFailed
    );

    // Then security accounting remains scoped and numeric compatibility is ambiguous.
    assert_eq!(
        engine
            .subscriber_status_by_key(&DataSetReaderKey::new("first-secure", 1))
            .expect("first status should exist")
            .security_failure_count,
        1
    );
    assert_eq!(
        engine
            .subscriber_status_by_key(&DataSetReaderKey::new("second-secure", 1))
            .expect("second status should exist")
            .security_failure_count,
        0
    );
    assert_eq!(engine.subscriber_status(1), None);
}

#[test]
fn direct_secured_replay_state_is_isolated_by_connection() {
    // Given two connections with distinct readers and targets sharing security material.
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let first_target = NodeId::new(1, "FirstReplayTarget");
    let second_target = NodeId::new(1, "SecondReplayTarget");
    for target in [&first_target, &second_target] {
        VariableBuilder::new(target, "ReplayTarget", "ReplayTarget")
            .data_type(DataTypeId::Double)
            .value(Variant::Double(0.0))
            .insert(&space);
    }
    let address_space = Arc::new(RwLock::new(space));
    let first_connection = connection_with_id(
        "first-replay",
        first_target.clone(),
        Some(MessageSecurityMode::SignAndEncrypt),
    );
    let mut second_connection = connection_with_id(
        "second-replay",
        second_target.clone(),
        Some(MessageSecurityMode::SignAndEncrypt),
    );
    second_connection.reader_groups[0].dataset_readers[0].dataset_reader_id = 2;
    let mut engine = PubSubEngine::with_connections(
        address_space.clone(),
        vec![first_connection, second_connection],
    );
    engine.register_security_group(
        SecurityGroup::new("line-a", Duration::from_secs(3600))
            .expect("security group fixture should be valid"),
    );
    let context = ContextOwned::default();
    let secured = engine
        .encode_publisher_uadp_message(
            "line-a",
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(5),
            &context.context(),
        )
        .expect("secured fixture should encode");

    // When the same valid frame enters each configured connection once.
    let first_outcome = engine
        .process_subscriber_datagram("first-replay", &secured, &context.context())
        .expect("first connection should accept the frame");

    // Then only the first connection's reader state and target are updated.
    assert_eq!(first_outcome.applied_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &first_target),
        Some(Variant::Double(5.0))
    );
    assert_eq!(
        target_value(&address_space.read(), &second_target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        engine
            .subscriber_status_by_key(&DataSetReaderKey::new("first-replay", 1))
            .expect("first reader status should exist")
            .accepted_count,
        1
    );
    assert_eq!(
        engine
            .subscriber_status_by_key(&DataSetReaderKey::new("second-replay", 2))
            .expect("second reader status should exist")
            .accepted_count,
        0
    );

    let second_outcome = engine
        .process_subscriber_datagram("second-replay", &secured, &context.context())
        .expect("second connection should independently accept the frame");

    // Then each connection has accepted the frame exactly once through its own reader.
    assert_eq!(second_outcome.applied_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &first_target),
        Some(Variant::Double(5.0))
    );
    assert_eq!(
        target_value(&address_space.read(), &second_target),
        Some(Variant::Double(5.0))
    );
    for key in [
        DataSetReaderKey::new("first-replay", 1),
        DataSetReaderKey::new("second-replay", 2),
    ] {
        let status = engine
            .subscriber_status_by_key(&key)
            .expect("connection-scoped reader status should exist");
        assert_eq!(status.accepted_count, 1);
        assert_eq!(status.security_failure_count, 0);
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

#[test]
fn direct_runtime_raw_ingress_group_secured_rejects_plaintext() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    let mut runtime = SubscriberRuntime::with_connections(address_space.clone(), vec![config])
        .expect("secured runtime fixture should be valid");
    let datagram = plaintext_datagram(10, &context.context());

    let result =
        runtime.process_datagram_for_connection("secure-conn", &datagram, &context.context());

    assert_eq!(result, Err(StatusCode::BadSecurityChecksFailed));
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("secured reader status should exist")
            .security_failure_count,
        1
    );
}

#[test]
fn direct_runtime_raw_ingress_dataset_reader_sign_and_encrypt_override_rejects_plaintext() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let mut config = connection(target.clone(), Some(MessageSecurityMode::None));
    let reader = &mut config.reader_groups[0].dataset_readers[0];
    reader.security_mode = Some(MessageSecurityMode::SignAndEncrypt);
    reader.security_policy_uri = Some(POLICY.to_uri().to_string());
    reader.security_group_id = Some("line-a".to_string());
    let mut runtime = SubscriberRuntime::with_connections(address_space.clone(), vec![config])
        .expect("reader-secured runtime fixture should be valid");
    let datagram = plaintext_datagram(11, &context.context());

    let result =
        runtime.process_datagram_for_connection("secure-conn", &datagram, &context.context());

    assert_eq!(result, Err(StatusCode::BadSecurityChecksFailed));
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("reader-secured status should exist")
            .security_failure_count,
        1
    );
}

#[test]
fn direct_runtime_raw_ingress_dataset_reader_none_override_over_secured_group_remains_allowed() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let mut config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    config.reader_groups[0].dataset_readers[0].security_mode = Some(MessageSecurityMode::None);
    let mut runtime = SubscriberRuntime::with_connections(address_space.clone(), vec![config])
        .expect("unsecured reader override fixture should be valid");
    let datagram = plaintext_datagram(12, &context.context());

    let outcome = runtime
        .process_datagram_for_connection("secure-conn", &datagram, &context.context())
        .expect("unsecured reader override should accept plaintext");

    assert_eq!(outcome.applied_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(12.0))
    );
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("unsecured reader status should exist")
            .security_failure_count,
        0
    );
}

#[test]
fn direct_runtime_raw_ingress_ambiguous_unscoped_remains_bad_invalid_argument() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let mut runtime = SubscriberRuntime::with_connections(
        address_space.clone(),
        vec![
            connection_with_id(
                "first-secure",
                target.clone(),
                Some(MessageSecurityMode::SignAndEncrypt),
            ),
            connection_with_id(
                "second-secure",
                target.clone(),
                Some(MessageSecurityMode::SignAndEncrypt),
            ),
        ],
    )
    .expect("ambiguous runtime fixture should be valid");
    let datagram = plaintext_datagram(13, &context.context());

    let result = runtime.process_datagram(&datagram, &context.context());

    assert_eq!(result, Err(StatusCode::BadInvalidArgument));
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    for connection_id in ["first-secure", "second-secure"] {
        assert_eq!(
            runtime
                .reader_status_by_key(&DataSetReaderKey::new(connection_id, 1))
                .expect("connection-scoped status should exist")
                .security_failure_count,
            0
        );
    }
}

#[test]
fn direct_runtime_raw_ingress_already_decoded_verified_message_remains_allowed() {
    let (address_space, target) = address_space_with_target();
    let config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    let mut runtime = SubscriberRuntime::with_connections(address_space.clone(), vec![config])
        .expect("secured runtime fixture should be valid");

    let outcome = runtime
        .process_network_message_for_connection("secure-conn", &message(14))
        .expect("already decoded verified message should remain allowed");

    assert_eq!(outcome.applied_readers, 1);
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(14.0))
    );
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("secured reader status should exist")
            .security_failure_count,
        0
    );
}

#[test]
fn legacy_decode_and_apply_rejects_plaintext_for_effectively_secured_reader() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    let datagram = plaintext_datagram(15, &context.context());

    let result = decode_and_apply(
        &address_space.read(),
        &datagram,
        &context.context(),
        &config.reader_groups,
    );

    assert_eq!(result, Err(StatusCode::BadSecurityChecksFailed));
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
}

#[test]
fn direct_runtime_raw_ingress_single_secured_connection_rejects_unscoped_plaintext() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    let mut runtime = SubscriberRuntime::with_connections(address_space.clone(), vec![config])
        .expect("secured runtime fixture should be valid");
    let datagram = plaintext_datagram(16, &context.context());

    let result = runtime.process_datagram(&datagram, &context.context());

    assert_eq!(result, Err(StatusCode::BadSecurityChecksFailed));
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("secured reader status should exist")
            .security_failure_count,
        1
    );
}

#[test]
fn direct_runtime_raw_ingress_unknown_connection_does_not_charge_security_failure() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let config = connection(target.clone(), Some(MessageSecurityMode::SignAndEncrypt));
    let mut runtime = SubscriberRuntime::with_connections(address_space.clone(), vec![config])
        .expect("secured runtime fixture should be valid");
    let datagram = plaintext_datagram(17, &context.context());

    let result =
        runtime.process_datagram_for_connection("missing-conn", &datagram, &context.context());

    assert_eq!(result, Err(StatusCode::BadNotFound));
    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(0.0))
    );
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("secured reader status should exist")
            .security_failure_count,
        0
    );
}

#[test]
fn direct_runtime_security_rejection_preserves_existing_reader_error() {
    let context = ContextOwned::default();
    let (address_space, target) = address_space_with_target();
    let mut config = connection(target, Some(MessageSecurityMode::SignAndEncrypt));
    config.reader_groups[0].dataset_readers[0].message_receive_timeout =
        Some(Duration::from_millis(10));
    let mut runtime = SubscriberRuntime::with_connections(address_space, vec![config])
        .expect("secured runtime fixture should be valid");
    let now = Instant::now();
    runtime
        .process_network_message_for_connection_at("secure-conn", &message(18), now)
        .expect("verified message should make the reader operational");
    runtime.check_timeouts_at(now + Duration::from_millis(11));
    assert_eq!(
        runtime
            .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
            .expect("secured reader status should exist")
            .last_error,
        Some(SubscriberError::MessageReceiveTimeout)
    );
    let datagram = plaintext_datagram(19, &context.context());

    let result =
        runtime.process_datagram_for_connection("secure-conn", &datagram, &context.context());

    assert_eq!(result, Err(StatusCode::BadSecurityChecksFailed));
    let status = runtime
        .reader_status_by_key(&DataSetReaderKey::new("secure-conn", 1))
        .expect("secured reader status should exist");
    assert_eq!(
        status.last_error,
        Some(SubscriberError::MessageReceiveTimeout)
    );
    assert_eq!(status.security_failure_count, 1);
}
