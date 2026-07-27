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
use opcua_types::{
    ApplicationDescription, ApplicationType, Array, ByteString, DateTime, LocalizedText,
    MessageSecurityMode, NodeId, ServerOnNetwork, StatusCode, UAString, Variant,
};

#[cfg(all(feature = "generated-address-space", feature = "companion-gds"))]
use crate::node_manager::memory::CoreNodeManager;
use crate::{gds::like_match::like_match, node_manager::RequestContext, rbac::WellKnownRole};
#[cfg(feature = "companion-gds")]
use opcua_types::ApplicationRecordDataType;

use super::directory_instance::DirectoryInstanceNodeIds;

/// `ApplicationType` bitmask values for `QueryApplications`' own `ApplicationType` filter
/// argument (Part 12 §6.5.10 -- distinct from the `ApplicationType` enum itself).
const QUERY_APPLICATION_TYPE_SERVERS: u32 = 0x1;
const QUERY_APPLICATION_TYPE_CLIENTS: u32 = 0x2;
/// A `ServerCapabilities` value of `"NA"` means the application doesn't support OPC UA at all;
/// Part 12 §6.5.10/§6.5.11 both say such records are never returned by either query method.
const CAPABILITY_NA: &str = "NA";

/// Default RSA key size for `StartNewKeyPairRequest`-generated key pairs.
const DEFAULT_NEW_KEY_PAIR_BITS: u32 = 2048;
/// Default certificate validity, matching `X509Data`'s typical default elsewhere in this crate.
const DEFAULT_CERTIFICATE_DURATION_DAYS: u32 = 365;

/// `FinishRequest`'s `(Certificate, PrivateKey, IssuerCertificates)` output (Part 12 §7.9.5).
type CompletedRequestBundle = (Vec<u8>, Option<Vec<u8>>, Vec<Vec<u8>>, NodeId, NodeId);

/// A registered application. Originally a minimal, self-contained stand-in for the Pull-model's
/// own internal certificate-issuance workflow; feature 108 extends it with the real
/// `ApplicationRecordDataType` fields (Part 12 §6.5.5) so the same registry backs both that
/// internal use and the real `RegisterApplication`/`QueryApplications`/etc. methods -- per
/// `specs/108-gds-directory-app-registry/research.md` R5, there is no basis in the spec for two
/// disjoint registries (an application registered via the Pull model's own certificate flow
/// becoming visible to `QueryApplications` is spec-correct, not a merge of unrelated concerns).
#[derive(Clone, Debug, Default)]
struct GdsApplicationRecord {
    /// Monotonically increasing identifier assigned at create/update time (Part 12 §6.5.10 --
    /// "Each time the GDS creates or updates an application record it shall assign a
    /// monotonically increasing identifier"). Backs `QueryApplications`/`QueryServers`'
    /// `StartingRecordId`/`NextRecordId` pagination.
    record_id: u64,
    certificate_group_ids: Vec<NodeId>,
    application_uri: String,
    application_type: ApplicationType,
    application_names: Vec<LocalizedText>,
    product_uri: String,
    discovery_urls: Vec<String>,
    server_capabilities: Vec<String>,
}

impl GdsApplicationRecord {
    /// The default `ApplicationName` per Part 12 Table 13 -- "the first element" when there's no
    /// session to locale-match against (this handler layer doesn't thread session locale
    /// preferences through, so this always uses the first-element fallback).
    fn default_application_name(&self) -> LocalizedText {
        self.application_names.first().cloned().unwrap_or_default()
    }

    fn to_application_description(&self) -> ApplicationDescription {
        ApplicationDescription {
            application_uri: UAString::from(self.application_uri.as_str()),
            product_uri: UAString::from(self.product_uri.as_str()),
            application_name: self.default_application_name(),
            application_type: self.application_type,
            gateway_server_uri: UAString::null(),
            discovery_profile_uri: UAString::null(),
            discovery_urls: uastring_array(&self.discovery_urls),
        }
    }

