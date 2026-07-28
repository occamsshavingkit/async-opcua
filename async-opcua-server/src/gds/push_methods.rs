//! GDS push model method callbacks (OPC UA Part 12 v1.05 §7.10, `ServerConfigurationType`).
//!
//! Every method here is registered against a real, verified `ServerConfiguration` AddressSpace
//! NodeId (confirmed via a live Read against a running server -- see
//! `specs/101-gds-push-fix/research.md`). `CreateSelfSignedCertificate`, `DeleteCertificate`, and
//! `GetCertificates` are Optional per Table 87 and are not wired here: their target nodes are
//! absent from the standard nodeset this project's code generator currently consumes.

use std::{collections::HashMap, sync::Arc};

use opcua_core::sync::RwLock;
use opcua_crypto::{gds_reload, CertificateGroup, PrivateKey, X509};
use opcua_types::{
    ByteString, LocalizedText, MessageSecurityMode, NodeId, ObjectId, ObjectTypeId, StatusCode,
    TrustListDataType, Variant,
};

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use crate::node_manager::memory::CoreNodeManager;
use crate::{node_manager::RequestContext, rbac::WellKnownRole, ServerHandle};

const SERVER_CONFIGURATION_OBJECT_ID: u32 = 12637;
const CREATE_SIGNING_REQUEST_METHOD_ID: u32 = 12737;
const UPDATE_CERTIFICATE_METHOD_ID: u32 = 13737;
const APPLY_CHANGES_METHOD_ID: u32 = 12740;
const CANCEL_CHANGES_METHOD_ID: u32 = 25708;
const GET_REJECTED_LIST_METHOD_ID: u32 = 12777;
const RESET_TO_SERVER_DEFAULTS_METHOD_ID: u32 = 25709;

/// A staged, not-yet-effective certificate and/or TrustList update, resolved by
/// `ApplyChanges`/`CancelChanges` (Part 12 §7.10.2 transaction lifecycle). This server supports
/// exactly one active transaction at a time, shared between `UpdateCertificate`
/// (`certificate_der`/`private_key_pem`) and the TrustList's `CloseAndUpdate`
/// (`pending_trust_lists`, see `gds::trust_list` and specs/102-gds-push-trustlist/data-model.md).
#[derive(Clone)]
pub(super) struct PushTransaction {
    pub(super) owning_session_id: u32,
    pub(super) certificate_der: Option<Vec<u8>>,
    pub(super) private_key_pem: Option<Vec<u8>>,
    pub(super) certificate_group_id: Option<NodeId>,
    pub(super) certificate_type_id: Option<NodeId>,
    pub(super) pending_trust_lists: HashMap<CertificateGroup, TrustListDataType>,
}

#[derive(Default)]
pub(super) enum PushTransactionState {
    #[default]
    Idle,
    Staged(PushTransaction),
    Applying {
        current: PushTransaction,
        replacement: Option<PushTransaction>,
    },
}

impl PushTransactionState {
    pub(super) fn staging_base(
        &self,
        session_id: u32,
    ) -> Result<Option<&PushTransaction>, StatusCode> {
        match self {
            Self::Idle => Ok(None),
            Self::Staged(existing) if existing.owning_session_id != session_id => {
                Err(StatusCode::BadTransactionPending)
            }
            Self::Staged(existing) => Ok(Some(existing)),
            Self::Applying { current, .. } if current.owning_session_id != session_id => {
                Err(StatusCode::BadTransactionPending)
            }
            Self::Applying { replacement, .. } => match replacement {
                Some(replacement) => Ok(Some(replacement)),
                None => Ok(None),
            },
        }
    }

    /// Stages `transaction`, replacing any update already staged by the same session.
    ///
    /// While an apply is active, the replacement must belong to `current.owning_session_id`.
    pub(super) fn stage(&mut self, transaction: PushTransaction) {
        match self {
            Self::Idle | Self::Staged(_) => *self = Self::Staged(transaction),
            Self::Applying {
                current,
                replacement,
            } => {
                debug_assert_eq!(
                    transaction.owning_session_id, current.owning_session_id,
                    "replacement transaction must belong to the active transaction's owning session"
                );
                *replacement = Some(transaction);
            }
        }
    }

    fn begin_apply(&mut self, session_id: u32) -> Result<PushTransaction, StatusCode> {
        match std::mem::take(self) {
            Self::Idle => Err(StatusCode::BadNothingToDo),
            Self::Staged(current) if current.owning_session_id != session_id => {
                *self = Self::Staged(current);
                Err(StatusCode::BadSessionIdInvalid)
            }
            Self::Staged(current) => {
                let staged = current.clone();
                *self = Self::Applying {
                    current,
                    replacement: None,
                };
                Ok(staged)
            }
            Self::Applying {
                current,
                replacement,
            } => {
                let status = if current.owning_session_id == session_id {
                    StatusCode::BadTransactionPending
                } else {
                    StatusCode::BadSessionIdInvalid
                };
                *self = Self::Applying {
                    current,
                    replacement,
                };
                Err(status)
            }
        }
    }

    fn finish_apply(&mut self, succeeded: bool) {
        *self = match std::mem::take(self) {
            Self::Applying {
                mut current,
                replacement: Some(replacement),
            } => match succeeded {
                true => Self::Staged(replacement),
                false => {
                    current
                        .pending_trust_lists
                        .extend(replacement.pending_trust_lists);
                    match replacement.certificate_der {
                        Some(certificate_der) => {
                            current.certificate_der = Some(certificate_der);
                            current.private_key_pem = replacement.private_key_pem;
                            current.certificate_group_id = replacement.certificate_group_id;
                            current.certificate_type_id = replacement.certificate_type_id;
                        }
                        None => {}
                    }
                    Self::Staged(current)
                }
            },
            Self::Applying {
                current,
                replacement: None,
            } => match succeeded {
                true => Self::Idle,
                false => Self::Staged(current),
            },
            state @ (Self::Idle | Self::Staged(_)) => state,
        };
    }
}

#[cfg(test)]
impl PushTransactionState {
    pub(super) fn staged(&self) -> Option<&PushTransaction> {
        match self {
            Self::Staged(transaction) => Some(transaction),
            Self::Idle | Self::Applying { .. } => None,
        }
    }

    pub(super) fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// In-memory state backing the GDS push model method callbacks.
#[derive(Default)]
pub struct GdsPushRegistry {
    pub(super) transaction: RwLock<PushTransactionState>,
}

/// Its [`Drop`] implementation calls `finish_apply` on normal scope exit and during unwinding.
struct ApplyTransactionGuard<'a> {
    transaction: &'a RwLock<PushTransactionState>,
    succeeded: bool,
}

