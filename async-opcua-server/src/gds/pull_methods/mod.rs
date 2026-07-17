//! GDS pull model method callbacks (OPC UA Part 12 v1.05 §7.9, `CertificateDirectoryType`).
//!
//! Every method here is registered against a real `CertificateDirectoryType` instance,
//! constructed at runtime from the GDS companion NodeSet import (see `gds::directory_instance`
//! and `specs/103-gds-pull-fix/research.md`) -- unlike the core `ServerConfigurationType` (Push
//! model, `gds::push_methods`), this type has no pre-instantiated singleton anywhere, so it
//! must be built before any of these methods have anything real to dispatch against.
//!
//! This project's RBAC system (`crate::rbac::WellKnownRole`) only models the eight standard
//! Part 3 well-known roles. The GDS companion spec defines additional roles
//! (`CertificateAuthorityAdmin`, `ApplicationAdmin`, `ApplicationSelfAdmin`) that Part 12 §7.9
//! cites for these methods; as a pragmatic simplification (matching the Push model's precedent
//! of using `SecurityAdmin` uniformly), this module enforces `SecurityAdmin` instead of
//! resolving those GDS-specific roles dynamically from the companion namespace.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use opcua_crypto::{PrivateKey, X509};
use opcua_types::{Array, ByteString, MessageSecurityMode, NodeId, StatusCode, Variant};

#[cfg(all(feature = "generated-address-space", feature = "companion-gds"))]
use crate::node_manager::memory::CoreNodeManager;
use crate::{node_manager::RequestContext, rbac::WellKnownRole};

use super::directory_instance::DirectoryInstanceNodeIds;

/// Default RSA key size for `StartNewKeyPairRequest`-generated key pairs.
const DEFAULT_NEW_KEY_PAIR_BITS: u32 = 2048;
/// Default certificate validity, matching `X509Data`'s typical default elsewhere in this crate.
const DEFAULT_CERTIFICATE_DURATION_DAYS: u32 = 365;

/// `FinishRequest`'s `(Certificate, PrivateKey, IssuerCertificates)` output (Part 12 §7.9.5).
type CompletedRequestBundle = (Vec<u8>, Option<Vec<u8>>, Vec<Vec<u8>>);

/// A registered application. This feature's minimal, self-contained registration -- not the
/// full `RegisterApplication` Method (a separate, larger, already-tracked TODO item; see
/// research.md).
#[derive(Clone, Debug)]
struct GdsApplicationRecord {
    certificate_group_ids: Vec<NodeId>,
    application_uri: String,
}

/// State of a staged Pull-model certificate request (§7.9.3-§7.9.5). `Start*` resolves this
/// synchronously (auto-approve; there is no separate human-approval-queue product in this SDK
/// to wait on -- see research.md), but the `Pending` state is real and part of the protocol
/// contract, exercised directly in tests.
#[derive(Clone, Debug)]
enum PullRequestState {
    /// Not yet resolved. `FinishRequest` reports `Bad_NothingToDo` while in this state. Only
    /// ever constructed by the `#[cfg(test)]`-only `stage_pending_request` helper -- production
    /// code always resolves synchronously into `Completed` (see module docs) -- so this variant
    /// is unconstructed in non-test builds by design, not an oversight.
    #[allow(dead_code)]
    Pending,
    /// Resolved: the real certificate material to return from `FinishRequest`.
    Completed {
        certificate_der: Vec<u8>,
        private_key: Option<Vec<u8>>,
        issuer_certificates: Vec<Vec<u8>>,
    },
}

#[derive(Clone, Debug)]
struct GdsPullRequest {
    application_id: NodeId,
    state: PullRequestState,
}

struct GdsPullMethodRegistryInner {
    applications: moka::sync::Cache<NodeId, GdsApplicationRecord>,
    requests: moka::sync::Cache<NodeId, GdsPullRequest>,
    next_id: AtomicU64,
}

impl Default for GdsPullMethodRegistryInner {
    fn default() -> Self {
        Self {
            applications: moka::sync::Cache::builder()
                .max_capacity(super::GDS_REGISTRY_CAPACITY as u64)
                .build(),
            requests: moka::sync::Cache::builder()
                .max_capacity(super::GDS_REGISTRY_CAPACITY as u64)
                .build(),
            next_id: AtomicU64::new(0),
        }
    }
}

/// In-memory, capacity-bounded registry for GDS Pull-model certificate-management state (see
/// `GDS_REGISTRY_CAPACITY`: on overflow, the oldest entry is evicted before inserting the new
/// one, keeping sustained authorized traffic from growing registry memory without bound).
#[derive(Clone, Default)]
pub struct GdsPullMethodRegistry {
    inner: Arc<GdsPullMethodRegistryInner>,
}

