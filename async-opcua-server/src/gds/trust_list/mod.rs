//! TrustList method callbacks for the Push model's three standard CertificateGroups (OPC UA Part
//! 12 v1.05 §7.8.2 `TrustListType`), extending Run 1's shared `PushTransaction` (see
//! `gds::push_methods` and `specs/102-gds-push-trustlist/`).

use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use opcua_core::sync::Mutex;
use opcua_crypto::{CertificateGroup, CertificateStore, Thumbprint, X509};
use opcua_types::{
    BinaryDecodable, BinaryEncodable, ByteString, ContextOwned, NodeId, StatusCode,
    TrustListDataType, Variant,
};

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
use crate::node_manager::memory::CoreNodeManager;
use crate::node_manager::RequestContext;

use super::push_methods::{
    authorize_authenticated_security_admin, non_empty_byte_string_arg, GdsPushRegistry,
    PushTransaction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrustListNodeIds {
    object: u32,
    open: u32,
    close: u32,
    read: u32,
    write: u32,
    get_position: u32,
    set_position: u32,
    open_with_masks: u32,
    close_and_update: u32,
    add_certificate: u32,
    remove_certificate: u32,
}

impl TrustListNodeIds {
    const fn for_group(certificate_group: CertificateGroup) -> Self {
        match certificate_group {
            CertificateGroup::DefaultApplication => Self {
                object: 12642,
                open: 12647,
                close: 12650,
                read: 12652,
                write: 12655,
                get_position: 12657,
                set_position: 12660,
                open_with_masks: 12663,
                close_and_update: 12666,
                add_certificate: 12668,
                remove_certificate: 12670,
            },
            CertificateGroup::DefaultHttps => Self {
                object: 14089,
                open: 14095,
                close: 14098,
                read: 14100,
                write: 14103,
                get_position: 14105,
                set_position: 14108,
                open_with_masks: 14111,
                close_and_update: 14114,
                add_certificate: 14117,
                remove_certificate: 14119,
            },
            CertificateGroup::DefaultUserToken => Self {
                object: 14123,
                open: 14129,
                close: 14132,
                read: 14134,
                write: 14137,
                get_position: 14139,
                set_position: 14142,
                open_with_masks: 14145,
                close_and_update: 14148,
                add_certificate: 14151,
                remove_certificate: 14153,
            },
        }
    }
}

/// `TrustListMasks` bit values (Part 12 §7.8.2.9).
mod masks {
    pub(super) const TRUSTED_CERTIFICATES: u32 = 1;
    pub(super) const TRUSTED_CRLS: u32 = 2;
    pub(super) const ISSUER_CERTIFICATES: u32 = 4;
    pub(super) const ISSUER_CRLS: u32 = 8;
    pub(super) const ALL: u32 = 15;
}

/// `OpenFileMode` bit values (Part 5, re-declared here for the two combinations TrustList
/// supports -- see [`opcua_types::OpenFileMode`]).
mod open_mode {
    pub(super) const READ: u8 = 1;
    pub(super) const WRITE: u8 = 2;
    pub(super) const ERASE_EXISTING: u8 = 4;
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum HandleMode {
    Read,
    Write,
}

struct TrustListFileHandleState {
    owning_session_id: u32,
    mode: HandleMode,
    buffer: Vec<u8>,
    position: u64,
}

/// Session-scoped, idle-timeout-bounded registry of open TrustList file handles (Part 12
/// §7.8.2.1 `ActivityTimeout`). Modeled on
/// [`crate::history::continuation::HistoryContinuationPointCache`], but uses `time_to_idle`
/// (rather than `time_to_live`) since `ActivityTimeout` is defined as the maximum elapsed time
/// *between calls*, not since the handle was created.
struct TrustListHandleRegistry {
    handles: moka::sync::Cache<u32, Arc<Mutex<TrustListFileHandleState>>>,
    next_handle: AtomicU32,
}

impl TrustListHandleRegistry {
    fn new(activity_timeout: Duration) -> Self {
        Self {
            handles: moka::sync::Cache::builder()
                .time_to_idle(activity_timeout)
                .build(),
            next_handle: AtomicU32::new(1),
        }
    }

    fn insert(&self, state: TrustListFileHandleState) -> u32 {
        loop {
            let handle = self.next_handle.fetch_add(1, Ordering::Relaxed).max(1);
            if !self.handles.contains_key(&handle) {
                self.handles.insert(handle, Arc::new(Mutex::new(state)));
                return handle;
            }
        }
    }

    fn get(
        &self,
        handle: u32,
        session_id: u32,
    ) -> Result<Arc<Mutex<TrustListFileHandleState>>, StatusCode> {
        let entry = self
            .handles
            .get(&handle)
            .ok_or(StatusCode::BadInvalidState)?;
        if entry.lock().owning_session_id != session_id {
            return Err(StatusCode::BadInvalidState);
        }
        Ok(entry)
    }

    fn remove(&self, handle: u32) {
        self.handles.invalidate(&handle);
    }
}

/// Handler for TrustList method calls against one standard CertificateGroup.
pub struct TrustListMethodHandler {
    push_registry: Arc<GdsPushRegistry>,
    handles: TrustListHandleRegistry,
    certificate_group: CertificateGroup,
    #[cfg_attr(not(feature = "companion-gds"), allow(dead_code))]
    node_ids: TrustListNodeIds,
}

impl TrustListMethodHandler {
    /// Creates a handler sharing `push_registry`'s transaction with Run 1's certificate-rotation
    /// methods, so `ApplyChanges`/`CancelChanges` resolve both kinds of pending change together.
    pub fn new(push_registry: Arc<GdsPushRegistry>) -> Self {
        Self::new_for_group(push_registry, CertificateGroup::DefaultApplication)
    }

    /// Creates a handler for `certificate_group`, sharing the global push transaction registry.
    pub fn new_for_group(
        push_registry: Arc<GdsPushRegistry>,
        certificate_group: CertificateGroup,
    ) -> Self {
        Self {
            push_registry,
            handles: TrustListHandleRegistry::new(Duration::from_secs(60)),
            certificate_group,
            node_ids: TrustListNodeIds::for_group(certificate_group),
        }
    }

    /// Handles `Open` (Part 5, restricted per Part 12 §7.8.2.2 to Read and Write+EraseExisting).
    pub fn handle_open(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let mode = byte_arg(args, 0)?;
        let session_id = context.session_id();

        let handle = match mode {
            open_mode::READ => {
                let store = context.info.certificate_store.read();
                let buffer = encode_trust_list(&build_trust_list_data(
                    &store,
                    self.certificate_group,
                    masks::ALL,
                ))?;
                self.handles.insert(TrustListFileHandleState {
                    owning_session_id: session_id,
                    mode: HandleMode::Read,
                    buffer,
                    position: 0,
                })
            }
            m if m == (open_mode::WRITE | open_mode::ERASE_EXISTING) => {
                if self
                    .push_registry
                    .transaction_pending_for_other_session(session_id)
                {
                    return Err(StatusCode::BadTransactionPending);
                }
                self.handles.insert(TrustListFileHandleState {
                    owning_session_id: session_id,
                    mode: HandleMode::Write,
                    buffer: Vec::new(),
                    position: 0,
                })
            }
            _ => return Err(StatusCode::BadNotSupported),
        };

        Ok(vec![Variant::from(handle)])
    }

    /// Handles `OpenWithMasks` (Part 12 §7.8.2.3): read-only, filtered to the requested subset.
    pub fn handle_open_with_masks(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let requested_masks = u32_arg(args, 0)?;
        let store = context.info.certificate_store.read();
        let buffer = encode_trust_list(&build_trust_list_data(
            &store,
            self.certificate_group,
            requested_masks,
        ))?;
        drop(store);

        let handle = self.handles.insert(TrustListFileHandleState {
            owning_session_id: context.session_id(),
            mode: HandleMode::Read,
            buffer,
            position: 0,
        });

        Ok(vec![Variant::from(handle)])
    }

    /// Handles `Read` (Part 5): chunked read from the handle's buffer at its current position.
    pub fn handle_read(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let file_handle = u32_arg(args, 0)?;
        let length = i32_arg(args, 1)?;
        let entry = self.handles.get(file_handle, context.session_id())?;
        let mut state = entry.lock();

        let start = state.position as usize;
        let data = if length <= 0 || start >= state.buffer.len() {
            Vec::new()
        } else {
            let end = (start + length as usize).min(state.buffer.len());
            state.buffer[start..end].to_vec()
        };
        state.position += data.len() as u64;

        Ok(vec![Variant::from(ByteString::from(data))])
    }

    /// Handles `Write` (Part 5): writes into the handle's buffer at its current position.
    pub fn handle_write(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let file_handle = u32_arg(args, 0)?;
        let data = non_empty_byte_string_arg(args, 1)?;
        let bytes: Vec<u8> = data.value.as_ref().map(|v| v.to_vec()).unwrap_or_default();

        let entry = self.handles.get(file_handle, context.session_id())?;
        let mut state = entry.lock();
        if state.mode != HandleMode::Write {
            return Err(StatusCode::BadNotWritable);
        }

        let start = state.position as usize;
        if start + bytes.len() > state.buffer.len() {
            state.buffer.resize(start + bytes.len(), 0);
        }
        state.buffer[start..start + bytes.len()].copy_from_slice(&bytes);
        state.position += bytes.len() as u64;

        Ok(vec![])
    }

    /// Handles `GetPosition` (Part 5).
    pub fn handle_get_position(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let file_handle = u32_arg(args, 0)?;
        let entry = self.handles.get(file_handle, context.session_id())?;
        let position = entry.lock().position;

        Ok(vec![Variant::from(position)])
    }

    /// Handles `SetPosition` (Part 5).
    pub fn handle_set_position(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let file_handle = u32_arg(args, 0)?;
        let position = u64_arg(args, 1)?;
        let entry = self.handles.get(file_handle, context.session_id())?;
        entry.lock().position = position;

        Ok(vec![])
    }

    /// Handles `Close` (Part 5): discards the handle. For a write-mode handle that never had
    /// `CloseAndUpdate` called, this discards the pending buffer with no side effects.
    pub fn handle_close(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let file_handle = u32_arg(args, 0)?;
        // Validate the handle belongs to this session before removing it.
        let _ = self.handles.get(file_handle, context.session_id())?;
        self.handles.remove(file_handle);

        Ok(vec![])
    }

    /// Handles `CloseAndUpdate` (Part 12 §7.8.2.5): validates and stages the written TrustList
    /// content as a pending change on the shared transaction, returning
    /// `ApplyChangesRequired = true`.
    pub fn handle_close_and_update(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        let file_handle = u32_arg(args, 0)?;
        let session_id = context.session_id();
        let entry = self.handles.get(file_handle, session_id)?;
        let buffer = {
            let state = entry.lock();
            if state.mode != HandleMode::Write {
                return Err(StatusCode::BadNotWritable);
            }
            state.buffer.clone()
        };

        let trust_list = decode_trust_list(&buffer).map_err(|_| {
            self.handles.remove(file_handle);
            StatusCode::BadInvalidArgument
        })?;

        if let Err(status) = validate_trust_list_certificates(&trust_list) {
            self.handles.remove(file_handle);
            return Err(status);
        }

        {
            let mut transaction = self.push_registry.transaction.write();
            match &*transaction {
                Some(existing) if existing.owning_session_id != session_id => {
                    self.handles.remove(file_handle);
                    return Err(StatusCode::BadTransactionPending);
                }
                Some(existing) => {
                    let mut updated = existing.clone();
                    updated
                        .pending_trust_lists
                        .insert(self.certificate_group, trust_list);
                    *transaction = Some(updated);
                }
                None => {
                    let pending_trust_lists =
                        std::collections::HashMap::from([(self.certificate_group, trust_list)]);
                    *transaction = Some(PushTransaction {
                        owning_session_id: session_id,
                        certificate_der: None,
                        private_key_pem: None,
                        certificate_group_id: None,
                        certificate_type_id: None,
                        pending_trust_lists,
                    });
                }
            }
        }
        self.handles.remove(file_handle);

        #[cfg(feature = "events")]
        #[cfg(feature = "companion-gds")]
        super::audit::trust_list_update_requested(
            context,
            NodeId::new(0, self.node_ids.object),
            NodeId::new(0, self.node_ids.close_and_update),
            NodeId::new(0, self.node_ids.object),
            "CloseAndUpdate",
        );

        Ok(vec![Variant::from(true)])
    }

    /// Handles `AddCertificate` (Part 12 §7.8.2.6): immediate, non-transactional single
    /// certificate add.
    pub fn handle_add_certificate(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if self
            .push_registry
            .transaction_pending_for_other_session(context.session_id())
        {
            return Err(StatusCode::BadTransactionPending);
        }

        let certificate = non_empty_byte_string_arg(args, 0)?;
        let is_trusted_certificate = bool_arg(args, 1)?;
        if !is_trusted_certificate {
            // AddCertificate cannot add issuer certificates per Part 12 §7.8.2.6.
            return Err(StatusCode::BadCertificateInvalid);
        }

        let der: Vec<u8> = certificate
            .value
            .as_ref()
            .ok_or(StatusCode::BadInvalidArgument)?
            .to_vec();
        let cert = X509::from_der(&der).map_err(|_| StatusCode::BadCertificateInvalid)?;
        validate_certificate_time(&cert)?;

        let store = context.info.certificate_store.read();
        store
            .store_trusted_cert_for_group(self.certificate_group, &cert)
            .map_err(|_| StatusCode::BadInternalError)?;

        #[cfg(feature = "events")]
        #[cfg(feature = "companion-gds")]
        super::audit::trust_list_updated(
            context,
            NodeId::new(0, self.node_ids.object),
            NodeId::new(0, self.node_ids.add_certificate),
            NodeId::new(0, self.node_ids.object),
            "AddCertificate",
        );

        Ok(vec![])
    }

    /// Handles `RemoveCertificate` (Part 12 §7.8.2.7): immediate, non-transactional single
    /// certificate removal by thumbprint.
    pub fn handle_remove_certificate(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        authorize_authenticated_security_admin(context)?;

        if self
            .push_registry
            .transaction_pending_for_other_session(context.session_id())
        {
            return Err(StatusCode::BadTransactionPending);
        }

        let thumbprint_str = string_arg(args, 0)?;
        let is_trusted_certificate = bool_arg(args, 1)?;
        let thumbprint =
            Thumbprint::parse_hex(&thumbprint_str).map_err(|_| StatusCode::BadInvalidArgument)?;

        let store = context.info.certificate_store.read();

        let target = find_cert_by_thumbprint(
            &store,
            self.certificate_group,
            &thumbprint,
            is_trusted_certificate,
        )
        .ok_or(StatusCode::BadInvalidArgument)?;

        if is_cert_still_needed(&store, self.certificate_group, &target) {
            return Err(StatusCode::BadCertificateChainIncomplete);
        }

        let removed = if is_trusted_certificate {
            store.remove_trusted_cert_for_group(self.certificate_group, &thumbprint)
        } else {
            store.remove_issuer_cert_for_group(self.certificate_group, &thumbprint)
        }
        .map_err(|_| StatusCode::BadInternalError)?;

        if !removed {
            return Err(StatusCode::BadInvalidArgument);
        }

        #[cfg(feature = "events")]
        #[cfg(feature = "companion-gds")]
        super::audit::trust_list_updated(
            context,
            NodeId::new(0, self.node_ids.object),
            NodeId::new(0, self.node_ids.remove_certificate),
            NodeId::new(0, self.node_ids.object),
            "RemoveCertificate",
        );

        Ok(vec![])
    }
}

/// Commits a staged TrustList change (called from `push_methods::handle_apply_changes`).
/// Only lists whose `TrustListMasks` bit is set in `trust_list.specified_lists` are replaced.
pub(super) fn apply_trust_list_update(
    store: &CertificateStore,
    certificate_group: CertificateGroup,
    trust_list: &TrustListDataType,
) -> Result<(), String> {
    let mask = trust_list.specified_lists;
    if mask & masks::TRUSTED_CERTIFICATES != 0 {
        store.replace_trusted_certs_for_group(
            certificate_group,
            &byte_strings_to_der(&trust_list.trusted_certificates),
        )?;
    }
    if mask & masks::ISSUER_CERTIFICATES != 0 {
        store.replace_issuer_certs_for_group(
            certificate_group,
            &byte_strings_to_der(&trust_list.issuer_certificates),
        )?;
    }
    if mask & masks::TRUSTED_CRLS != 0 {
        store.replace_trusted_crls_for_group(
            certificate_group,
            &byte_strings_to_der(&trust_list.trusted_crls),
        )?;
    }
    if mask & masks::ISSUER_CRLS != 0 {
        store.replace_issuer_crls_for_group(
            certificate_group,
            &byte_strings_to_der(&trust_list.issuer_crls),
        )?;
    }
    Ok(())
}

fn byte_strings_to_der(list: &Option<Vec<ByteString>>) -> Vec<Vec<u8>> {
    list.as_ref()
        .map(|items| {
            items
                .iter()
                .filter_map(|b| b.value.as_ref().map(|v| v.to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

/// Populates `*target` with `ders()`'s output (as `ByteString`s) if `bit` is set in `mask`;
/// `ders` is a thunk so an unset bit skips the (potentially file-reading) collection entirely.
fn set_der_list(
    target: &mut Option<Vec<ByteString>>,
    mask: u32,
    bit: u32,
    ders: impl FnOnce() -> Vec<Vec<u8>>,
) {
    if mask & bit != 0 {
        *target = Some(ders().into_iter().map(ByteString::from).collect());
    }
}

fn build_trust_list_data(
    store: &CertificateStore,
    certificate_group: CertificateGroup,
    mask: u32,
) -> TrustListDataType {
    let mut data = TrustListDataType {
        specified_lists: mask,
        ..Default::default()
    };
    set_der_list(
        &mut data.trusted_certificates,
        mask,
        masks::TRUSTED_CERTIFICATES,
        || {
            store
                .read_trusted_certs_for_group(certificate_group)
                .iter()
                .filter_map(|c| c.to_der().ok())
                .collect()
        },
    );
    set_der_list(&mut data.trusted_crls, mask, masks::TRUSTED_CRLS, || {
        store.read_trusted_crls_der_for_group(certificate_group)
    });
    set_der_list(
        &mut data.issuer_certificates,
        mask,
        masks::ISSUER_CERTIFICATES,
        || {
            store
                .read_issuer_certs_for_group(certificate_group)
                .iter()
                .filter_map(|c| c.to_der().ok())
                .collect()
        },
    );
    set_der_list(&mut data.issuer_crls, mask, masks::ISSUER_CRLS, || {
        store.read_issuer_crls_der_for_group(certificate_group)
    });
    data
}

fn encode_trust_list(data: &TrustListDataType) -> Result<Vec<u8>, StatusCode> {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut buffer = Vec::new();
    data.encode(&mut buffer, &ctx)
        .map_err(|_| StatusCode::BadInternalError)?;
    Ok(buffer)
}

fn decode_trust_list(buffer: &[u8]) -> Result<TrustListDataType, StatusCode> {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    let mut reader = buffer;
    TrustListDataType::decode(&mut reader, &ctx).map_err(|_| StatusCode::BadInvalidArgument)
}

/// Validates every certificate in the proposed new `TrustedCertificates` list (Part 12 §7.8.2.5:
/// "the Server shall verify that every Certificate in the new TrustList is valid using the
/// validation process defined in OPC 10000-4"). Scoped to structural (parses as X.509) and
/// temporal (not expired/not-yet-valid) validity -- a full chain-validation simulation against
/// the *proposed* new list (rather than the currently-persisted store) is out of scope for this
/// run; see research.md.
fn validate_trust_list_certificates(trust_list: &TrustListDataType) -> Result<(), StatusCode> {
    let Some(certs) = &trust_list.trusted_certificates else {
        return Ok(());
    };
    for der in certs {
        let Some(bytes) = der.value.as_ref() else {
            continue;
        };
        let cert = X509::from_der(bytes).map_err(|_| StatusCode::BadCertificateInvalid)?;
        validate_certificate_time(&cert)?;
    }
    Ok(())
}

fn validate_certificate_time(cert: &X509) -> Result<(), StatusCode> {
    cert.is_time_valid(&chrono::Utc::now())
        .map_err(|_| StatusCode::BadCertificateInvalid)
}

fn find_cert_by_thumbprint(
    store: &CertificateStore,
    certificate_group: CertificateGroup,
    thumbprint: &Thumbprint,
    is_trusted_certificate: bool,
) -> Option<X509> {
    let certs = if is_trusted_certificate {
        store.read_trusted_certs_for_group(certificate_group)
    } else {
        store.read_issuer_certs_for_group(certificate_group)
    };
    certs.into_iter().find(|c| c.thumbprint() == *thumbprint)
}

/// Whether `target` (a candidate for removal) is a CA still needed to validate some *other*
/// certificate currently in the trusted or issuer store (Part 12 §7.8.2.7).
fn is_cert_still_needed(
    store: &CertificateStore,
    certificate_group: CertificateGroup,
    target: &X509,
) -> bool {
    let target_thumbprint = target.thumbprint();
    let subject_name = target.subject_name();
    store
        .read_trusted_certs_for_group(certificate_group)
        .iter()
        .chain(store.read_issuer_certs_for_group(certificate_group).iter())
        .any(|other| other.thumbprint() != target_thumbprint && other.issuer_name() == subject_name)
}

/// Returns the standard `DefaultApplicationGroup.TrustList` object id.
pub fn trust_list_object_id() -> NodeId {
    trust_list_object_id_for_group(CertificateGroup::DefaultApplication)
}

pub(super) fn trust_list_object_id_for_group(certificate_group: CertificateGroup) -> NodeId {
    NodeId::new(0, TrustListNodeIds::for_group(certificate_group).object)
}

/// Returns the standard `Open` method id.
pub fn open_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).open,
    )
}

/// Returns the standard `Close` method id.
pub fn close_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).close,
    )
}

/// Returns the standard `Read` method id.
pub fn read_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).read,
    )
}