impl<'a> ApplyTransactionGuard<'a> {
    fn new(transaction: &'a RwLock<PushTransactionState>) -> Self {
        Self {
            transaction,
            succeeded: false,
        }
    }

    /// Records the outcome consumed by [`Drop`]; the transition itself occurs on drop.
    fn finish(mut self, succeeded: bool) {
        self.succeeded = succeeded;
    }
}

impl Drop for ApplyTransactionGuard<'_> {
    fn drop(&mut self) {
        self.transaction.write().finish_apply(self.succeeded);
    }
}

fn resolve_certificate_identifier(
    identifier: NodeId,
    default: NodeId,
) -> Result<NodeId, StatusCode> {
    if identifier.is_null() {
        Ok(default)
    } else if identifier == default {
        Ok(identifier)
    } else {
        Err(StatusCode::BadInvalidArgument)
    }
}

impl GdsPushRegistry {
    /// Whether a transaction is currently open, owned by a session other than `session_id`.
    /// Used by methods that must not proceed while another session's transaction is pending
    /// (TrustList `Open` in write mode, `AddCertificate`, `RemoveCertificate`), without
    /// themselves reserving or creating a transaction.
    pub(super) fn transaction_pending_for_other_session(&self, session_id: u32) -> bool {
        match &*self.transaction.read() {
            PushTransactionState::Idle => false,
            PushTransactionState::Staged(transaction)
            | PushTransactionState::Applying {
                current: transaction,
                ..
            } => transaction.owning_session_id != session_id,
        }
    }
}

/// Handler for GDS push model method calls.
pub struct GdsPushMethodHandler {
    registry: Arc<GdsPushRegistry>,
    /// Used by `ResetToServerDefaults` to actually schedule a shutdown. `None` if the caller of
    /// [`register_gds_push_methods_with_registry`] didn't provide one, in which case
    /// `ResetToServerDefaults` reports `Bad_NotSupported`.
    shutdown_handle: Option<ServerHandle>,
}

impl GdsPushMethodHandler {
    /// Creates a handler backed by the supplied in-memory registry.
    pub fn new(registry: Arc<GdsPushRegistry>, shutdown_handle: Option<ServerHandle>) -> Self {
        Self {
            registry,
            shutdown_handle,
        }
    }

    /// Handles `CreateSigningRequest` (Part 12 §7.10.10): returns a PKCS#10 DER-encoded
    /// Certificate Request signed with the server's own private key.
    pub fn handle_create_signing_request(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_encrypted_security_admin(context)?;

        if args.len() < 5 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let _certificate_group_id = node_id_arg(args, 0)?;
        let _certificate_type_id = node_id_arg(args, 1)?;
        let subject_name = opt_string_arg(args, 2)?;
        let regenerate_private_key = bool_arg(args, 3)?;
        let nonce = byte_string_arg(args, 4)?;

        if regenerate_private_key && nonce.value.as_ref().map_or(0, |v| v.len()) < 32 {
            return Err(StatusCode::BadInvalidArgument);
        }

        let store = context.info.certificate_store.read();
        let pkey = store
            .read_own_pkey()
            .map_err(|_| StatusCode::BadInternalError)?;
        let own_cert = store
            .read_own_cert()
            .map_err(|_| StatusCode::BadInternalError)?;
        drop(store);

        let subject = subject_name.unwrap_or_else(|| own_cert.subject_name());
        let application_uri = context.info.application_uri.as_ref().to_owned();

        let csr_der = X509::create_signing_request(&pkey, &subject, &application_uri)
            .map_err(|_| StatusCode::BadInvalidArgument)?;

        Ok(vec![Variant::from(ByteString::from(csr_der))])
    }

    /// Handles `UpdateCertificate` (Part 12 §7.10.5): stages a new certificate (and optional
    /// private key), returning `ApplyChangesRequired = true`.
    pub fn handle_update_certificate(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.len() < 6 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let certificate_group_id = resolve_certificate_identifier(
            node_id_arg(args, 0)?,
            NodeId::from(ObjectId::ServerConfiguration_CertificateGroups_DefaultApplicationGroup),
        )?;
        let certificate_type_id = resolve_certificate_identifier(
            node_id_arg(args, 1)?,
            NodeId::from(ObjectTypeId::RsaSha256ApplicationCertificateType),
        )?;
        let certificate = non_empty_byte_string_arg(args, 2)?;
        let _issuer_certificates = args.get(3);
        let private_key_format = opt_string_arg(args, 4)?;
        let private_key = byte_string_arg(args, 5)?;

        let certificate_der: Vec<u8> = certificate
            .value
            .as_ref()
            .ok_or(StatusCode::BadInvalidArgument)?
            .to_vec();
        let new_cert =
            X509::from_der(&certificate_der).map_err(|_| StatusCode::BadCertificateInvalid)?;
        new_cert
            .is_application_uri_valid(context.info.application_uri.as_ref())
            .map_err(|_| StatusCode::BadCertificateUriInvalid)?;

        let private_key_pem: Option<Vec<u8>> = match (private_key_format, private_key.value) {
            (None, None) => None,
            (Some(format), Some(bytes)) if format.eq_ignore_ascii_case("pem") => {
                let bytes = bytes.to_vec();
                PrivateKey::from_pem(&bytes).map_err(|_| StatusCode::BadNotSupported)?;
                Some(bytes)
            }
            (Some(_), Some(_)) => return Err(StatusCode::BadNotSupported),
            _ => return Err(StatusCode::BadInvalidArgument),
        };

        {
            let mut transaction = self.registry.transaction.write();
            let pending_trust_lists = transaction
                .staging_base(context.session_id())?
                .map_or_else(HashMap::new, |existing| {
                    existing.pending_trust_lists.clone()
                });
            transaction.stage(PushTransaction {
                owning_session_id: context.session_id(),
                certificate_der: Some(certificate_der),
                private_key_pem,
                certificate_group_id: Some(certificate_group_id.clone()),
                certificate_type_id: Some(certificate_type_id.clone()),
                pending_trust_lists,
            });
        }

        #[cfg(feature = "events")]
        #[cfg(feature = "companion-gds")]
        super::audit::certificate_update_requested(
            context,
            server_configuration_object_id(),
            update_certificate_method_id(),
            certificate_group_id,
            certificate_type_id,
            args,
        );

        Ok(vec![Variant::from(true)])
    }

