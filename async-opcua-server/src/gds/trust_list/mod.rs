//! TrustList method callbacks for the Push model's `DefaultApplicationGroup` CertificateGroup
//! (OPC UA Part 12 v1.05 §7.8.2 `TrustListType`), extending Run 1's shared `PushTransaction`
//! (see `gds::push_methods` and `specs/102-gds-push-trustlist/`).
//!
//! Every method here is registered against a real, verified `DefaultApplicationGroup.TrustList`
//! AddressSpace NodeId (confirmed via a live Read against a running server -- see
//! `specs/102-gds-push-trustlist/research.md`). `DefaultHttpsGroup`/`DefaultUserTokenGroup` are
//! out of scope for this run (see spec.md Assumptions).

use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use opcua_core::sync::Mutex;
use opcua_crypto::{CertificateStore, Thumbprint, X509};
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

const TRUST_LIST_OBJECT_ID: u32 = 12642;
const OPEN_METHOD_ID: u32 = 12647;
const CLOSE_METHOD_ID: u32 = 12650;
const READ_METHOD_ID: u32 = 12652;
const WRITE_METHOD_ID: u32 = 12655;
const GET_POSITION_METHOD_ID: u32 = 12657;
const SET_POSITION_METHOD_ID: u32 = 12660;
const OPEN_WITH_MASKS_METHOD_ID: u32 = 12663;
const CLOSE_AND_UPDATE_METHOD_ID: u32 = 12666;
const ADD_CERTIFICATE_METHOD_ID: u32 = 12668;
const REMOVE_CERTIFICATE_METHOD_ID: u32 = 12670;

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

/// Handler for TrustList method calls against `DefaultApplicationGroup.TrustList`.
pub struct TrustListMethodHandler {
    push_registry: Arc<GdsPushRegistry>,
    handles: TrustListHandleRegistry,
}

impl TrustListMethodHandler {
    /// Creates a handler sharing `push_registry`'s transaction with Run 1's certificate-rotation
    /// methods, so `ApplyChanges`/`CancelChanges` resolve both kinds of pending change together.
    pub fn new(push_registry: Arc<GdsPushRegistry>) -> Self {
        Self {
            push_registry,
            handles: TrustListHandleRegistry::new(Duration::from_secs(60)),
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
                let buffer = encode_trust_list(&build_trust_list_data(&store, masks::ALL))?;
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
        let buffer = encode_trust_list(&build_trust_list_data(&store, requested_masks))?;
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
                    updated.pending_trust_list = Some(trust_list);
                    *transaction = Some(updated);
                }
                None => {
                    *transaction = Some(PushTransaction {
                        owning_session_id: session_id,
                        certificate_der: None,
                        private_key_pem: None,
                        pending_trust_list: Some(trust_list),
                    });
                }
            }
        }
        self.handles.remove(file_handle);

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
            .store_trusted_cert(&cert)
            .map_err(|_| StatusCode::BadInternalError)?;

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

        let target = find_cert_by_thumbprint(&store, &thumbprint, is_trusted_certificate)
            .ok_or(StatusCode::BadInvalidArgument)?;

        if is_cert_still_needed(&store, &target) {
            return Err(StatusCode::BadCertificateChainIncomplete);
        }

        let removed = if is_trusted_certificate {
            store.remove_trusted_cert(&thumbprint)
        } else {
            store.remove_issuer_cert(&thumbprint)
        }
        .map_err(|_| StatusCode::BadInternalError)?;

        if !removed {
            return Err(StatusCode::BadInvalidArgument);
        }

        Ok(vec![])
    }
}

