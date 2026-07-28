use opcua_types::{NodeId, ObjectTypeId, Variant};

use crate::{
    node_manager::RequestContext,
    session::audit::{dispatch_gds_method_audit, GdsAuditEventDetails},
};

const APPLICATION_REGISTRATION_CHANGED_AUDIT_EVENT_TYPE_ID: u32 = 26;
const CERTIFICATE_REQUESTED_AUDIT_EVENT_TYPE_ID: u32 = 91;
const CERTIFICATE_DELIVERED_AUDIT_EVENT_TYPE_ID: u32 = 109;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuditAction {
    AddCertificate,
    ApplyChanges,
    CloseAndUpdate,
    FinishRequest,
    RegisterApplication,
    RemoveCertificate,
    StartNewKeyPairRequest,
    StartSigningRequest,
    UnregisterApplication,
    UpdateApplication,
    UpdateCertificate,
    Unknown(&'static str),
}

impl From<&'static str> for AuditAction {
    fn from(action: &'static str) -> Self {
        match action {
            "AddCertificate" => Self::AddCertificate,
            "ApplyChanges" => Self::ApplyChanges,
            "CloseAndUpdate" => Self::CloseAndUpdate,
            "FinishRequest" => Self::FinishRequest,
            "RegisterApplication" => Self::RegisterApplication,
            "RemoveCertificate" => Self::RemoveCertificate,
            "StartNewKeyPairRequest" => Self::StartNewKeyPairRequest,
            "StartSigningRequest" => Self::StartSigningRequest,
            "UnregisterApplication" => Self::UnregisterApplication,
            "UpdateApplication" => Self::UpdateApplication,
            "UpdateCertificate" => Self::UpdateCertificate,
            unknown => Self::Unknown(unknown),
        }
    }
}

impl AuditAction {
    const fn display_text(self) -> &'static str {
        match self {
            Self::AddCertificate => "AddCertificate",
            Self::ApplyChanges => "ApplyChanges",
            Self::CloseAndUpdate => "CloseAndUpdate",
            Self::FinishRequest => "FinishRequest",
            Self::RegisterApplication => "RegisterApplication",
            Self::RemoveCertificate => "RemoveCertificate",
            Self::StartNewKeyPairRequest => "StartNewKeyPairRequest",
            Self::StartSigningRequest => "StartSigningRequest",
            Self::UnregisterApplication => "UnregisterApplication",
            Self::UpdateApplication => "UpdateApplication",
            Self::UpdateCertificate => "UpdateCertificate",
            Self::Unknown(action) => action,
        }
    }
}