    /// Handles `ApplyChanges` (Part 12 §7.10.9): commits a pending certificate update and/or a
    /// pending TrustList update (Part 12 §7.8.2.5, staged by `gds::trust_list::CloseAndUpdate`).
    pub fn handle_apply_changes(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let staged = {
            let mut transaction = self.registry.transaction.write();
            transaction.begin_apply(context.session_id())?
        };
        let apply_guard = ApplyTransactionGuard::new(&self.registry.transaction);

        #[cfg(feature = "events")]
        let _changed_certificate = staged
            .certificate_group_id
            .clone()
            .zip(staged.certificate_type_id.clone());
        #[cfg(feature = "events")]
        let _changed_trust_lists: Vec<_> = staged.pending_trust_lists.keys().copied().collect();

        let apply_result: Result<(), StatusCode> = (|| {
            if let Some(certificate_der) = staged.certificate_der {
                let store = context.info.certificate_store.read();
                let pkey_pem = match staged.private_key_pem {
                    Some(pem) => pem,
                    None => store
                        .read_own_pkey()
                        .map_err(|_| StatusCode::BadInternalError)?
                        .to_pem()
                        .map_err(|_| StatusCode::BadInternalError)?
                        .into_bytes(),
                };
                gds_reload::save_new_credentials(&store, &certificate_der, &pkey_pem)
                    .map_err(|_| StatusCode::BadInternalError)?;
                let (cert, pkey) = gds_reload::reload_store_from_disk(&store)
                    .map_err(|_| StatusCode::BadInternalError)?;
                drop(store);

                let mut endpoint_certs = context.info.endpoint_certificates.write();
                for entry in endpoint_certs.values_mut() {
                    *entry = Some((cert.clone(), pkey.clone()));
                }
            }

            if !staged.pending_trust_lists.is_empty() {
                let store = context.info.certificate_store.read();
                for (certificate_group, trust_list) in staged.pending_trust_lists {
                    super::trust_list::apply_trust_list_update(
                        &store,
                        certificate_group,
                        &trust_list,
                    )
                    .map_err(|_| StatusCode::BadInternalError)?;
                }
            }

            Ok(())
        })();

        apply_guard.finish(apply_result.is_ok());
        apply_result?;

        #[cfg(feature = "events")]
        #[cfg(feature = "companion-gds")]
        if let Some((certificate_group, certificate_type)) = _changed_certificate {
            super::audit::certificate_updated(
                context,
                server_configuration_object_id(),
                apply_changes_method_id(),
                certificate_group,
                certificate_type,
                &[],
            );
        }
        #[cfg(feature = "events")]
        #[cfg(feature = "companion-gds")]
        for certificate_group in _changed_trust_lists {
            super::audit::trust_list_updated(
                context,
                server_configuration_object_id(),
                apply_changes_method_id(),
                super::trust_list::trust_list_object_id_for_group(certificate_group),
                super::audit::AuditAction::ApplyChanges,
                &[],
            );
        }

        Ok(vec![])
    }

    /// Handles `CancelChanges` (Part 12 §7.10.11): discards a pending certificate update.
    pub fn handle_cancel_changes(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let mut transaction = self.registry.transaction.write();
        match &*transaction {
            PushTransactionState::Idle => Err(StatusCode::BadNothingToDo),
            PushTransactionState::Staged(existing)
                if existing.owning_session_id != context.session_id() =>
            {
                Err(StatusCode::BadSessionIdInvalid)
            }
            PushTransactionState::Staged(_) => {
                *transaction = PushTransactionState::Idle;
                Ok(vec![])
            }
            PushTransactionState::Applying { current, .. }
                if current.owning_session_id != context.session_id() =>
            {
                Err(StatusCode::BadSessionIdInvalid)
            }
            PushTransactionState::Applying { .. } => Err(StatusCode::BadTransactionPending),
        }
    }

    /// Handles `GetRejectedList` (Part 12 §7.10.12): returns certificates the server has
    /// rejected. Implemented directly against the certificate store's rejected-certs directory
    /// rather than delegating to `DefaultApplicationGroup.GetRejectedList` (§7.8.3.2), whose node
    /// was confirmed absent from this server's default AddressSpace.
    pub fn handle_get_rejected_list(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let store = context.info.certificate_store.read();
        let certs = store.read_rejected_certs();
        let der: Vec<ByteString> = certs.iter().map(X509::as_byte_string).collect();

        Ok(vec![Variant::from(der)])
    }

    /// Handles `ResetToServerDefaults` (Part 12 §7.10.13): warns clients and schedules a
    /// shutdown. Returns `Bad_NotSupported` if this handler was registered without a
    /// [`ServerHandle`] to actually schedule the shutdown with.
    pub fn handle_reset_to_server_defaults(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let Some(handle) = &self.shutdown_handle else {
            return Err(StatusCode::BadNotSupported);
        };

        handle.shutdown_after(
            std::time::Duration::from_secs(5),
            LocalizedText::new(
                "en",
                "Server security configuration is being reset to defaults; credentials issued \
                 before this reset may no longer be valid after restart.",
            ),
        );

        Ok(vec![])
    }
}

/// Returns the standard `ServerConfiguration` object id.
pub fn server_configuration_object_id() -> NodeId {
    NodeId::new(0, SERVER_CONFIGURATION_OBJECT_ID)
}

/// Returns the standard `CreateSigningRequest` method id.
pub fn create_signing_request_method_id() -> NodeId {
    NodeId::new(0, CREATE_SIGNING_REQUEST_METHOD_ID)
}

/// Returns the standard `UpdateCertificate` method id.
pub fn update_certificate_method_id() -> NodeId {
    NodeId::new(0, UPDATE_CERTIFICATE_METHOD_ID)
}

/// Returns the standard `ApplyChanges` method id.
pub fn apply_changes_method_id() -> NodeId {
    NodeId::new(0, APPLY_CHANGES_METHOD_ID)
}

/// Returns the standard `CancelChanges` method id.
pub fn cancel_changes_method_id() -> NodeId {
    NodeId::new(0, CANCEL_CHANGES_METHOD_ID)
}

/// Returns the standard `GetRejectedList` method id.
pub fn get_rejected_list_method_id() -> NodeId {
    NodeId::new(0, GET_REJECTED_LIST_METHOD_ID)
}

/// Returns the standard `ResetToServerDefaults` method id.
pub fn reset_to_server_defaults_method_id() -> NodeId {
    NodeId::new(0, RESET_TO_SERVER_DEFAULTS_METHOD_ID)
}