impl GdsPullMethodRegistry {
    /// Registers a minimal application record for the Pull-model workflow and returns its
    /// newly-assigned `ApplicationId`. This is *not* the full `RegisterApplication` Method --
    /// see research.md.
    pub fn register_application(
        &self,
        application_uri: impl Into<String>,
        default_application_group_id: NodeId,
    ) -> NodeId {
        let ns = default_application_group_id.namespace;
        let application_id = self.next_node_id(ns, "Application");
        self.inner.applications.insert(
            application_id.clone(),
            GdsApplicationRecord {
                certificate_group_ids: vec![default_application_group_id],
                application_uri: application_uri.into(),
            },
        );
        application_id
    }

    fn next_node_id(&self, ns: u16, prefix: &str) -> NodeId {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        NodeId::new(ns, format!("{prefix}.{id}"))
    }

    fn application(&self, application_id: &NodeId) -> Option<GdsApplicationRecord> {
        self.inner.applications.get(application_id)
    }

    fn stage_completed_request(
        &self,
        ns: u16,
        application_id: NodeId,
        certificate_der: Vec<u8>,
        private_key: Option<Vec<u8>>,
    ) -> NodeId {
        let request_id = self.next_node_id(ns, "Request");
        self.inner.requests.insert(
            request_id.clone(),
            GdsPullRequest {
                application_id,
                state: PullRequestState::Completed {
                    certificate_der,
                    private_key,
                    issuer_certificates: Vec::new(),
                },
            },
        );
        request_id
    }

    /// Test-only: stages a request directly in the `Pending` state, since the normal `Start*`
    /// handlers resolve synchronously and never leave one pending (see module docs).
    #[cfg(test)]
    fn stage_pending_request(&self, ns: u16, application_id: NodeId) -> NodeId {
        let request_id = self.next_node_id(ns, "Request");
        self.inner.requests.insert(
            request_id.clone(),
            GdsPullRequest {
                application_id,
                state: PullRequestState::Pending,
            },
        );
        request_id
    }

    fn take_completed_request(
        &self,
        application_id: &NodeId,
        request_id: &NodeId,
    ) -> Result<CompletedRequestBundle, StatusCode> {
        let request = self
            .inner
            .requests
            .get(request_id)
            .ok_or(StatusCode::BadInvalidArgument)?;
        if &request.application_id != application_id {
            return Err(StatusCode::BadInvalidArgument);
        }
        match request.state {
            PullRequestState::Pending => Err(StatusCode::BadNothingToDo),
            PullRequestState::Completed {
                certificate_der,
                private_key,
                issuer_certificates,
            } => {
                self.inner.requests.invalidate(request_id);
                Ok((certificate_der, private_key, issuer_certificates))
            }
        }
    }
}

/// Handler for GDS Pull-model (`CertificateDirectoryType`) method calls.
pub struct GdsPullMethodHandler {
    registry: GdsPullMethodRegistry,
    directory: DirectoryInstanceNodeIds,
}

impl GdsPullMethodHandler {
    /// Creates a handler backed by `registry`, dispatching against the resolved `directory`
    /// instance NodeIds (`gds::directory_instance::instantiate_certificate_directory`).
    pub fn new(registry: GdsPullMethodRegistry, directory: DirectoryInstanceNodeIds) -> Self {
        Self {
            registry,
            directory,
        }
    }

    /// Returns the registry backing this handler (for registering test/deployment applications).
    pub fn registry(&self) -> &GdsPullMethodRegistry {
        &self.registry
    }

    /// Returns the resolved `CertificateDirectoryType` instance NodeIds this handler dispatches
    /// against.
    pub fn directory(&self) -> &DirectoryInstanceNodeIds {
        &self.directory
    }

    /// Handles `StartSigningRequest` (§7.9.3): signs the caller-supplied CSR's public key with
    /// this server's own key, acting as the Pull model's CertificateManager.
    pub fn handle_start_signing_request(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_encrypted_security_admin(context)?;

        if args.len() < 4 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let _certificate_group_id = node_id_arg(args, 1)?;
        let _certificate_type_id = node_id_arg(args, 2)?;
        let csr = non_empty_byte_string_arg(args, 3)?;

        let application = self
            .registry
            .application(&application_id)
            .ok_or(StatusCode::BadNotFound)?;

        let csr_der: Vec<u8> = csr
            .value
            .as_ref()
            .ok_or(StatusCode::BadInvalidArgument)?
            .to_vec();
        let public_key_der = X509::public_key_from_signing_request(&csr_der)
            .map_err(|_| StatusCode::BadInvalidArgument)?;

        let store = context.info.certificate_store.read();
        let issuer_cert = store
            .read_own_cert()
            .map_err(|_| StatusCode::BadInternalError)?;
        let issuer_pkey = store
            .read_own_pkey()
            .map_err(|_| StatusCode::BadInternalError)?;
        drop(store);

        let certificate_der = X509::issue_certificate_for_public_key(
            &public_key_der,
            &issuer_pkey,
            &issuer_cert,
            &format!("CN={}", application.application_uri),
            &application.application_uri,
            DEFAULT_CERTIFICATE_DURATION_DAYS,
        )
        .map_err(|_| StatusCode::BadInvalidArgument)?;

        let request_id = self.registry.stage_completed_request(
            application_id.namespace,
            application_id,
            certificate_der,
            None,
        );

        Ok(vec![Variant::from(request_id)])
    }

