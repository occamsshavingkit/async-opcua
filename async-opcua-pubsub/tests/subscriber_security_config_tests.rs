//! Subscriber security configuration boundary tests.

use opcua_crypto::SecurityPolicy;
use opcua_pubsub::{
    DataSetReaderConfig, PubSubConnectionConfig, ReaderGroupConfig, WriterGroupConfig,
};
use opcua_types::{MessageSecurityMode, StatusCode};

struct SecurityFixture {
    mode: MessageSecurityMode,
    policy: SecurityPolicy,
    group_id: &'static str,
}

fn reader(reader_id: u16) -> DataSetReaderConfig {
    DataSetReaderConfig {
        name: Some(format!("reader-{reader_id}")),
        dataset_reader_id: reader_id,
        dataset_writer_id: reader_id,
        ..DataSetReaderConfig::default()
    }
}

fn secured_reader(reader_id: u16, security: SecurityFixture) -> DataSetReaderConfig {
    DataSetReaderConfig {
        security_mode: Some(security.mode),
        security_policy_uri: Some(security.policy.to_uri().to_string()),
        security_group_id: Some(security.group_id.to_string()),
        ..reader(reader_id)
    }
}

fn secured_group(
    reader_group_id: u16,
    reader_id: u16,
    security: SecurityFixture,
) -> ReaderGroupConfig {
    ReaderGroupConfig {
        reader_group_id,
        security_mode: Some(security.mode),
        security_policy_uri: Some(security.policy.to_uri().to_string()),
        security_group_id: Some(security.group_id.to_string()),
        dataset_readers: vec![reader(reader_id)],
    }
}

fn unsecured_group(reader_group_id: u16, reader_id: u16) -> ReaderGroupConfig {
    ReaderGroupConfig {
        reader_group_id,
        dataset_readers: vec![reader(reader_id)],
        ..ReaderGroupConfig::default()
    }
}

fn connection(reader_groups: Vec<ReaderGroupConfig>) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: "subscriber-security".to_string(),
        name: "subscriber-security".to_string(),
        address: "udp://127.0.0.1:4840".to_string(),
        writer_groups: Vec::<WriterGroupConfig>::new(),
        reader_groups,
    }
}

fn security(
    mode: MessageSecurityMode,
    policy: SecurityPolicy,
    group_id: &'static str,
) -> SecurityFixture {
    SecurityFixture {
        mode,
        policy,
        group_id,
    }
}

#[test]
fn validation_rejects_mixed_secure_and_unsecured_readers() {
    // Given: one secured reader and one unsecured reader on the same connection.
    let config = connection(vec![
        secured_group(
            1,
            1,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        ),
        unsecured_group(2, 2),
    ]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: heterogeneous effective security is rejected before runtime processing.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
}

#[test]
fn validation_rejects_unsecured_reader_before_secured_reader() {
    // Given: an unsecured reader precedes a secured reader on the same connection.
    let config = connection(vec![
        unsecured_group(1, 1),
        secured_group(
            2,
            2,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        ),
    ]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: heterogeneous effective security is rejected regardless of reader order.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
}

#[test]
fn validation_rejects_secure_readers_with_different_modes() {
    // Given: secured readers with identical policy/group but different effective modes.
    let config = connection(vec![
        secured_group(
            1,
            1,
            security(
                MessageSecurityMode::Sign,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        ),
        secured_group(
            2,
            2,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        ),
    ]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the mode mismatch is rejected.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
}

#[test]
fn validation_rejects_secure_readers_with_different_policies() {
    // Given: secured readers with identical mode/group but different effective policies.
    let config = connection(vec![
        secured_group(
            1,
            1,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes128Ctr,
                "line-a",
            ),
        ),
        secured_group(
            2,
            2,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        ),
    ]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the policy mismatch is rejected.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
}

#[test]
fn validation_rejects_secure_readers_with_different_security_groups() {
    // Given: secured readers with identical mode/policy but different effective groups.
    let config = connection(vec![
        secured_group(
            1,
            1,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        ),
        secured_group(
            2,
            2,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-b",
            ),
        ),
    ]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the security-group mismatch is rejected.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
}

#[test]
fn validation_rejects_invalid_effective_message_security_mode() {
    // Given: one reader group whose effective mode is Invalid at the validation boundary.
    let config = connection(vec![secured_group(
        1,
        1,
        security(
            MessageSecurityMode::Invalid,
            SecurityPolicy::PubSubAes256Ctr,
            "line-a",
        ),
    )]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the unsupported mode is rejected as invalid configuration.
    assert_eq!(result, Err(StatusCode::BadConfigurationError));
}

#[test]
fn validation_accepts_identical_effective_tuple_from_group_and_reader_overrides() {
    // Given: one reader inherits security while another overrides every field identically.
    let inherited = secured_group(
        1,
        1,
        security(
            MessageSecurityMode::SignAndEncrypt,
            SecurityPolicy::PubSubAes256Ctr,
            "line-a",
        ),
    );
    let overridden = ReaderGroupConfig {
        reader_group_id: 2,
        security_mode: Some(MessageSecurityMode::None),
        security_policy_uri: None,
        security_group_id: None,
        dataset_readers: vec![secured_reader(
            2,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        )],
    };
    let config = connection(vec![inherited, overridden]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: equivalent effective tuples are accepted regardless of override location.
    assert_eq!(result, Ok(()));
}

#[test]
fn validation_accepts_all_unsecured_readers() {
    // Given: multiple readers whose effective security state is unsecured.
    let config = connection(vec![unsecured_group(1, 1), unsecured_group(2, 2)]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the homogeneous unsecured state is accepted.
    assert_eq!(result, Ok(()));
}

#[test]
fn validation_accepts_no_reader_groups_or_readers() {
    // Given: empty connection and empty ReaderGroup edge cases.
    let no_groups = connection(Vec::new());
    let no_readers = connection(vec![ReaderGroupConfig {
        reader_group_id: 1,
        ..ReaderGroupConfig::default()
    }]);

    // When: each subscriber configuration is validated.
    let no_groups_result = no_groups.validate_subscriber_config();
    let no_readers_result = no_readers.validate_subscriber_config();

    // Then: both represent an unsecured connection and are accepted.
    assert_eq!(no_groups_result, Ok(()));
    assert_eq!(no_readers_result, Ok(()));
}

#[test]
fn validation_accepts_secure_readers_with_an_empty_group_between_them() {
    // Given: readers with identical security are separated by an empty ReaderGroup.
    let secured = |reader_group_id, reader_id| {
        secured_group(
            reader_group_id,
            reader_id,
            security(
                MessageSecurityMode::SignAndEncrypt,
                SecurityPolicy::PubSubAes256Ctr,
                "line-a",
            ),
        )
    };
    let config = connection(vec![
        secured(1, 1),
        ReaderGroupConfig {
            reader_group_id: 2,
            ..ReaderGroupConfig::default()
        },
        secured(3, 3),
    ]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the empty group is neutral and the secure tuple remains homogeneous.
    assert_eq!(result, Ok(()));
}

#[test]
fn validation_accepts_one_secure_reader() {
    // Given: a connection with exactly one valid secured reader.
    let config = connection(vec![secured_group(
        1,
        1,
        security(
            MessageSecurityMode::SignAndEncrypt,
            SecurityPolicy::PubSubAes256Ctr,
            "line-a",
        ),
    )]);

    // When: subscriber configuration is validated.
    let result = config.validate_subscriber_config();

    // Then: the single effective secure tuple is accepted.
    assert_eq!(result, Ok(()));
}