/// Registers GDS push method callbacks on the core (namespace 0) node manager -- `ServerConfiguration`
/// and its methods are standard namespace-0 nodes, owned by [`CoreNodeManager`], not a
/// user-added [`SimpleNodeManager`](crate::node_manager::memory::SimpleNodeManager). Without
/// shutdown support for `ResetToServerDefaults` (it will report `Bad_NotSupported`). Prefer
/// [`register_gds_push_methods_with_handle`] when a [`ServerHandle`] is available.
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_push_methods(node_manager: &CoreNodeManager) -> Arc<GdsPushRegistry> {
    let registry = Arc::new(GdsPushRegistry::default());
    register_gds_push_methods_with_registry(node_manager, registry.clone(), None);
    registry
}

/// Registers GDS push method callbacks on the core node manager, wiring `ResetToServerDefaults`
/// to actually schedule a shutdown via the supplied [`ServerHandle`].
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_push_methods_with_handle(
    node_manager: &CoreNodeManager,
    handle: ServerHandle,
) -> Arc<GdsPushRegistry> {
    let registry = Arc::new(GdsPushRegistry::default());
    register_gds_push_methods_with_registry(node_manager, registry.clone(), Some(handle));
    registry
}

/// Registers GDS push method callbacks using the supplied registry.
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_gds_push_methods_with_registry(
    node_manager: &CoreNodeManager,
    registry: Arc<GdsPushRegistry>,
    shutdown_handle: Option<ServerHandle>,
) {
    let handler = Arc::new(GdsPushMethodHandler::new(registry, shutdown_handle));

    let h = handler.clone();
    node_manager.inner().add_method_callback_with_context(
        create_signing_request_method_id(),
        move |ctx, _id, args| h.handle_create_signing_request(ctx, args),
    );

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(update_certificate_method_id(), move |ctx, _id, args| {
            h.handle_update_certificate(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(apply_changes_method_id(), move |ctx, _id, _args| {
            h.handle_apply_changes(ctx)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(cancel_changes_method_id(), move |ctx, _id, _args| {
            h.handle_cancel_changes(ctx)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(get_rejected_list_method_id(), move |ctx, _id, _args| {
            h.handle_get_rejected_list(ctx)
        });

    let h = handler;
    node_manager.inner().add_method_callback_with_context(
        reset_to_server_defaults_method_id(),
        move |ctx, _id, _args| h.handle_reset_to_server_defaults(ctx),
    );
}

pub(super) fn authorize_encrypted_security_admin(
    context: &RequestContext,
) -> Result<(), StatusCode> {
    if context.security_mode() != MessageSecurityMode::SignAndEncrypt {
        return Err(StatusCode::BadSecurityModeInsufficient);
    }
    require_security_admin(context)
}

pub(super) fn authorize_authenticated_security_admin(
    context: &RequestContext,
) -> Result<(), StatusCode> {
    if context.security_mode() == MessageSecurityMode::None {
        return Err(StatusCode::BadSecurityModeInsufficient);
    }
    require_security_admin(context)
}

pub(super) fn require_security_admin(context: &RequestContext) -> Result<(), StatusCode> {
    if !context
        .user_roles()
        .contains(&WellKnownRole::SecurityAdmin.node_id())
    {
        return Err(StatusCode::BadUserAccessDenied);
    }
    Ok(())
}

pub(super) fn node_id_arg(args: &[Variant], index: usize) -> Result<NodeId, StatusCode> {
    match args.get(index) {
        Some(Variant::NodeId(node_id)) => Ok(node_id.as_ref().clone()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

pub(super) fn byte_string_arg(args: &[Variant], index: usize) -> Result<ByteString, StatusCode> {
    match args.get(index) {
        Some(Variant::ByteString(value)) => Ok(value.clone()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

pub(super) fn non_empty_byte_string_arg(
    args: &[Variant],
    index: usize,
) -> Result<ByteString, StatusCode> {
    let value = byte_string_arg(args, index)?;
    if value.is_null_or_empty() {
        Err(StatusCode::BadInvalidArgument)
    } else {
        Ok(value)
    }
}

pub(super) fn bool_arg(args: &[Variant], index: usize) -> Result<bool, StatusCode> {
    match args.get(index) {
        Some(Variant::Boolean(value)) => Ok(*value),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

pub(super) fn opt_string_arg(args: &[Variant], index: usize) -> Result<Option<String>, StatusCode> {
    match args.get(index) {
        Some(Variant::String(value)) if value.is_null() => Ok(None),
        Some(Variant::String(value)) => Ok(Some(value.as_ref().to_owned())),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use opcua_core::sync::RwLock;
    use opcua_crypto::SecurityPolicy;
    use opcua_types::{
        AnonymousIdentityToken, ApplicationDescription, ByteString, MessageSecurityMode, NodeId,
        StatusCode, UAString, Variant,
    };

    use crate::{
        authenticator::UserToken,
        identity_token::IdentityToken,
        node_manager::{RequestContext, RequestContextInner},
        session::instance::Session,
        ServerBuilder,
    };

    use super::*;

    fn unique_test_pki_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        tempfile::Builder::new()
            .prefix(&format!(
                "async-opcua-gds-push-test-pki-{}-{id}-",
                std::process::id()
            ))
            .tempdir()
            .expect("failed to create a securely-permissioned test PKI directory")
            .keep()
    }

    fn request_context(
        security_mode: MessageSecurityMode,
        user_roles: Vec<NodeId>,
    ) -> (RequestContext, crate::ServerHandle) {
        let (_server, handle) = ServerBuilder::new_anonymous("gds push method test")
            .without_node_managers()
            .pki_dir(unique_test_pki_dir())
            .create_sample_keypair(true)
            .build()
            .expect("test server should build");
        let info = Arc::clone(handle.info());
        let user_roles = Arc::new(user_roles);
        let session = Arc::new(RwLock::new(Session::create(
            &info,
            NodeId::new(0, 1),
            1,
            60_000,
            0,
            0,
            UAString::from("opc.tcp://localhost"),
            SecurityPolicy::Basic256Sha256.to_uri().to_string(),
            IdentityToken::Anonymous(AnonymousIdentityToken {
                policy_id: UAString::from("anonymous"),
            }),
            None,
            ByteString::null(),
            UAString::from("gds-push-method-test"),
            ApplicationDescription::default(),
            security_mode,
        )));

        let context = RequestContext::new_test(Arc::new(RequestContextInner {
            session,
            session_id: 1,
            authenticator: info.authenticator.clone(),
            token: UserToken("gds-push-method-test".to_string()),
            user_roles,
            type_tree: info.type_tree.clone(),
            type_tree_getter: info.type_tree_getter.clone(),
            subscriptions: handle.subscriptions().clone(),
            info,
        }));
        (context, handle)
    }

    fn security_admin_request_context(
        security_mode: MessageSecurityMode,
    ) -> (RequestContext, crate::ServerHandle) {
        request_context(security_mode, vec![WellKnownRole::SecurityAdmin.node_id()])
    }

    #[tokio::test]
    async fn create_signing_request_returns_a_parseable_csr() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        let args = vec![
            Variant::from(NodeId::null()),
            Variant::from(NodeId::null()),
            Variant::from(UAString::null()),
            Variant::from(false),
            Variant::from(ByteString::null()),
        ];

        let outputs = handler
            .handle_create_signing_request(&context, &args)
            .expect("valid request should succeed");

        assert_eq!(outputs.len(), 1);
        let Variant::ByteString(csr) = &outputs[0] else {
            panic!("expected ByteString output, got {:?}", outputs[0]);
        };
        let der: Vec<u8> = csr.value.as_ref().expect("csr should not be null").to_vec();
        let parsed =
            x509_cert::request::CertReq::try_from(der.as_slice()).expect("csr should be valid DER");
        assert_eq!(parsed.info.version, x509_cert::request::Version::V1);
    }

    #[tokio::test]
    async fn create_signing_request_requires_encrypted_channel() {
        let (context, _handle) = security_admin_request_context(MessageSecurityMode::Sign);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        let args = vec![
            Variant::from(NodeId::null()),
            Variant::from(NodeId::null()),
            Variant::from(UAString::null()),
            Variant::from(false),
            Variant::from(ByteString::null()),
        ];

        assert_eq!(
            handler.handle_create_signing_request(&context, &args),
            Err(StatusCode::BadSecurityModeInsufficient)
        );
    }

    #[tokio::test]
    async fn create_signing_request_requires_security_admin() {
        let (context, _handle) = request_context(
            MessageSecurityMode::SignAndEncrypt,
            vec![WellKnownRole::AuthenticatedUser.node_id()],
        );
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        let args = vec![
            Variant::from(NodeId::null()),
            Variant::from(NodeId::null()),
            Variant::from(UAString::null()),
            Variant::from(false),
            Variant::from(ByteString::null()),
        ];

        assert_eq!(
            handler.handle_create_signing_request(&context, &args),
            Err(StatusCode::BadUserAccessDenied)
        );
    }

    fn self_signed_cert_der(context: &RequestContext) -> Vec<u8> {
        let store = context.info.certificate_store.read();
        let cert = store.read_own_cert().expect("own cert should exist");
        cert.to_der().expect("cert should encode to DER")
    }

    fn update_certificate_args(cert_der: Vec<u8>) -> Vec<Variant> {
        vec![
            Variant::from(NodeId::null()),
            Variant::from(NodeId::null()),
            Variant::from(ByteString::from(cert_der)),
            Variant::Empty,
            Variant::from(UAString::null()),
            Variant::from(ByteString::null()),
        ]
    }

    #[tokio::test]
    async fn update_certificate_rejects_invalid_certificate_group_id_before_parsing() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let mut args = update_certificate_args(vec![0x01, 0x02, 0x03]);
        args[0] = Variant::from(NodeId::new(0, 999_999u32));

        assert_eq!(
            handler.handle_update_certificate(&context, &args),
            Err(StatusCode::BadInvalidArgument)
        );
        assert!(handler.registry.transaction.read().is_idle());
    }

    #[tokio::test]
    async fn update_certificate_rejects_invalid_certificate_type_id_before_parsing() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let mut args = update_certificate_args(vec![0x01, 0x02, 0x03]);
        args[1] = Variant::from(NodeId::new(0, 999_999u32));

        assert_eq!(
            handler.handle_update_certificate(&context, &args),
            Err(StatusCode::BadInvalidArgument)
        );
        assert!(handler.registry.transaction.read().is_idle());
    }

    #[tokio::test]
    async fn update_certificate_stages_a_transaction_and_requires_apply_changes() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let cert_der = self_signed_cert_der(&context);

        let outputs = handler
            .handle_update_certificate(&context, &update_certificate_args(cert_der))
            .expect("valid update should succeed");

        assert_eq!(outputs, vec![Variant::from(true)]);
        assert!(handler.registry.transaction.read().staged().is_some());
    }

    #[tokio::test]
    async fn update_certificate_normalizes_null_ids_to_effective_defaults() {
        // OPC-10000-12 §7.10.5: null IDs resolve to their effective certificate defaults.
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let cert_der = self_signed_cert_der(&context);

        handler
            .handle_update_certificate(&context, &update_certificate_args(cert_der))
            .expect("valid update should succeed");

        let transaction = handler.registry.transaction.read();
        let staged = transaction
            .staged()
            .expect("update should stage a transaction");
        assert_eq!(
            staged.certificate_group_id,
            Some(NodeId::from(
                ObjectId::ServerConfiguration_CertificateGroups_DefaultApplicationGroup,
            ))
        );
        assert_eq!(
            staged.certificate_type_id,
            Some(NodeId::from(
                ObjectTypeId::RsaSha256ApplicationCertificateType
            ))
        );
    }

    #[tokio::test]
    async fn update_certificate_from_a_second_session_is_rejected_while_transaction_open() {
        let registry = Arc::new(GdsPushRegistry::default());
        let handler = GdsPushMethodHandler::new(registry, None);
        let (context1, _h1) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let cert_der = self_signed_cert_der(&context1);

        handler
            .handle_update_certificate(&context1, &update_certificate_args(cert_der.clone()))
            .expect("first update should succeed");

        let (context2_base, _h2) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        // Give the second context a different session id to simulate a distinct session.
        let context2 = RequestContext::new_test(Arc::new(RequestContextInner {
            session: context2_base.session.clone(),
            session_id: 2,
            authenticator: context2_base.authenticator.clone(),
            token: context2_base.token.clone(),
            user_roles: context2_base.user_roles.clone(),
            type_tree: context2_base.type_tree.clone(),
            type_tree_getter: context2_base.type_tree_getter.clone(),
            subscriptions: context2_base.subscriptions.clone(),
            info: context2_base.info.clone(),
        }));

        assert_eq!(
            handler.handle_update_certificate(&context2, &update_certificate_args(cert_der)),
            Err(StatusCode::BadTransactionPending)
        );
    }

    #[tokio::test]
    async fn apply_changes_commits_the_staged_certificate() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let cert_der = self_signed_cert_der(&context);

        handler
            .handle_update_certificate(&context, &update_certificate_args(cert_der.clone()))
            .expect("update should succeed");

        handler
            .handle_apply_changes(&context)
            .expect("apply changes should succeed");

        assert!(handler.registry.transaction.read().is_idle());
        let store = context.info.certificate_store.read();
        let applied = store.read_own_cert().expect("own cert should exist");
        assert_eq!(applied.to_der().expect("cert should encode"), cert_der);
    }

    #[tokio::test]
    async fn apply_changes_failure_keeps_the_owning_sessions_transaction_staged() {
        // Given
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let cert_der = self_signed_cert_der(&context);
        handler
            .handle_update_certificate(&context, &update_certificate_args(cert_der))
            .expect("update should succeed");
        let cert_path = context.info.certificate_store.read().own_certificate_path();
        std::fs::remove_file(&cert_path).expect("own certificate file should be removable");
        std::fs::create_dir(&cert_path)
            .expect("directory at the certificate path should force a real write failure");

        // When
        let result = handler.handle_apply_changes(&context);

        // Then
        assert_eq!(result, Err(StatusCode::BadInternalError));
        let transaction = handler.registry.transaction.read();
        let staged = transaction.staged().expect(
            "failed ApplyChanges must keep the owning session's transaction staged for retry",
        );
        assert_eq!(staged.owning_session_id, context.session_id());
    }

    #[test]
    fn successful_finish_keeps_identical_same_session_replacement_staged() {
        // Given
        const SESSION_ID: u32 = 1;
        let certificate_der = vec![1, 2, 3];
        let transaction = PushTransaction {
            owning_session_id: SESSION_ID,
            certificate_der: Some(certificate_der.clone()),
            private_key_pem: None,
            certificate_group_id: None,
            certificate_type_id: None,
            pending_trust_lists: HashMap::new(),
        };
        let replacement = transaction.clone();
        let mut state = PushTransactionState::Staged(transaction);
        state
            .begin_apply(SESSION_ID)
            .expect("first apply should begin");

        // When
        state.stage(replacement);
        state.finish_apply(true);

        // Then
        let staged = state
            .staged()
            .expect("successful apply must preserve a same-session replacement");
        assert_eq!(staged.owning_session_id, SESSION_ID);
        assert_eq!(
            staged.certificate_der.as_deref(),
            Some(certificate_der.as_slice())
        );
    }

    #[test]
    fn failed_finish_combines_current_work_with_certificate_replacement() {
        // Given
        const SESSION_ID: u32 = 1;
        let current_certificate_der = vec![1, 2, 3];
        let current_private_key_pem = vec![4, 5, 6];
        let current_certificate_group_id = NodeId::new(0, 10u32);
        let current_certificate_type_id = NodeId::new(0, 11u32);
        let replacement_certificate_der = vec![7, 8, 9];
        let replacement_private_key_pem = vec![10, 11, 12];
        let replacement_certificate_group_id = NodeId::new(0, 20u32);
        let replacement_certificate_type_id = NodeId::new(0, 21u32);
        let current = PushTransaction {
            owning_session_id: SESSION_ID,
            certificate_der: Some(current_certificate_der),
            private_key_pem: Some(current_private_key_pem),
            certificate_group_id: Some(current_certificate_group_id),
            certificate_type_id: Some(current_certificate_type_id),
            pending_trust_lists: HashMap::from([
                (
                    CertificateGroup::DefaultApplication,
                    TrustListDataType {
                        specified_lists: 1,
                        ..TrustListDataType::default()
                    },
                ),
                (
                    CertificateGroup::DefaultHttps,
                    TrustListDataType {
                        specified_lists: 2,
                        ..TrustListDataType::default()
                    },
                ),
            ]),
        };
        let replacement = PushTransaction {
            owning_session_id: SESSION_ID,
            certificate_der: Some(replacement_certificate_der.clone()),
            private_key_pem: Some(replacement_private_key_pem.clone()),
            certificate_group_id: Some(replacement_certificate_group_id.clone()),
            certificate_type_id: Some(replacement_certificate_type_id.clone()),
            pending_trust_lists: HashMap::from([
                (
                    CertificateGroup::DefaultApplication,
                    TrustListDataType {
                        specified_lists: 3,
                        ..TrustListDataType::default()
                    },
                ),
                (
                    CertificateGroup::DefaultUserToken,
                    TrustListDataType {
                        specified_lists: 4,
                        ..TrustListDataType::default()
                    },
                ),
            ]),
        };
        let mut state = PushTransactionState::Staged(current);
        state
            .begin_apply(SESSION_ID)
            .expect("first apply should begin");
        state.stage(replacement);

        // When
        state.finish_apply(false);

        // Then
        let staged = state
            .staged()
            .expect("failed apply must combine current work with its replacement");
        assert_eq!(
            staged.certificate_der.as_deref(),
            Some(replacement_certificate_der.as_slice())
        );
        assert_eq!(
            staged.private_key_pem.as_deref(),
            Some(replacement_private_key_pem.as_slice())
        );
        assert_eq!(
            staged.certificate_group_id,
            Some(replacement_certificate_group_id)
        );
        assert_eq!(
            staged.certificate_type_id,
            Some(replacement_certificate_type_id)
        );
        assert_eq!(staged.pending_trust_lists.len(), 3);
        assert_eq!(
            staged
                .pending_trust_lists
                .get(&CertificateGroup::DefaultApplication)
                .map(|trust_list| trust_list.specified_lists),
            Some(3)
        );
        assert_eq!(
            staged
                .pending_trust_lists
                .get(&CertificateGroup::DefaultHttps)
                .map(|trust_list| trust_list.specified_lists),
            Some(2)
        );
        assert_eq!(
            staged
                .pending_trust_lists
                .get(&CertificateGroup::DefaultUserToken)
                .map(|trust_list| trust_list.specified_lists),
            Some(4)
        );
    }

    #[test]
    fn failed_finish_without_replacement_certificate_keeps_current_certificate_tuple() {
        // Given
        const SESSION_ID: u32 = 1;
        let current_certificate_der = vec![1, 2, 3];
        let current_private_key_pem = vec![4, 5, 6];
        let current_certificate_group_id = NodeId::new(0, 10u32);
        let current_certificate_type_id = NodeId::new(0, 11u32);
        let current = PushTransaction {
            owning_session_id: SESSION_ID,
            certificate_der: Some(current_certificate_der.clone()),
            private_key_pem: Some(current_private_key_pem.clone()),
            certificate_group_id: Some(current_certificate_group_id.clone()),
            certificate_type_id: Some(current_certificate_type_id.clone()),
            pending_trust_lists: HashMap::from([(
                CertificateGroup::DefaultApplication,
                TrustListDataType {
                    specified_lists: 1,
                    ..TrustListDataType::default()
                },
            )]),
        };
        let replacement = PushTransaction {
            owning_session_id: SESSION_ID,
            certificate_der: None,
            private_key_pem: Some(vec![7, 8, 9]),
            certificate_group_id: Some(NodeId::new(0, 20u32)),
            certificate_type_id: Some(NodeId::new(0, 21u32)),
            pending_trust_lists: HashMap::from([(
                CertificateGroup::DefaultUserToken,
                TrustListDataType {
                    specified_lists: 2,
                    ..TrustListDataType::default()
                },
            )]),
        };
        let mut state = PushTransactionState::Staged(current);
        state
            .begin_apply(SESSION_ID)
            .expect("first apply should begin");
        state.stage(replacement);

        // When
        state.finish_apply(false);

        // Then
        let staged = state
            .staged()
            .expect("failed apply must preserve the current certificate tuple");
        assert_eq!(
            staged.certificate_der.as_deref(),
            Some(current_certificate_der.as_slice())
        );
        assert_eq!(
            staged.private_key_pem.as_deref(),
            Some(current_private_key_pem.as_slice())
        );
        assert_eq!(
            staged.certificate_group_id,
            Some(current_certificate_group_id)
        );
        assert_eq!(
            staged.certificate_type_id,
            Some(current_certificate_type_id)
        );
        assert_eq!(staged.pending_trust_lists.len(), 2);
        assert_eq!(
            staged
                .pending_trust_lists
                .get(&CertificateGroup::DefaultApplication)
                .map(|trust_list| trust_list.specified_lists),
            Some(1)
        );
        assert_eq!(
            staged
                .pending_trust_lists
                .get(&CertificateGroup::DefaultUserToken)
                .map(|trust_list| trust_list.specified_lists),
            Some(2)
        );
    }

    #[test]
    fn staging_base_hides_current_fields_while_apply_is_active() {
        // Given
        const SESSION_ID: u32 = 1;
        let state = PushTransactionState::Applying {
            current: PushTransaction {
                owning_session_id: SESSION_ID,
                certificate_der: Some(vec![1, 2, 3]),
                private_key_pem: None,
                certificate_group_id: None,
                certificate_type_id: None,
                pending_trust_lists: HashMap::from([(
                    CertificateGroup::DefaultApplication,
                    TrustListDataType {
                        specified_lists: 1,
                        trusted_certificates: Some(vec![ByteString::from(vec![4, 5, 6])]),
                        trusted_crls: None,
                        issuer_certificates: None,
                        issuer_crls: None,
                    },
                )]),
            },
            replacement: None,
        };

        // When
        let staging_base = state
            .staging_base(SESSION_ID)
            .expect("owning session should be allowed to stage a replacement");

        // Then
        assert!(
            staging_base.is_none(),
            "the in-flight certificate and trust-list fields must not seed a replacement"
        );
    }

    #[test]
    fn begin_apply_from_owning_session_is_rejected_while_apply_is_active() {
        // Given
        const SESSION_ID: u32 = 1;
        let transaction = PushTransaction {
            owning_session_id: SESSION_ID,
            certificate_der: Some(vec![1, 2, 3]),
            private_key_pem: None,
            certificate_group_id: None,
            certificate_type_id: None,
            pending_trust_lists: HashMap::new(),
        };
        let mut state = PushTransactionState::Staged(transaction);
        state
            .begin_apply(SESSION_ID)
            .expect("first apply should begin");

        // When
        let duplicate = state.begin_apply(SESSION_ID);

        // Then
        assert!(matches!(duplicate, Err(StatusCode::BadTransactionPending)));
        assert!(matches!(
            state,
            PushTransactionState::Applying {
                current,
                replacement: None,
            } if current.owning_session_id == SESSION_ID
        ));
    }

    #[test]
    fn apply_guard_restores_staged_transaction_on_unwind() {
        // Given
        const SESSION_ID: u32 = 1;
        let registry = GdsPushRegistry::default();
        {
            let mut state = registry.transaction.write();
            state.stage(PushTransaction {
                owning_session_id: SESSION_ID,
                certificate_der: Some(vec![1, 2, 3]),
                private_key_pem: None,
                certificate_group_id: None,
                certificate_type_id: None,
                pending_trust_lists: HashMap::new(),
            });
            state.begin_apply(SESSION_ID).expect("apply should begin");
        }

        // When
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = ApplyTransactionGuard::new(&registry.transaction);
            panic!("simulated panic while applying changes");
        }));

        // Then
        assert!(unwind.is_err());
        let state = registry.transaction.read();
        let staged = state
            .staged()
            .expect("unwind cleanup must restore the current transaction");
        assert_eq!(staged.owning_session_id, SESSION_ID);
    }

    #[tokio::test]
    async fn apply_changes_with_no_transaction_reports_nothing_to_do() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        assert_eq!(
            handler.handle_apply_changes(&context),
            Err(StatusCode::BadNothingToDo)
        );
    }

    #[tokio::test]
    async fn cancel_changes_discards_the_staged_certificate() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);
        let cert_der = self_signed_cert_der(&context);
        let original_der = self_signed_cert_der(&context);

        handler
            .handle_update_certificate(&context, &update_certificate_args(cert_der))
            .expect("update should succeed");

        handler
            .handle_cancel_changes(&context)
            .expect("cancel should succeed");

        assert!(handler.registry.transaction.read().is_idle());
        let store = context.info.certificate_store.read();
        let unchanged = store.read_own_cert().expect("own cert should exist");
        assert_eq!(
            unchanged.to_der().expect("cert should encode"),
            original_der
        );
    }

    #[tokio::test]
    async fn cancel_changes_with_no_transaction_reports_nothing_to_do() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        assert_eq!(
            handler.handle_cancel_changes(&context),
            Err(StatusCode::BadNothingToDo)
        );
    }

    #[tokio::test]
    async fn apply_changes_commits_a_pending_trust_list_update() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let registry = Arc::new(GdsPushRegistry::default());
        let handler = GdsPushMethodHandler::new(registry.clone(), None);
        let original_own_cert_der = self_signed_cert_der(&context);
        let new_trusted_der = self_signed_cert_der(&context);

        *registry.transaction.write() = PushTransactionState::Staged(PushTransaction {
            owning_session_id: context.session_id(),
            certificate_der: None,
            private_key_pem: None,
            certificate_group_id: None,
            certificate_type_id: None,
            pending_trust_lists: HashMap::from([(
                CertificateGroup::DefaultApplication,
                TrustListDataType {
                    specified_lists: 1, // TrustListMasks::TrustedCertificates
                    trusted_certificates: Some(vec![ByteString::from(new_trusted_der)]),
                    trusted_crls: None,
                    issuer_certificates: None,
                    issuer_crls: None,
                },
            )]),
        });

        handler
            .handle_apply_changes(&context)
            .expect("apply changes should succeed");

        assert!(handler.registry.transaction.read().is_idle());
        let store = context.info.certificate_store.read();
        // The TrustList update must not affect the server's own application certificate.
        let own_cert = store.read_own_cert().expect("own cert should exist");
        assert_eq!(
            own_cert.to_der().expect("cert should encode"),
            original_own_cert_der
        );
        assert_eq!(store.read_trusted_certs().len(), 1);
    }

    #[tokio::test]
    async fn cancel_changes_discards_a_pending_trust_list_update() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let registry = Arc::new(GdsPushRegistry::default());
        let handler = GdsPushMethodHandler::new(registry.clone(), None);

        *registry.transaction.write() = PushTransactionState::Staged(PushTransaction {
            owning_session_id: context.session_id(),
            certificate_der: None,
            private_key_pem: None,
            certificate_group_id: None,
            certificate_type_id: None,
            pending_trust_lists: HashMap::from([(
                CertificateGroup::DefaultApplication,
                TrustListDataType {
                    specified_lists: 1,
                    trusted_certificates: Some(vec![ByteString::from(self_signed_cert_der(
                        &context,
                    ))]),
                    trusted_crls: None,
                    issuer_certificates: None,
                    issuer_crls: None,
                },
            )]),
        });

        handler
            .handle_cancel_changes(&context)
            .expect("cancel should succeed");

        assert!(handler.registry.transaction.read().is_idle());
        assert!(context
            .info
            .certificate_store
            .read()
            .read_trusted_certs()
            .is_empty());
    }

    #[tokio::test]
    async fn update_certificate_is_rejected_while_a_trust_list_transaction_is_open_elsewhere() {
        let registry = Arc::new(GdsPushRegistry::default());
        let handler = GdsPushMethodHandler::new(registry.clone(), None);
        *registry.transaction.write() = PushTransactionState::Staged(PushTransaction {
            owning_session_id: 1,
            certificate_der: None,
            private_key_pem: None,
            certificate_group_id: None,
            certificate_type_id: None,
            pending_trust_lists: HashMap::from([(
                CertificateGroup::DefaultApplication,
                TrustListDataType::default(),
            )]),
        });

        let (context2_base, _h2) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let context2 = RequestContext::new_test(Arc::new(RequestContextInner {
            session: context2_base.session.clone(),
            session_id: 2,
            authenticator: context2_base.authenticator.clone(),
            token: context2_base.token.clone(),
            user_roles: context2_base.user_roles.clone(),
            type_tree: context2_base.type_tree.clone(),
            type_tree_getter: context2_base.type_tree_getter.clone(),
            subscriptions: context2_base.subscriptions.clone(),
            info: context2_base.info.clone(),
        }));
        let cert_der = self_signed_cert_der(&context2);

        assert_eq!(
            handler.handle_update_certificate(&context2, &update_certificate_args(cert_der)),
            Err(StatusCode::BadTransactionPending)
        );
    }

    #[tokio::test]
    async fn get_rejected_list_requires_security_admin() {
        let (context, _handle) = request_context(
            MessageSecurityMode::Sign,
            vec![WellKnownRole::AuthenticatedUser.node_id()],
        );
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        assert_eq!(
            handler.handle_get_rejected_list(&context),
            Err(StatusCode::BadUserAccessDenied)
        );
    }

    #[tokio::test]
    async fn get_rejected_list_returns_rejected_certificates() {
        let (context, _handle) = security_admin_request_context(MessageSecurityMode::Sign);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        let rejected_cert = {
            let store = context.info.certificate_store.read();
            let cert = store.read_own_cert().expect("own cert should exist");
            store
                .store_rejected_cert(&cert)
                .expect("should be able to store rejected cert");
            cert
        };

        let outputs = handler
            .handle_get_rejected_list(&context)
            .expect("get rejected list should succeed");
        assert_eq!(outputs.len(), 1);
        let Variant::Array(certs) = &outputs[0] else {
            panic!("expected array output, got {:?}", outputs[0]);
        };
        assert_eq!(certs.values.len(), 1);
        assert_eq!(
            certs.values[0],
            Variant::from(rejected_cert.as_byte_string())
        );
    }

    #[tokio::test]
    async fn reset_to_server_defaults_without_handle_is_not_supported() {
        let (context, _handle) = security_admin_request_context(MessageSecurityMode::Sign);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        assert_eq!(
            handler.handle_reset_to_server_defaults(&context),
            Err(StatusCode::BadNotSupported)
        );
    }

    #[tokio::test]
    async fn reset_to_server_defaults_with_handle_schedules_shutdown() {
        let (context, handle) = security_admin_request_context(MessageSecurityMode::Sign);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), Some(handle));

        handler
            .handle_reset_to_server_defaults(&context)
            .expect("reset should succeed when a handle is available");
    }

    #[tokio::test]
    async fn methods_reject_missing_arguments() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let handler = GdsPushMethodHandler::new(Arc::new(GdsPushRegistry::default()), None);

        assert_eq!(
            handler.handle_create_signing_request(&context, &[]),
            Err(StatusCode::BadArgumentsMissing)
        );
        assert_eq!(
            handler.handle_update_certificate(&context, &[]),
            Err(StatusCode::BadArgumentsMissing)
        );
    }

    #[tokio::test]
    async fn push_method_ids_match_the_verified_standard_nodeset() {
        assert_eq!(server_configuration_object_id(), NodeId::new(0, 12637));
        assert_eq!(create_signing_request_method_id(), NodeId::new(0, 12737));
        assert_eq!(update_certificate_method_id(), NodeId::new(0, 13737));
        assert_eq!(apply_changes_method_id(), NodeId::new(0, 12740));
        assert_eq!(cancel_changes_method_id(), NodeId::new(0, 25708));
        assert_eq!(get_rejected_list_method_id(), NodeId::new(0, 12777));
        assert_eq!(reset_to_server_defaults_method_id(), NodeId::new(0, 25709));
    }
}