    /// Handles `StartNewKeyPairRequest` (§7.9.4): generates a fresh key pair and issues a
    /// certificate for it, signed by this server acting as the Pull model's CertificateManager.
    pub fn handle_start_new_key_pair_request(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_encrypted_security_admin(context)?;

        if args.len() < 7 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let _certificate_group_id = node_id_arg(args, 1)?;
        let _certificate_type_id = node_id_arg(args, 2)?;
        let subject_name = opt_string_arg(args, 3)?;
        let _domain_names = args.get(4);
        let private_key_format = opt_string_arg(args, 5)?.unwrap_or_default();
        let _private_key_password = opt_string_arg(args, 6)?;

        let application = self
            .registry
            .application(&application_id)
            .ok_or(StatusCode::BadNotFound)?;

        if !private_key_format.is_empty()
            && !private_key_format.eq_ignore_ascii_case("PEM")
            && !private_key_format.eq_ignore_ascii_case("PFX")
        {
            return Err(StatusCode::BadInvalidArgument);
        }
        if private_key_format.eq_ignore_ascii_case("PFX") {
            // PFX packaging is not implemented; PEM is always supported per §7.9.4.
            return Err(StatusCode::BadNotSupported);
        }

        let new_pkey =
            PrivateKey::new(DEFAULT_NEW_KEY_PAIR_BITS).map_err(|_| StatusCode::BadInternalError)?;
        let public_key_der = new_pkey
            .public_key_to_der()
            .map_err(|_| StatusCode::BadInternalError)?;

        let store = context.info.certificate_store.read();
        let issuer_cert = store
            .read_own_cert()
            .map_err(|_| StatusCode::BadInternalError)?;
        let issuer_pkey = store
            .read_own_pkey()
            .map_err(|_| StatusCode::BadInternalError)?;
        drop(store);

        let subject = subject_name.unwrap_or_else(|| format!("CN={}", application.application_uri));
        let certificate_der = X509::issue_certificate_for_public_key(
            &public_key_der,
            &issuer_pkey,
            &issuer_cert,
            &subject,
            &application.application_uri,
            DEFAULT_CERTIFICATE_DURATION_DAYS,
        )
        .map_err(|_| StatusCode::BadInvalidArgument)?;

        let private_key_pem = new_pkey
            .to_pem()
            .map_err(|_| StatusCode::BadInternalError)?
            .into_bytes();

        let request_id = self.registry.stage_completed_request(
            application_id.namespace,
            application_id,
            certificate_der,
            Some(private_key_pem),
        );

        Ok(vec![Variant::from(request_id)])
    }

    /// Handles `FinishRequest` (§7.9.5).
    pub fn handle_finish_request(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_encrypted_security_admin(context)?;

        if args.len() < 2 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let request_id = node_id_arg(args, 1)?;

        if self.registry.application(&application_id).is_none() {
            return Err(StatusCode::BadNotFound);
        }

        let (certificate_der, private_key, issuer_certificates) = self
            .registry
            .take_completed_request(&application_id, &request_id)?;

        let issuer_certificates: Vec<ByteString> = issuer_certificates
            .into_iter()
            .map(ByteString::from)
            .collect();
        let issuer_certificates_array = Array::new(
            opcua_types::VariantScalarTypeId::ByteString,
            issuer_certificates
                .into_iter()
                .map(Variant::from)
                .collect::<Vec<_>>(),
        )
        .map_err(|_| StatusCode::BadUnexpectedError)?;

        Ok(vec![
            Variant::from(ByteString::from(certificate_der)),
            Variant::from(ByteString::from(private_key.unwrap_or_default())),
            Variant::Array(Box::new(issuer_certificates_array)),
        ])
    }

