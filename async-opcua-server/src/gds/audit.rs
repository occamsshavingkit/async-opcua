use opcua_types::{NodeId, ObjectTypeId};

use crate::{
    node_manager::RequestContext,
    session::audit::{dispatch_gds_method_audit, GdsAuditEventDetails},
};

const APPLICATION_REGISTRATION_CHANGED_AUDIT_EVENT_TYPE_ID: u32 = 26;
const CERTIFICATE_REQUESTED_AUDIT_EVENT_TYPE_ID: u32 = 91;
const CERTIFICATE_DELIVERED_AUDIT_EVENT_TYPE_ID: u32 = 109;

pub(super) fn certificate_update_requested(
    context: &RequestContext,
    source_node: NodeId,
    method_id: NodeId,
) {
    dispatch_gds_method_audit(
        context,
        details(
            ObjectTypeId::CertificateUpdateRequestedAuditEventType.into(),
            source_node,
            method_id,
            "UpdateCertificate",
        ),
    );
}

pub(super) fn certificate_updated(
    context: &RequestContext,
    source_node: NodeId,
    method_id: NodeId,
    certificate_group: NodeId,
    certificate_type: NodeId,
) {
    let mut event = details(
        ObjectTypeId::CertificateUpdatedAuditEventType.into(),
        source_node,
        method_id,
        "ApplyChanges",
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
    action: &'static str,
) {
    let mut event = details(
        ObjectTypeId::TrustListUpdatedAuditEventType.into(),
        source_node,
        method_id,
        action,
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
    action: &'static str,
) {
    let event_type = NodeId::new(
        directory_object_id.namespace,
        CERTIFICATE_REQUESTED_AUDIT_EVENT_TYPE_ID,
    );
    let mut event = details(event_type, directory_object_id, method_id, action);
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
) {
    let event_type = NodeId::new(
        directory_object_id.namespace,
        CERTIFICATE_DELIVERED_AUDIT_EVENT_TYPE_ID,
    );
    let mut event = details(event_type, directory_object_id, method_id, "FinishRequest");
    event.certificate_group = Some(certificate_group);
    event.certificate_type = Some(certificate_type);
    dispatch_gds_method_audit(context, event);
}

pub(super) fn application_registration_changed(
    context: &RequestContext,
    directory_object_id: NodeId,
    method_id: NodeId,
    action: &'static str,
) {
    let event_type = NodeId::new(
        directory_object_id.namespace,
        APPLICATION_REGISTRATION_CHANGED_AUDIT_EVENT_TYPE_ID,
    );
    dispatch_gds_method_audit(
        context,
        details(event_type, directory_object_id, method_id, action),
    );
}

fn details(
    event_type: NodeId,
    source_node: NodeId,
    method_id: NodeId,
    action: &'static str,
) -> GdsAuditEventDetails {
    GdsAuditEventDetails {
        event_type,
        source_node,
        method_id,
        action,
        certificate_group: None,
        certificate_type: None,
        trust_list_id: None,
    }
}