/// Commits a staged TrustList change (called from `push_methods::handle_apply_changes`).
/// Only lists whose `TrustListMasks` bit is set in `trust_list.specified_lists` are replaced.
pub(super) fn apply_trust_list_update(
    store: &CertificateStore,
    trust_list: &TrustListDataType,
) -> Result<(), String> {
    type ReplaceFn = fn(&CertificateStore, &[Vec<u8>]) -> Result<(), String>;
    let mask = trust_list.specified_lists;
    let lists: [(u32, &Option<Vec<ByteString>>, ReplaceFn); 4] = [
        (
            masks::TRUSTED_CERTIFICATES,
            &trust_list.trusted_certificates,
            CertificateStore::replace_trusted_certs,
        ),
        (
            masks::ISSUER_CERTIFICATES,
            &trust_list.issuer_certificates,
            CertificateStore::replace_issuer_certs,
        ),
        (
            masks::TRUSTED_CRLS,
            &trust_list.trusted_crls,
            CertificateStore::replace_trusted_crls,
        ),
        (
            masks::ISSUER_CRLS,
            &trust_list.issuer_crls,
            CertificateStore::replace_issuer_crls,
        ),
    ];
    for (bit, field, replace) in lists {
        if mask & bit != 0 {
            replace(store, &byte_strings_to_der(field))?;
        }
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

fn build_trust_list_data(store: &CertificateStore, mask: u32) -> TrustListDataType {
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
                .read_trusted_certs()
                .iter()
                .filter_map(|c| c.to_der().ok())
                .collect()
        },
    );
    set_der_list(&mut data.trusted_crls, mask, masks::TRUSTED_CRLS, || {
        store.read_trusted_crls_der()
    });
    set_der_list(
        &mut data.issuer_certificates,
        mask,
        masks::ISSUER_CERTIFICATES,
        || {
            store
                .read_issuer_certs()
                .iter()
                .filter_map(|c| c.to_der().ok())
                .collect()
        },
    );
    set_der_list(&mut data.issuer_crls, mask, masks::ISSUER_CRLS, || {
        store.read_issuer_crls_der()
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
    thumbprint: &Thumbprint,
    is_trusted_certificate: bool,
) -> Option<X509> {
    let certs = if is_trusted_certificate {
        store.read_trusted_certs()
    } else {
        store.read_issuer_certs()
    };
    certs.into_iter().find(|c| c.thumbprint() == *thumbprint)
}

/// Whether `target` (a candidate for removal) is a CA still needed to validate some *other*
/// certificate currently in the trusted or issuer store (Part 12 §7.8.2.7).
fn is_cert_still_needed(store: &CertificateStore, target: &X509) -> bool {
    let target_thumbprint = target.thumbprint();
    let subject_name = target.subject_name();
    store
        .read_trusted_certs()
        .iter()
        .chain(store.read_issuer_certs().iter())
        .any(|other| other.thumbprint() != target_thumbprint && other.issuer_name() == subject_name)
}

/// Returns the standard `DefaultApplicationGroup.TrustList` object id.
pub fn trust_list_object_id() -> NodeId {
    NodeId::new(0, TRUST_LIST_OBJECT_ID)
}

/// Returns the standard `Open` method id.
pub fn open_method_id() -> NodeId {
    NodeId::new(0, OPEN_METHOD_ID)
}

/// Returns the standard `Close` method id.
pub fn close_method_id() -> NodeId {
    NodeId::new(0, CLOSE_METHOD_ID)
}

/// Returns the standard `Read` method id.
pub fn read_method_id() -> NodeId {
    NodeId::new(0, READ_METHOD_ID)
}

/// Returns the standard `Write` method id.
pub fn write_method_id() -> NodeId {
    NodeId::new(0, WRITE_METHOD_ID)
}

/// Returns the standard `GetPosition` method id.
pub fn get_position_method_id() -> NodeId {
    NodeId::new(0, GET_POSITION_METHOD_ID)
}

/// Returns the standard `SetPosition` method id.
pub fn set_position_method_id() -> NodeId {
    NodeId::new(0, SET_POSITION_METHOD_ID)
}

/// Returns the standard `OpenWithMasks` method id.
pub fn open_with_masks_method_id() -> NodeId {
    NodeId::new(0, OPEN_WITH_MASKS_METHOD_ID)
}

/// Returns the standard `CloseAndUpdate` method id.
pub fn close_and_update_method_id() -> NodeId {
    NodeId::new(0, CLOSE_AND_UPDATE_METHOD_ID)
}

/// Returns the standard `AddCertificate` method id.
pub fn add_certificate_method_id() -> NodeId {
    NodeId::new(0, ADD_CERTIFICATE_METHOD_ID)
}

/// Returns the standard `RemoveCertificate` method id.
pub fn remove_certificate_method_id() -> NodeId {
    NodeId::new(0, REMOVE_CERTIFICATE_METHOD_ID)
}

/// Registers the TrustList method callbacks on the core (namespace 0) node manager, sharing
/// `push_registry`'s transaction with Run 1's certificate-rotation methods.
#[cfg(all(feature = "method-call", feature = "generated-address-space"))]
pub fn register_trust_list_methods(
    node_manager: &CoreNodeManager,
    push_registry: Arc<GdsPushRegistry>,
) -> Arc<TrustListMethodHandler> {
    let handler = Arc::new(TrustListMethodHandler::new(push_registry));

    type Handle = fn(
        &TrustListMethodHandler,
        &RequestContext,
        &[Variant],
    ) -> Result<Vec<Variant>, StatusCode>;
    let bindings: [(NodeId, Handle); 10] = [
        (open_method_id(), TrustListMethodHandler::handle_open),
        (
            open_with_masks_method_id(),
            TrustListMethodHandler::handle_open_with_masks,
        ),
        (read_method_id(), TrustListMethodHandler::handle_read),
        (write_method_id(), TrustListMethodHandler::handle_write),
        (
            get_position_method_id(),
            TrustListMethodHandler::handle_get_position,
        ),
        (
            set_position_method_id(),
            TrustListMethodHandler::handle_set_position,
        ),
        (close_method_id(), TrustListMethodHandler::handle_close),
        (
            close_and_update_method_id(),
            TrustListMethodHandler::handle_close_and_update,
        ),
        (
            add_certificate_method_id(),
            TrustListMethodHandler::handle_add_certificate,
        ),
        (
            remove_certificate_method_id(),
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

    handler
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

#[cfg(test)]
mod tests;