    /// Handles `GetCertificateGroups` (§7.9.7).
    pub fn handle_get_certificate_groups(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.is_empty() {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let application = self
            .registry
            .application(&application_id)
            .ok_or(StatusCode::BadNotFound)?;

        let groups: Vec<Variant> = application
            .certificate_group_ids
            .into_iter()
            .map(Variant::from)
            .collect();
        let array = Array::new(opcua_types::VariantScalarTypeId::NodeId, groups)
            .map_err(|_| StatusCode::BadUnexpectedError)?;

        Ok(vec![Variant::Array(Box::new(array))])
    }

    /// Handles `GetTrustList` (§7.9.9).
    pub fn handle_get_trust_list(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.len() < 2 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let certificate_group_id = node_id_arg(args, 1)?;

        let application = self
            .registry
            .application(&application_id)
            .ok_or(StatusCode::BadNotFound)?;

        if !certificate_group_id.is_null()
            && !application
                .certificate_group_ids
                .contains(&certificate_group_id)
        {
            return Err(StatusCode::BadInvalidArgument);
        }

        Ok(vec![Variant::from(
            self.directory
                .default_application_group_trust_list_id
                .clone(),
        )])
    }

    /// Handles `GetCertificateStatus` (§7.9.10).
    pub fn handle_get_certificate_status(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.len() < 3 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let _certificate_group_id = node_id_arg(args, 1)?;
        let _certificate_type_id = node_id_arg(args, 2)?;

        if self.registry.application(&application_id).is_none() {
            return Err(StatusCode::BadNotFound);
        }

        // No certificate-expiry tracking is modeled for Pull-model-issued certificates in this
        // run; always report that no update is required. A real deployment would check the
        // issued certificate's validity window here.
        Ok(vec![Variant::from(false)])
    }
}

fn authorize_encrypted_security_admin(context: &RequestContext) -> Result<(), StatusCode> {
    if context.security_mode() != MessageSecurityMode::SignAndEncrypt {
        return Err(StatusCode::BadSecurityModeInsufficient);
    }
    require_security_admin(context)
}

fn authorize_authenticated_security_admin(context: &RequestContext) -> Result<(), StatusCode> {
    if context.security_mode() == MessageSecurityMode::None {
        return Err(StatusCode::BadSecurityModeInsufficient);
    }
    require_security_admin(context)
}

fn require_security_admin(context: &RequestContext) -> Result<(), StatusCode> {
    if !context
        .user_roles()
        .contains(&WellKnownRole::SecurityAdmin.node_id())
    {
        return Err(StatusCode::BadUserAccessDenied);
    }
    Ok(())
}

fn node_id_arg(args: &[Variant], index: usize) -> Result<NodeId, StatusCode> {
    match args.get(index) {
        Some(Variant::NodeId(node_id)) => Ok(node_id.as_ref().clone()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn byte_string_arg(args: &[Variant], index: usize) -> Result<ByteString, StatusCode> {
    match args.get(index) {
        Some(Variant::ByteString(value)) => Ok(value.clone()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn non_empty_byte_string_arg(args: &[Variant], index: usize) -> Result<ByteString, StatusCode> {
    let value = byte_string_arg(args, index)?;
    if value.is_null_or_empty() {
        Err(StatusCode::BadInvalidArgument)
    } else {
        Ok(value)
    }
}

fn opt_string_arg(args: &[Variant], index: usize) -> Result<Option<String>, StatusCode> {
    match args.get(index) {
        Some(Variant::String(value)) if value.is_null() => Ok(None),
        Some(Variant::String(value)) => Ok(Some(value.as_ref().to_owned())),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

/// Registers `handler`'s six Mandatory Pull-model method callbacks on `core_node_manager`,
/// against the real, instantiated `CertificateDirectoryType` NodeIds it holds. Called by
/// `gds::register_gds_pull_methods_from_companion` once the Directory instance exists.
#[cfg(all(feature = "generated-address-space", feature = "companion-gds"))]
pub(super) fn register_pull_method_callbacks(
    core_node_manager: &CoreNodeManager,
    handler: Arc<GdsPullMethodHandler>,
) {
    type Handle =
        fn(&GdsPullMethodHandler, &RequestContext, &[Variant]) -> Result<Vec<Variant>, StatusCode>;
    let bindings: [(NodeId, Handle); 6] = [
        (
            handler.directory.start_signing_request_id.clone(),
            GdsPullMethodHandler::handle_start_signing_request,
        ),
        (
            handler.directory.start_new_key_pair_request_id.clone(),
            GdsPullMethodHandler::handle_start_new_key_pair_request,
        ),
        (
            handler.directory.finish_request_id.clone(),
            GdsPullMethodHandler::handle_finish_request,
        ),
        (
            handler.directory.get_certificate_groups_id.clone(),
            GdsPullMethodHandler::handle_get_certificate_groups,
        ),
        (
            handler.directory.get_trust_list_id.clone(),
            GdsPullMethodHandler::handle_get_trust_list,
        ),
        (
            handler.directory.get_certificate_status_id.clone(),
            GdsPullMethodHandler::handle_get_certificate_status,
        ),
    ];

    for (method_id, invoke) in bindings {
        let h = handler.clone();
        core_node_manager
            .inner()
            .add_method_callback_with_context(method_id, move |ctx, _id, args| {
                invoke(&h, ctx, args)
            });
    }
}

#[cfg(test)]
mod tests;