pub(super) fn certificate_update_requested(
    context: &RequestContext,
    source_node: NodeId,
    method_id: NodeId,
    certificate_group: NodeId,
    certificate_type: NodeId,
    args: &[Variant],
) {
    let mut event = details(
        ObjectTypeId::CertificateUpdateRequestedAuditEventType.into(),
        source_node,
        method_id,
        AuditAction::UpdateCertificate,
        args,
    );
    event.certificate_group = Some(certificate_group);
    event.certificate_type = Some(certificate_type);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn certificate_updated(
    context: &RequestContext,
    source_node: NodeId,
    method_id: NodeId,
    certificate_group: NodeId,
    certificate_type: NodeId,
    args: &[Variant],
) {
    let mut event = details(
        ObjectTypeId::CertificateUpdatedAuditEventType.into(),
        source_node,
        method_id,
        AuditAction::ApplyChanges,
        args,
    );
    event.certificate_group = Some(certificate_group);
    event.certificate_type = Some(certificate_type);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn trust_list_updated(
    context: &RequestContext,
    source_node: NodeId,
    method_id: NodeId,
    trust_list_id: NodeId,
    action: AuditAction,
    args: &[Variant],
) {
    let mut event = details(
        ObjectTypeId::TrustListUpdatedAuditEventType.into(),
        source_node,
        method_id,
        action,
        args,
    );
    event.trust_list_id = Some(trust_list_id);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn trust_list_update_requested(
    context: &RequestContext,
    source_node: NodeId,
    method_id: NodeId,
    trust_list_id: NodeId,
    action: AuditAction,
    args: &[Variant],
) {
    let mut event = details(
        ObjectTypeId::TrustListUpdateRequestedAuditEventType.into(),
        source_node,
        method_id,
        action,
        args,
    );
    event.trust_list_id = Some(trust_list_id);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn certificate_requested(
    context: &RequestContext,
    directory_object_id: NodeId,
    method_id: NodeId,
    certificate_group: NodeId,
    certificate_type: NodeId,
    action: AuditAction,
    args: &[Variant],
) {
    let event_type = NodeId::new(
        directory_object_id.namespace,
        CERTIFICATE_REQUESTED_AUDIT_EVENT_TYPE_ID,
    );
    let mut event = details(event_type, directory_object_id, method_id, action, args);
    event.certificate_group = Some(certificate_group);
    event.certificate_type = Some(certificate_type);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn certificate_delivered(
    context: &RequestContext,
    directory_object_id: NodeId,
    method_id: NodeId,
    certificate_group: NodeId,
    certificate_type: NodeId,
    args: &[Variant],
) {
    let event_type = NodeId::new(
        directory_object_id.namespace,
        CERTIFICATE_DELIVERED_AUDIT_EVENT_TYPE_ID,
    );
    let mut event = details(
        event_type,
        directory_object_id,
        method_id,
        AuditAction::FinishRequest,
        args,
    );
    event.certificate_group = Some(certificate_group);
    event.certificate_type = Some(certificate_type);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn application_registration_changed(
    context: &RequestContext,
    directory_object_id: NodeId,
    method_id: NodeId,
    action: AuditAction,
    args: &[Variant],
) {
    let event_type = NodeId::new(
        directory_object_id.namespace,
        APPLICATION_REGISTRATION_CHANGED_AUDIT_EVENT_TYPE_ID,
    );
    dispatch_gds_method_audit(
        context,
        details(event_type, directory_object_id, method_id, action, args),
    );
}

fn details(
    event_type: NodeId,
    source_node: NodeId,
    method_id: NodeId,
    action: AuditAction,
    args: &[Variant],
) -> GdsAuditEventDetails {
    GdsAuditEventDetails {
        event_type,
        source_node,
        method_id,
        action: action.display_text(),
        input_arguments: sanitize_input_arguments(action, args),
        certificate_group: None,
        certificate_type: None,
        trust_list_id: None,
    }
}

// These allowlists index complete OPC UA method argument lists in specification order.
// AddCertificate(Certificate, IsTrusted) redacts Certificate.
const ADD_CERTIFICATE_VISIBLE_ARGUMENTS: &[usize] = &[1];
const APPLY_CHANGES_VISIBLE_ARGUMENTS: &[usize] = &[];
const CLOSE_AND_UPDATE_VISIBLE_ARGUMENTS: &[usize] = &[];
// FinishRequest(ApplicationId, RequestId) retains both identifiers.
const FINISH_REQUEST_VISIBLE_ARGUMENTS: &[usize] = &[0, 1];
const REGISTER_APPLICATION_VISIBLE_ARGUMENTS: &[usize] = &[0];
const REMOVE_CERTIFICATE_VISIBLE_ARGUMENTS: &[usize] = &[0, 1];
// StartNewKeyPairRequest retains fields through PrivateKeyFormat and redacts PrivateKeyPassword.
const START_NEW_KEY_PAIR_REQUEST_VISIBLE_ARGUMENTS: &[usize] = &[0, 1, 2, 3, 4, 5];
// StartSigningRequest retains its three identifiers and redacts any appended CSR material.
const START_SIGNING_REQUEST_VISIBLE_ARGUMENTS: &[usize] = &[0, 1, 2];
const UNREGISTER_APPLICATION_VISIBLE_ARGUMENTS: &[usize] = &[0];
const UPDATE_APPLICATION_VISIBLE_ARGUMENTS: &[usize] = &[0];
// UpdateCertificate retains group, type, and PrivateKeyFormat; certificate/key bytes are redacted.
const UPDATE_CERTIFICATE_VISIBLE_ARGUMENTS: &[usize] = &[0, 1, 4];
const UNKNOWN_VISIBLE_ARGUMENTS: &[usize] = &[];

fn visible_argument_indices(action: AuditAction) -> &'static [usize] {
    match action {
        AuditAction::AddCertificate => ADD_CERTIFICATE_VISIBLE_ARGUMENTS,
        AuditAction::ApplyChanges => APPLY_CHANGES_VISIBLE_ARGUMENTS,
        AuditAction::CloseAndUpdate => CLOSE_AND_UPDATE_VISIBLE_ARGUMENTS,
        AuditAction::FinishRequest => FINISH_REQUEST_VISIBLE_ARGUMENTS,
        AuditAction::RegisterApplication => REGISTER_APPLICATION_VISIBLE_ARGUMENTS,
        AuditAction::RemoveCertificate => REMOVE_CERTIFICATE_VISIBLE_ARGUMENTS,
        AuditAction::StartNewKeyPairRequest => START_NEW_KEY_PAIR_REQUEST_VISIBLE_ARGUMENTS,
        AuditAction::StartSigningRequest => START_SIGNING_REQUEST_VISIBLE_ARGUMENTS,
        AuditAction::UnregisterApplication => UNREGISTER_APPLICATION_VISIBLE_ARGUMENTS,
        AuditAction::UpdateApplication => UPDATE_APPLICATION_VISIBLE_ARGUMENTS,
        AuditAction::UpdateCertificate => UPDATE_CERTIFICATE_VISIBLE_ARGUMENTS,
        AuditAction::Unknown(_) => UNKNOWN_VISIBLE_ARGUMENTS,
    }
}

const fn expected_argument_count(action: AuditAction) -> Option<usize> {
    match action {
        AuditAction::AddCertificate => Some(2),
        AuditAction::ApplyChanges => Some(0),
        AuditAction::CloseAndUpdate => Some(1),
        AuditAction::FinishRequest => Some(2),
        AuditAction::RegisterApplication => Some(1),
        AuditAction::RemoveCertificate => Some(2),
        AuditAction::StartNewKeyPairRequest => Some(7),
        AuditAction::StartSigningRequest => Some(4),
        AuditAction::UnregisterApplication => Some(1),
        AuditAction::UpdateApplication => Some(1),
        AuditAction::UpdateCertificate => Some(6),
        AuditAction::Unknown(_) => None,
    }
}

fn sanitize_input_arguments(action: AuditAction, args: &[Variant]) -> Vec<Variant> {
    let visible_arguments = match expected_argument_count(action) {
        Some(expected_count) if args.len() == expected_count => visible_argument_indices(action),
        Some(_) | None => UNKNOWN_VISIBLE_ARGUMENTS,
    };
    args.iter()
        .enumerate()
        .map(|(index, argument)| {
            if visible_arguments.contains(&index) {
                argument.clone()
            } else {
                Variant::Empty
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