/// Returns the standard `Write` method id.
pub fn write_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).write,
    )
}

/// Returns the standard `GetPosition` method id.
pub fn get_position_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).get_position,
    )
}

/// Returns the standard `SetPosition` method id.
pub fn set_position_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).set_position,
    )
}

/// Returns the standard `OpenWithMasks` method id.
pub fn open_with_masks_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).open_with_masks,
    )
}

/// Returns the standard `CloseAndUpdate` method id.
pub fn close_and_update_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).close_and_update,
    )
}

/// Returns the standard `AddCertificate` method id.
pub fn add_certificate_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).add_certificate,
    )
}

/// Returns the standard `RemoveCertificate` method id.
pub fn remove_certificate_method_id() -> NodeId {
    NodeId::new(
        0,
        TrustListNodeIds::for_group(CertificateGroup::DefaultApplication).remove_certificate,
    )
}

/// Registers the TrustList method callbacks on the core (namespace 0) node manager, sharing
/// `push_registry`'s transaction with Run 1's certificate-rotation methods.
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_trust_list_methods(
    node_manager: &CoreNodeManager,
    push_registry: Arc<GdsPushRegistry>,
) -> Arc<TrustListMethodHandler> {
    let application_handler = Arc::new(TrustListMethodHandler::new(push_registry.clone()));
    register_trust_list_group_methods(node_manager, application_handler.clone());
    register_trust_list_group_methods(
        node_manager,
        Arc::new(TrustListMethodHandler::new_for_group(
            push_registry.clone(),
            CertificateGroup::DefaultHttps,
        )),
    );
    register_trust_list_group_methods(
        node_manager,
        Arc::new(TrustListMethodHandler::new_for_group(
            push_registry,
            CertificateGroup::DefaultUserToken,
        )),
    );

    application_handler
}

