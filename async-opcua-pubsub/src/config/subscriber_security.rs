use opcua_types::{MessageSecurityMode, StatusCode};

use super::{
    validate_unique_reader_names, DataSetReaderConfig, PubSubConnectionConfig, ReaderGroupConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubscriberSecurityConfig {
    pub(crate) security_mode: MessageSecurityMode,
    pub(crate) security_policy_uri: String,
    pub(crate) security_group_id: String,
}

pub(super) fn validated_subscriber_security(
    connection: &PubSubConnectionConfig,
) -> Result<Option<SubscriberSecurityConfig>, StatusCode> {
    let mut shared_security = None;

    // Invariant: shared_security summarizes every reader in completed reader groups.
    // Termination: each finite reader group is visited exactly once.
    for reader_group in &connection.reader_groups {
        validate_unique_reader_names(reader_group)?;

        // Invariant: shared_security is the unique effective tuple of every reader processed so far.
        // Termination: each finite DataSetReader list is visited once, without retries or awaits.
        for reader in &reader_group.dataset_readers {
            reader.validate()?;
            let effective_security = effective_security_tuple(reader_group, reader)?;
            match &shared_security {
                None => shared_security = Some(effective_security),
                Some(expected) if expected == &effective_security => {}
                Some(_) => return Err(StatusCode::BadConfigurationError),
            }
        }
    }

    Ok(shared_security
        .flatten()
        .map(
            |(security_mode, security_policy_uri, security_group_id)| SubscriberSecurityConfig {
                security_mode,
                security_policy_uri: security_policy_uri.to_owned(),
                security_group_id: security_group_id.to_owned(),
            },
        ))
}

pub(crate) fn reader_groups_require_security(
    reader_groups: &[ReaderGroupConfig],
) -> Result<bool, StatusCode> {
    for reader_group in reader_groups {
        for reader in &reader_group.dataset_readers {
            if effective_security_tuple(reader_group, reader)?.is_some() {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn effective_security_tuple<'a>(
    reader_group: &'a ReaderGroupConfig,
    reader: &'a DataSetReaderConfig,
) -> Result<Option<(MessageSecurityMode, &'a str, &'a str)>, StatusCode> {
    let Some(security_mode) = reader.security_mode.or(reader_group.security_mode) else {
        return Ok(None);
    };
    if security_mode == MessageSecurityMode::Invalid {
        return Err(StatusCode::BadConfigurationError);
    }
    if !matches!(
        security_mode,
        MessageSecurityMode::Sign | MessageSecurityMode::SignAndEncrypt
    ) {
        return Ok(None);
    }

    let security_policy_uri = reader
        .security_policy_uri
        .as_deref()
        .or(reader_group.security_policy_uri.as_deref())
        .filter(|policy| !policy.is_empty())
        .ok_or(StatusCode::BadConfigurationError)?;
    let security_group_id = reader
        .security_group_id
        .as_deref()
        .or(reader_group.security_group_id.as_deref())
        .filter(|group_id| !group_id.is_empty())
        .ok_or(StatusCode::BadConfigurationError)?;

    Ok(Some((
        security_mode,
        security_policy_uri,
        security_group_id,
    )))
}