    /// One `ServerOnNetwork` row per discovery URL, per Part 12 Table 15.
    fn to_server_on_network_rows(&self) -> Vec<ServerOnNetwork> {
        let server_name = UAString::from(
            self.default_application_name()
                .text
                .value()
                .clone()
                .unwrap_or_default(),
        );
        self.discovery_urls
            .iter()
            .map(|url| ServerOnNetwork {
                record_id: self.record_id as u32,
                server_name: server_name.clone(),
                discovery_url: UAString::from(url.as_str()),
                server_capabilities: uastring_array(&self.server_capabilities),
            })
            .collect()
    }

    #[cfg(feature = "companion-gds")]
    fn to_wire_type(&self, application_id: NodeId) -> ApplicationRecordDataType {
        ApplicationRecordDataType {
            application_id,
            application_uri: UAString::from(self.application_uri.as_str()),
            application_type: self.application_type,
            application_names: (!self.application_names.is_empty())
                .then(|| self.application_names.clone()),
            product_uri: UAString::from(self.product_uri.as_str()),
            discovery_urls: uastring_array(&self.discovery_urls),
            server_capabilities: uastring_array(&self.server_capabilities),
        }
    }

    fn matches_type_mask(&self, mask: u32) -> bool {
        if mask == 0 {
            return true;
        }
        let is_server = matches!(
            self.application_type,
            ApplicationType::Server
                | ApplicationType::ClientAndServer
                | ApplicationType::DiscoveryServer
        );
        let is_client = matches!(
            self.application_type,
            ApplicationType::Client | ApplicationType::ClientAndServer
        );
        (mask & QUERY_APPLICATION_TYPE_SERVERS != 0 && is_server)
            || (mask & QUERY_APPLICATION_TYPE_CLIENTS != 0 && is_client)
    }
}

fn uastring_array(values: &[String]) -> Option<Vec<UAString>> {
    (!values.is_empty()).then(|| values.iter().map(|v| UAString::from(v.as_str())).collect())
}

#[cfg(feature = "companion-gds")]
fn strings_from(values: Option<Vec<UAString>>) -> Vec<String> {
    values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.value().clone())
        .collect()
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
    certificate_group_id: NodeId,
    certificate_type_id: NodeId,
    state: PullRequestState,
}