#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
fn register_trust_list_group_methods(
    node_manager: &CoreNodeManager,
    handler: Arc<TrustListMethodHandler>,
) {
    type Handle = fn(
        &TrustListMethodHandler,
        &RequestContext,
        &[Variant],
    ) -> Result<Vec<Variant>, StatusCode>;
    let node_ids = handler.node_ids;
    let bindings: [(NodeId, Handle); 10] = [
        (
            NodeId::new(0, node_ids.open),
            TrustListMethodHandler::handle_open,
        ),
        (
            NodeId::new(0, node_ids.open_with_masks),
            TrustListMethodHandler::handle_open_with_masks,
        ),
        (
            NodeId::new(0, node_ids.read),
            TrustListMethodHandler::handle_read,
        ),
        (
            NodeId::new(0, node_ids.write),
            TrustListMethodHandler::handle_write,
        ),
        (
            NodeId::new(0, node_ids.get_position),
            TrustListMethodHandler::handle_get_position,
        ),
        (
            NodeId::new(0, node_ids.set_position),
            TrustListMethodHandler::handle_set_position,
        ),
        (
            NodeId::new(0, node_ids.close),
            TrustListMethodHandler::handle_close,
        ),
        (
            NodeId::new(0, node_ids.close_and_update),
            TrustListMethodHandler::handle_close_and_update,
        ),
        (
            NodeId::new(0, node_ids.add_certificate),
            TrustListMethodHandler::handle_add_certificate,
        ),
        (
            NodeId::new(0, node_ids.remove_certificate),
            TrustListMethodHandler::handle_remove_certificate,
        ),
    ];

    for (method_id, invoke) in bindings {
        let h = handler.clone();
        node_manager
            .inner()
            .add_method_callback_with_context(method_id, move |ctx, _id, args| {
                invoke(&h, ctx, args)
            });
    }
}

fn byte_arg(args: &[Variant], index: usize) -> Result<u8, StatusCode> {
    match args.get(index) {
        Some(Variant::Byte(value)) => Ok(*value),
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

fn u64_arg(args: &[Variant], index: usize) -> Result<u64, StatusCode> {
    match args.get(index) {
        Some(Variant::UInt64(value)) => Ok(*value),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn i32_arg(args: &[Variant], index: usize) -> Result<i32, StatusCode> {
    match args.get(index) {
        Some(Variant::Int32(value)) => Ok(*value),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn bool_arg(args: &[Variant], index: usize) -> Result<bool, StatusCode> {
    match args.get(index) {
        Some(Variant::Boolean(value)) => Ok(*value),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn string_arg(args: &[Variant], index: usize) -> Result<String, StatusCode> {
    match args.get(index) {
        Some(Variant::String(value)) if !value.is_null() => Ok(value.as_ref().to_owned()),
        Some(_) => Err(StatusCode::BadInvalidArgument),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

#[cfg(all(test, feature = "events", feature = "companion-gds"))]
mod audit_tests;
#[cfg(test)]
mod tests;