struct GdsPullMethodRegistryInner {
    applications: moka::sync::Cache<NodeId, GdsApplicationRecord>,
    requests: moka::sync::Cache<NodeId, GdsPullRequest>,
    next_id: AtomicU64,
    /// Reported verbatim as `QueryApplications`/`QueryServers`' `LastCounterResetTime` (Part 12
    /// §6.5.10/§6.5.11). This registry is in-memory and never persisted, so there is nothing to
    /// "reset" mid-run -- a fresh, later value only ever appears after a full server restart,
    /// which is exactly the signal a real client needs to detect it must restart its own
    /// pagination from `StartingRecordId=0` (see `specs/108-gds-directory-app-registry/
    /// research.md` R9).
    created_at: DateTime,
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
            // Starts at 1, not 0: `QueryApplications`/`QueryServers` both define `StartingRecordId
            // = 0` to mean "start with the first record in the database" (Part 12 §6.5.10/
            // §6.5.11), which only works if no real record is ever assigned identifier 0.
            next_id: AtomicU64::new(1),
            created_at: DateTime::now(),
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
                record_id: self.next_record_id(),
                certificate_group_ids: vec![default_application_group_id],
                application_uri: application_uri.into(),
                ..Default::default()
            },
        );
        application_id
    }

    /// The real `RegisterApplication` Method (Part 12 §6.5.6): rejects a duplicate
    /// `ApplicationUri` with `Bad_EntryExists` (FR-002), otherwise assigns a fresh `ApplicationId`
    /// and `record_id` and stores the full record. `default_application_group_id` is seeded onto
    /// the new record the same way the Pull-model's own internal `register_application` does, so
    /// an application registered through this Method can immediately use
    /// `GetCertificateGroups`/`GetTrustList` against the real `DefaultApplicationGroup`.
    #[cfg(feature = "companion-gds")]
    fn register_full(
        &self,
        ns: u16,
        record: ApplicationRecordDataType,
        default_application_group_id: NodeId,
    ) -> Result<NodeId, StatusCode> {
        if record.application_uri.is_empty() {
            return Err(StatusCode::BadInvalidArgument);
        }
        let application_uri = record.application_uri.value().clone().unwrap_or_default();
        if self.find_by_uri(&application_uri).is_some() {
            return Err(StatusCode::BadEntryExists);
        }

        let application_id = self.next_node_id(ns, "Application");
        self.inner.applications.insert(
            application_id.clone(),
            GdsApplicationRecord {
                record_id: self.next_record_id(),
                certificate_group_ids: vec![default_application_group_id],
                application_uri,
                application_type: record.application_type,
                application_names: record.application_names.unwrap_or_default(),
                product_uri: record.product_uri.value().clone().unwrap_or_default(),
                discovery_urls: strings_from(record.discovery_urls),
                server_capabilities: strings_from(record.server_capabilities),
            },
        );
        Ok(application_id)
    }

    /// The real `UpdateApplication` Method (Part 12 §6.5.7): `Bad_NotFound` if `ApplicationId`
    /// is unknown, `Bad_WriteNotSupported` if `ApplicationUri` was changed.
    #[cfg(feature = "companion-gds")]
    fn update_full(&self, record: ApplicationRecordDataType) -> Result<(), StatusCode> {
        let existing = self
            .application(&record.application_id)
            .ok_or(StatusCode::BadNotFound)?;
        let new_uri = record.application_uri.value().clone().unwrap_or_default();
        if new_uri != existing.application_uri {
            return Err(StatusCode::BadWriteNotSupported);
        }

        self.inner.applications.insert(
            record.application_id.clone(),
            GdsApplicationRecord {
                record_id: self.next_record_id(),
                certificate_group_ids: existing.certificate_group_ids,
                application_uri: new_uri,
                application_type: record.application_type,
                application_names: record.application_names.unwrap_or_default(),
                product_uri: record.product_uri.value().clone().unwrap_or_default(),
                discovery_urls: strings_from(record.discovery_urls),
                server_capabilities: strings_from(record.server_capabilities),
            },
        );
        Ok(())
    }

    /// The real `UnregisterApplication` Method (Part 12 §6.5.8): `Bad_NotFound` if unknown.
    /// Deliberately does not revoke any certificates issued to the application -- see
    /// `specs/108-gds-directory-app-registry/research.md` R6 and `TODO.md` (same gap already
    /// tracked for this SDK's `RevokeCertificate`, CU 3582).
    fn unregister(&self, application_id: &NodeId) -> Result<(), StatusCode> {
        if self.application(application_id).is_none() {
            return Err(StatusCode::BadNotFound);
        }
        self.inner.applications.invalidate(application_id);
        Ok(())
    }

    /// The real `FindApplications` Method (Part 12 §6.5.4): exact (not LIKE-pattern) match on
    /// `ApplicationUri`, distinct from `QueryApplications`' own LIKE-based URI filter.
    #[cfg(feature = "companion-gds")]
    fn find_by_uri(&self, application_uri: &str) -> Option<(NodeId, GdsApplicationRecord)> {
        self.inner
            .applications
            .iter()
            .find(|(_, record)| record.application_uri == application_uri)
            .map(|(k, v)| (k.as_ref().clone(), v))
    }

    /// Filters and sorts registry entries by `record_id` ascending, per Part 12 §6.5.10/§6.5.11's
    /// shared AND-combined filter semantics (research.md R2) -- shared by `QueryApplications` and
    /// `QueryServers`, which page over the exact same underlying registry/counter, not two
    /// separately-maintained data sources.
    #[allow(clippy::too_many_arguments)]
    fn filtered_records(
        &self,
        application_name_pattern: &str,
        application_uri_pattern: &str,
        product_uri_pattern: &str,
        application_type_mask: u32,
        capabilities: &[String],
    ) -> Vec<(NodeId, GdsApplicationRecord)> {
        let mut records: Vec<(NodeId, GdsApplicationRecord)> = self
            .inner
            .applications
            .iter()
            .map(|(k, v)| (k.as_ref().clone(), v))
            .collect();
        records.retain(|(_, record)| {
            if record
                .server_capabilities
                .iter()
                .any(|c| c == CAPABILITY_NA)
            {
                return false;
            }
            if !record.matches_type_mask(application_type_mask) {
                return false;
            }
            if !application_name_pattern.is_empty() {
                let name = record
                    .default_application_name()
                    .text
                    .value()
                    .clone()
                    .unwrap_or_default();
                if !like_match(application_name_pattern, &name) {
                    return false;
                }
            }
            if !application_uri_pattern.is_empty()
                && !like_match(application_uri_pattern, &record.application_uri)
            {
                return false;
            }
            if !product_uri_pattern.is_empty()
                && !like_match(product_uri_pattern, &record.product_uri)
            {
                return false;
            }
            if !capabilities.is_empty()
                && !capabilities
                    .iter()
                    .all(|c| record.server_capabilities.contains(c))
            {
                return false;
            }
            true
        });
        records.sort_by_key(|(_, record)| record.record_id);
        records
    }

    fn registry_created_at(&self) -> DateTime {
        self.inner.created_at
    }

    fn next_record_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
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
        certificate_group_id: NodeId,
        certificate_type_id: NodeId,
        certificate_der: Vec<u8>,
        private_key: Option<Vec<u8>>,
    ) -> NodeId {
        let request_id = self.next_node_id(ns, "Request");
        self.inner.requests.insert(
            request_id.clone(),
            GdsPullRequest {
                application_id,
                certificate_group_id,
                certificate_type_id,
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
                certificate_group_id: NodeId::null(),
                certificate_type_id: NodeId::null(),
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
                Ok((
                    certificate_der,
                    private_key,
                    issuer_certificates,
                    request.certificate_group_id,
                    request.certificate_type_id,
                ))
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
        let certificate_group_id = node_id_arg(args, 1)?;
        let certificate_type_id = node_id_arg(args, 2)?;
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
            certificate_group_id.clone(),
            certificate_type_id.clone(),
            certificate_der,
            None,
        );

        #[cfg(feature = "events")]
        super::audit::certificate_requested(
            context,
            self.directory.directory_object_id.clone(),
            self.directory.start_signing_request_id.clone(),
            certificate_group_id,
            certificate_type_id,
            "StartSigningRequest",
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
        let certificate_group_id = node_id_arg(args, 1)?;
        let certificate_type_id = node_id_arg(args, 2)?;
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
            certificate_group_id.clone(),
            certificate_type_id.clone(),
            certificate_der,
            Some(private_key_pem),
        );

        #[cfg(feature = "events")]
        super::audit::certificate_requested(
            context,
            self.directory.directory_object_id.clone(),
            self.directory.start_new_key_pair_request_id.clone(),
            certificate_group_id,
            certificate_type_id,
            "StartNewKeyPairRequest",
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

        let (
            certificate_der,
            private_key,
            issuer_certificates,
            certificate_group_id,
            certificate_type_id,
        ) = self
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

        let outputs = vec![
            Variant::from(ByteString::from(certificate_der)),
            Variant::from(ByteString::from(private_key.unwrap_or_default())),
            Variant::Array(Box::new(issuer_certificates_array)),
        ];

        #[cfg(feature = "events")]
        super::audit::certificate_delivered(
            context,
            self.directory.directory_object_id.clone(),
            self.directory.finish_request_id.clone(),
            certificate_group_id,
            certificate_type_id,
        );

        Ok(outputs)
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

    /// Handles `RegisterApplication` (Part 12 §6.5.6). Requires `SecurityAdmin` (research.md R4).
    #[cfg(feature = "companion-gds")]
    pub fn handle_register_application(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.is_empty() {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let record = application_record_arg(args, 0)?;
        let ns = self.directory.directory_object_id.namespace;
        let application_id = self.registry.register_full(
            ns,
            record,
            self.directory.default_application_group_id.clone(),
        )?;
        #[cfg(feature = "events")]
        super::audit::application_registration_changed(
            context,
            self.directory.directory_object_id.clone(),
            self.directory.register_application_id.clone(),
            "RegisterApplication",
        );
        Ok(vec![Variant::from(application_id)])
    }

    /// Handles `UpdateApplication` (Part 12 §6.5.7). Requires `SecurityAdmin` (research.md R4).
    #[cfg(feature = "companion-gds")]
    pub fn handle_update_application(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.is_empty() {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let record = application_record_arg(args, 0)?;
        self.registry.update_full(record)?;
        #[cfg(feature = "events")]
        super::audit::application_registration_changed(
            context,
            self.directory.directory_object_id.clone(),
            self.directory.update_application_id.clone(),
            "UpdateApplication",
        );
        Ok(vec![])
    }

    /// Handles `UnregisterApplication` (Part 12 §6.5.8). Requires `SecurityAdmin` (research.md
    /// R4). Deliberately does not revoke certificates issued to the application -- see
    /// `GdsPullMethodRegistry::unregister`'s own doc comment and research.md R6.
    pub fn handle_unregister_application(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if args.is_empty() {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        self.registry.unregister(&application_id)?;
        #[cfg(feature = "events")]
        super::audit::application_registration_changed(
            context,
            self.directory.directory_object_id.clone(),
            self.directory.unregister_application_id.clone(),
            "UnregisterApplication",
        );
        Ok(vec![])
    }

    /// Handles `GetApplication` (Part 12 §6.5.9). No role restriction (research.md R2/R4 -- the
    /// spec text names no specific Role for this read, unlike the three write methods above).
    #[cfg(feature = "companion-gds")]
    pub fn handle_get_application(
        &self,
        _context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        if args.is_empty() {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_id = node_id_arg(args, 0)?;
        let record = self
            .registry
            .application(&application_id)
            .ok_or(StatusCode::BadNotFound)?;
        Ok(vec![Variant::from(opcua_types::ExtensionObject::new(
            record.to_wire_type(application_id),
        ))])
    }

    /// Handles `FindApplications` (Part 12 §6.5.4). No role restriction ("can be called by any
    /// Client"). Exact match on `ApplicationUri`, NOT the LIKE-pattern matching
    /// `QueryApplications`/`QueryServers` use for the same field.
    #[cfg(feature = "companion-gds")]
    pub fn handle_find_applications(
        &self,
        _context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        if args.is_empty() {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let application_uri = string_arg(args, 0)?;
        if application_uri.is_empty() {
            return Err(StatusCode::BadInvalidArgument);
        }

        let found = self.registry.find_by_uri(&application_uri);
        let applications: Vec<Variant> = found
            .into_iter()
            .map(|(application_id, record)| {
                Variant::from(opcua_types::ExtensionObject::new(
                    record.to_wire_type(application_id),
                ))
            })
            .collect();
        let array = Array::new(
            opcua_types::VariantScalarTypeId::ExtensionObject,
            applications,
        )
        .map_err(|_| StatusCode::BadUnexpectedError)?;
        Ok(vec![Variant::Array(Box::new(array))])
    }

    /// Handles `QueryApplications` (Part 12 §6.5.10). No role restriction ("Any Client is able to
    /// call this Method").
    pub fn handle_query_applications(
        &self,
        _context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        if args.len() < 7 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let starting_record_id = u32_arg(args, 0)? as u64;
        let max_records_to_return = u32_arg(args, 1)?;
        let application_name = string_arg(args, 2)?;
        let application_uri = string_arg(args, 3)?;
        let application_type_mask = u32_arg(args, 4)?;
        let product_uri = string_arg(args, 5)?;
        let capabilities = string_array_arg(args, 6)?;

        let mut records = self.registry.filtered_records(
            &application_name,
            &application_uri,
            &product_uri,
            application_type_mask,
            &capabilities,
        );
        records.retain(|(_, record)| record.record_id > starting_record_id);

        let limit = if max_records_to_return == 0 {
            records.len()
        } else {
            max_records_to_return as usize
        };
        let has_more = records.len() > limit;
        records.truncate(limit);
        // `NextRecordId` must be the LAST record actually returned in this batch, not the next
        // unreturned one -- `StartingRecordId`'s own filter is a strict `>`, so passing the next
        // unreturned record's own id back as `StartingRecordId` would skip that record entirely.
        let next_record_id = if has_more {
            records.last().map(|(_, r)| r.record_id).unwrap_or(0)
        } else {
            0
        };

        let descriptions: Vec<Variant> = records
            .iter()
            .map(|(_, record)| {
                Variant::from(opcua_types::ExtensionObject::new(
                    record.to_application_description(),
                ))
            })
            .collect();
        let array = Array::new(
            opcua_types::VariantScalarTypeId::ExtensionObject,
            descriptions,
        )
        .map_err(|_| StatusCode::BadUnexpectedError)?;

        Ok(vec![
            Variant::from(self.registry.registry_created_at()),
            Variant::from(next_record_id as u32),
            Variant::Array(Box::new(array)),
        ])
    }

    /// Handles the deprecated `QueryServers` (Part 12 §6.5.11). No permission required. Shares
    /// `QueryApplications`' underlying filtered/paginated record set (research.md R2's own
    /// "monotonically increasing identifier" text is shared between the two methods, not two
    /// independent counters), projecting one `ServerOnNetwork` row per discovery URL instead of
    /// one `ApplicationDescription` per application (Part 12 Table 15).
    pub fn handle_query_servers(
        &self,
        _context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        if args.len() < 6 {
            return Err(StatusCode::BadArgumentsMissing);
        }
        let starting_record_id = u32_arg(args, 0)? as u64;
        let max_records_to_return = u32_arg(args, 1)?;
        let application_name = string_arg(args, 2)?;
        let application_uri = string_arg(args, 3)?;
        let product_uri = string_arg(args, 4)?;
        let server_capabilities = string_array_arg(args, 5)?;

        let mut records = self.registry.filtered_records(
            &application_name,
            &application_uri,
            &product_uri,
            QUERY_APPLICATION_TYPE_SERVERS,
            &server_capabilities,
        );
        records.retain(|(_, record)| record.record_id > starting_record_id);

        // Expand to `ServerOnNetwork` rows (one per discovery URL, Part 12 Table 15) BEFORE
        // truncating: `MaxRecordsToReturn` bounds the number of *rows* in the response, not the
        // number of underlying application records, so truncating records first could still
        // return more rows than requested whenever a record has multiple discovery URLs.
        let mut rows: Vec<Variant> = records
            .iter()
            .flat_map(|(_, record)| record.to_server_on_network_rows())
            .map(Variant::from)
            .collect();
        let limit = if max_records_to_return == 0 {
            rows.len()
        } else {
            max_records_to_return as usize
        };
        rows.truncate(limit);

        let array = Array::new(opcua_types::VariantScalarTypeId::ExtensionObject, rows)
            .map_err(|_| StatusCode::BadUnexpectedError)?;

        Ok(vec![
            Variant::from(self.registry.registry_created_at()),
            Variant::Array(Box::new(array)),
        ])
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

fn string_arg(args: &[Variant], index: usize) -> Result<String, StatusCode> {
    match args.get(index) {
        Some(Variant::String(value)) => Ok(value.value().clone().unwrap_or_default()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn u32_arg(args: &[Variant], index: usize) -> Result<u32, StatusCode> {
    match args.get(index) {
        Some(Variant::UInt32(value)) => Ok(*value),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn string_array_arg(args: &[Variant], index: usize) -> Result<Vec<String>, StatusCode> {
    match args.get(index) {
        Some(Variant::Array(array)) => array
            .values
            .iter()
            .map(|v| match v {
                // A null String *entry* (distinct from a non-String element) legitimately means
                // "no capability at this position" and is dropped, not rejected.
                Variant::String(s) => Ok(s.value().clone()),
                _ => Err(StatusCode::BadTypeMismatch),
            })
            .collect::<Result<Vec<Option<String>>, StatusCode>>()
            .map(|values| values.into_iter().flatten().collect()),
        Some(Variant::String(s)) if s.is_null() => Ok(Vec::new()),
        Some(Variant::Empty) => Ok(Vec::new()),
        None => Err(StatusCode::BadArgumentsMissing),
        Some(_) => Err(StatusCode::BadTypeMismatch),
    }
}

/// Decodes an `ApplicationRecordDataType` argument. Requires the caller (both server and, for
/// the round-trip test, client) to have registered `application_record::
/// GdsApplicationRecordTypeLoader` -- see that module's docs.
#[cfg(feature = "companion-gds")]
fn application_record_arg(
    args: &[Variant],
    index: usize,
) -> Result<ApplicationRecordDataType, StatusCode> {
    match args.get(index) {
        Some(Variant::ExtensionObject(obj)) => obj
            .clone()
            .into_inner_as::<ApplicationRecordDataType>()
            .map(|boxed| *boxed)
            .ok_or(StatusCode::BadTypeMismatch),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

/// Registers `handler`'s Mandatory Pull-model method callbacks (certificate management, §7.9,
/// plus the feature-108 Directory application-registry methods, §6.5.4/§6.5.6-§6.5.11) on
/// `core_node_manager`, against the real, instantiated `CertificateDirectoryType` NodeIds it
/// holds. Called by `gds::register_gds_pull_methods_from_companion` once the Directory instance
/// exists.
#[cfg(all(feature = "generated-address-space", feature = "companion-gds"))]
pub(super) fn register_pull_method_callbacks(
    core_node_manager: &CoreNodeManager,
    handler: Arc<GdsPullMethodHandler>,
) {
    type Handle =
        fn(&GdsPullMethodHandler, &RequestContext, &[Variant]) -> Result<Vec<Variant>, StatusCode>;
    let mut bindings: Vec<(NodeId, Handle)> = vec![
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
        (
            handler.directory.unregister_application_id.clone(),
            GdsPullMethodHandler::handle_unregister_application,
        ),
        (
            handler.directory.query_applications_id.clone(),
            GdsPullMethodHandler::handle_query_applications,
        ),
        (
            handler.directory.query_servers_id.clone(),
            GdsPullMethodHandler::handle_query_servers,
        ),
    ];

    // `ApplicationRecordDataType` (the wire type these four methods traffic in) only exists
    // under `companion-gds` -- see `application_record.rs`'s module doc and research.md R8's
    // note that this type has no generated binding, requiring a hand-authored `DynEncodable`
    // impl that Cargo's cross-crate feature unification can't safely gate any other way.
    #[cfg(feature = "companion-gds")]
    bindings.extend([
        (
            handler.directory.register_application_id.clone(),
            GdsPullMethodHandler::handle_register_application as Handle,
        ),
        (
            handler.directory.update_application_id.clone(),
            GdsPullMethodHandler::handle_update_application as Handle,
        ),
        (
            handler.directory.get_application_id.clone(),
            GdsPullMethodHandler::handle_get_application as Handle,
        ),
        (
            handler.directory.find_applications_id.clone(),
            GdsPullMethodHandler::handle_find_applications as Handle,
        ),
    ]);

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
