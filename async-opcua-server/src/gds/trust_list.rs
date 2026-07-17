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

        if is_cert_still_needed(&store, &target, &thumbprint) {
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
    let specified = trust_list.specified_lists;
    if specified & masks::TRUSTED_CERTIFICATES != 0 {
        store.replace_trusted_certs(&byte_strings_to_der(&trust_list.trusted_certificates))?;
    }
    if specified & masks::ISSUER_CERTIFICATES != 0 {
        store.replace_issuer_certs(&byte_strings_to_der(&trust_list.issuer_certificates))?;
    }
    if specified & masks::TRUSTED_CRLS != 0 {
        store.replace_trusted_crls(&byte_strings_to_der(&trust_list.trusted_crls))?;
    }
    if specified & masks::ISSUER_CRLS != 0 {
        store.replace_issuer_crls(&byte_strings_to_der(&trust_list.issuer_crls))?;
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

fn build_trust_list_data(store: &CertificateStore, masks: u32) -> TrustListDataType {
    let mut data = TrustListDataType {
        specified_lists: masks,
        ..Default::default()
    };
    if masks & masks::TRUSTED_CERTIFICATES != 0 {
        data.trusted_certificates = Some(
            store
                .read_trusted_certs()
                .iter()
                .filter_map(|c| c.to_der().ok())
                .map(ByteString::from)
                .collect(),
        );
    }
    if masks & masks::TRUSTED_CRLS != 0 {
        data.trusted_crls = Some(
            store
                .read_trusted_crls_der()
                .into_iter()
                .map(ByteString::from)
                .collect(),
        );
    }
    if masks & masks::ISSUER_CERTIFICATES != 0 {
        data.issuer_certificates = Some(
            store
                .read_issuer_certs()
                .iter()
                .filter_map(|c| c.to_der().ok())
                .map(ByteString::from)
                .collect(),
        );
    }
    if masks & masks::ISSUER_CRLS != 0 {
        data.issuer_crls = Some(
            store
                .read_issuer_crls_der()
                .into_iter()
                .map(ByteString::from)
                .collect(),
        );
    }
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
fn is_cert_still_needed(
    store: &CertificateStore,
    target: &X509,
    target_thumbprint: &Thumbprint,
) -> bool {
    let subject_name = target.subject_name();
    store
        .read_trusted_certs()
        .iter()
        .chain(store.read_issuer_certs().iter())
        .any(|other| {
            other.thumbprint() != *target_thumbprint && other.issuer_name() == subject_name
        })
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

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(open_method_id(), move |ctx, _id, args| {
            h.handle_open(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(open_with_masks_method_id(), move |ctx, _id, args| {
            h.handle_open_with_masks(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(read_method_id(), move |ctx, _id, args| {
            h.handle_read(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(write_method_id(), move |ctx, _id, args| {
            h.handle_write(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(get_position_method_id(), move |ctx, _id, args| {
            h.handle_get_position(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(set_position_method_id(), move |ctx, _id, args| {
            h.handle_set_position(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(close_method_id(), move |ctx, _id, args| {
            h.handle_close(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(close_and_update_method_id(), move |ctx, _id, args| {
            h.handle_close_and_update(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(add_certificate_method_id(), move |ctx, _id, args| {
            h.handle_add_certificate(ctx, args)
        });

    let h = handler.clone();
    node_manager
        .inner()
        .add_method_callback_with_context(remove_certificate_method_id(), move |ctx, _id, args| {
            h.handle_remove_certificate(ctx, args)
        });

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
mod tests {
    use opcua_core::sync::RwLock;
    use opcua_crypto::{SecurityPolicy, X509Data};
    use opcua_types::{
        AnonymousIdentityToken, ApplicationDescription, MessageSecurityMode, UAString,
    };

    use crate::{
        authenticator::UserToken, identity_token::IdentityToken, node_manager::RequestContextInner,
        rbac::WellKnownRole, session::instance::Session, ServerBuilder,
    };

    use super::*;

    #[test]
    fn method_ids_match_the_verified_standard_nodeset() {
        assert_eq!(trust_list_object_id(), NodeId::new(0, 12642));
        assert_eq!(open_method_id(), NodeId::new(0, 12647));
        assert_eq!(close_method_id(), NodeId::new(0, 12650));
        assert_eq!(read_method_id(), NodeId::new(0, 12652));
        assert_eq!(write_method_id(), NodeId::new(0, 12655));
        assert_eq!(get_position_method_id(), NodeId::new(0, 12657));
        assert_eq!(set_position_method_id(), NodeId::new(0, 12660));
        assert_eq!(open_with_masks_method_id(), NodeId::new(0, 12663));
        assert_eq!(close_and_update_method_id(), NodeId::new(0, 12666));
        assert_eq!(add_certificate_method_id(), NodeId::new(0, 12668));
        assert_eq!(remove_certificate_method_id(), NodeId::new(0, 12670));
    }

    fn unique_test_pki_dir() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        tempfile::Builder::new()
            .prefix(&format!(
                "async-opcua-gds-trust-list-test-pki-{}-{id}-",
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
        let (_server, handle) = ServerBuilder::new_anonymous("trust list method test")
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
            UAString::from("trust-list-method-test"),
            ApplicationDescription::default(),
            security_mode,
        )));

        let context = RequestContext::new_test(Arc::new(RequestContextInner {
            session,
            session_id: 1,
            authenticator: info.authenticator.clone(),
            token: UserToken("trust-list-method-test".to_string()),
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

    fn self_signed_cert_with_cn(cn: &str) -> X509 {
        let data = X509Data {
            key_size: 2048,
            common_name: cn.to_owned(),
            organization: "async-opcua tests".to_owned(),
            organizational_unit: String::new(),
            country: "IE".to_owned(),
            state: String::new(),
            alt_host_names: opcua_crypto::AlternateNames::new(),
            certificate_duration_days: 365,
        };
        let (cert, _pkey) = X509::cert_and_pkey(&data).expect("cert generation should succeed");
        cert
    }

    fn handler(push_registry: Arc<GdsPushRegistry>) -> TrustListMethodHandler {
        TrustListMethodHandler::new(push_registry)
    }

    #[tokio::test]
    async fn open_read_then_read_returns_the_actual_trust_list() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let cert = self_signed_cert_with_cn("trusted-fixture");
        context
            .info
            .certificate_store
            .read()
            .store_trusted_cert(&cert)
            .expect("fixture cert should store");
        let h = handler(Arc::new(GdsPushRegistry::default()));

        let open_out = h
            .handle_open(&context, &[Variant::from(open_mode::READ)])
            .expect("open should succeed");
        let Variant::UInt32(file_handle) = open_out[0] else {
            panic!("expected UInt32 file handle");
        };

        let read_out = h
            .handle_read(
                &context,
                &[Variant::from(file_handle), Variant::from(i32::MAX)],
            )
            .expect("read should succeed");
        let Variant::ByteString(data) = &read_out[0] else {
            panic!("expected ByteString data");
        };
        let bytes = data.value.as_ref().expect("data should not be null");
        let decoded = decode_trust_list(bytes).expect("should decode as TrustListDataType");
        let trusted = decoded
            .trusted_certificates
            .expect("trusted_certificates should be present");
        assert_eq!(trusted.len(), 1);

        h.handle_close(&context, &[Variant::from(file_handle)])
            .expect("close should succeed");
    }

    #[tokio::test]
    async fn open_with_masks_returns_only_the_requested_subset() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let cert = self_signed_cert_with_cn("trusted-fixture");
        context
            .info
            .certificate_store
            .read()
            .store_trusted_cert(&cert)
            .expect("fixture cert should store");
        let h = handler(Arc::new(GdsPushRegistry::default()));

        let open_out = h
            .handle_open_with_masks(&context, &[Variant::from(masks::TRUSTED_CERTIFICATES)])
            .expect("open with masks should succeed");
        let Variant::UInt32(file_handle) = open_out[0] else {
            panic!("expected UInt32 file handle");
        };
        let read_out = h
            .handle_read(
                &context,
                &[Variant::from(file_handle), Variant::from(i32::MAX)],
            )
            .expect("read should succeed");
        let Variant::ByteString(data) = &read_out[0] else {
            panic!("expected ByteString data");
        };
        let bytes = data.value.as_ref().expect("data should not be null");
        let decoded = decode_trust_list(bytes).expect("should decode");

        assert!(decoded.trusted_certificates.is_some());
        assert!(decoded.trusted_crls.is_none());
        assert!(decoded.issuer_certificates.is_none());
        assert!(decoded.issuer_crls.is_none());
    }

    #[tokio::test]
    async fn open_write_write_close_and_update_stages_pending_change_without_mutating_store() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let registry = Arc::new(GdsPushRegistry::default());
        let h = handler(registry.clone());

        let new_cert = self_signed_cert_with_cn("newly-trusted");
        let trust_list = TrustListDataType {
            specified_lists: masks::TRUSTED_CERTIFICATES,
            trusted_certificates: Some(vec![ByteString::from(
                new_cert.to_der().expect("cert should encode"),
            )]),
            ..Default::default()
        };
        let payload = encode_trust_list(&trust_list).expect("should encode");

        let open_out = h
            .handle_open(
                &context,
                &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
            )
            .expect("open for write should succeed");
        let Variant::UInt32(file_handle) = open_out[0] else {
            panic!("expected UInt32 file handle");
        };

        h.handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(ByteString::from(payload)),
            ],
        )
        .expect("write should succeed");

        let outputs = h
            .handle_close_and_update(&context, &[Variant::from(file_handle)])
            .expect("close and update should succeed");
        assert_eq!(outputs, vec![Variant::from(true)]);

        // Not yet applied.
        assert!(context
            .info
            .certificate_store
            .read()
            .read_trusted_certs()
            .is_empty());
        assert!(registry.transaction.read().is_some());
    }

    #[tokio::test]
    async fn close_and_update_with_invalid_certificate_is_rejected_without_mutating_store() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let h = handler(Arc::new(GdsPushRegistry::default()));

        let trust_list = TrustListDataType {
            specified_lists: masks::TRUSTED_CERTIFICATES,
            trusted_certificates: Some(vec![ByteString::from(vec![0u8; 16])]),
            ..Default::default()
        };
        let payload = encode_trust_list(&trust_list).expect("should encode");

        let open_out = h
            .handle_open(
                &context,
                &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
            )
            .expect("open for write should succeed");
        let Variant::UInt32(file_handle) = open_out[0] else {
            panic!("expected UInt32 file handle");
        };
        h.handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(ByteString::from(payload)),
            ],
        )
        .expect("write should succeed");

        assert_eq!(
            h.handle_close_and_update(&context, &[Variant::from(file_handle)]),
            Err(StatusCode::BadCertificateInvalid)
        );
        assert!(context
            .info
            .certificate_store
            .read()
            .read_trusted_certs()
            .is_empty());
    }

    #[tokio::test]
    async fn open_write_requires_security_admin() {
        let (context, _handle) = request_context(
            MessageSecurityMode::Sign,
            vec![WellKnownRole::AuthenticatedUser.node_id()],
        );
        let h = handler(Arc::new(GdsPushRegistry::default()));

        assert_eq!(
            h.handle_open(&context, &[Variant::from(open_mode::READ)]),
            Err(StatusCode::BadUserAccessDenied)
        );
    }

    #[tokio::test]
    async fn open_write_rejects_unauthenticated_channel() {
        let (context, _handle) = security_admin_request_context(MessageSecurityMode::None);
        let h = handler(Arc::new(GdsPushRegistry::default()));

        assert_eq!(
            h.handle_open(&context, &[Variant::from(open_mode::READ)]),
            Err(StatusCode::BadSecurityModeInsufficient)
        );
    }

    #[tokio::test]
    async fn add_certificate_immediately_adds_a_trusted_certificate() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let h = handler(Arc::new(GdsPushRegistry::default()));
        let cert = self_signed_cert_with_cn("added-directly");
        let der = cert.to_der().expect("cert should encode");

        h.handle_add_certificate(
            &context,
            &[Variant::from(ByteString::from(der)), Variant::from(true)],
        )
        .expect("add certificate should succeed");

        let trusted = context.info.certificate_store.read().read_trusted_certs();
        assert_eq!(trusted.len(), 1);
    }

    #[tokio::test]
    async fn add_certificate_rejects_is_trusted_certificate_false() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let h = handler(Arc::new(GdsPushRegistry::default()));
        let cert = self_signed_cert_with_cn("rejected-issuer-add");
        let der = cert.to_der().expect("cert should encode");

        assert_eq!(
            h.handle_add_certificate(
                &context,
                &[Variant::from(ByteString::from(der)), Variant::from(false)],
            ),
            Err(StatusCode::BadCertificateInvalid)
        );
    }

    #[tokio::test]
    async fn remove_certificate_immediately_removes_a_trusted_certificate() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let h = handler(Arc::new(GdsPushRegistry::default()));
        let cert = self_signed_cert_with_cn("to-be-removed");
        let store = context.info.certificate_store.read();
        store.store_trusted_cert(&cert).expect("should store");
        drop(store);

        h.handle_remove_certificate(
            &context,
            &[
                Variant::from(UAString::from(cert.thumbprint().as_hex_string())),
                Variant::from(true),
            ],
        )
        .expect("remove certificate should succeed");

        assert!(context
            .info
            .certificate_store
            .read()
            .read_trusted_certs()
            .is_empty());
    }

    #[tokio::test]
    async fn remove_certificate_refuses_a_still_needed_ca() {
        let (context, _handle) =
            security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let h = handler(Arc::new(GdsPushRegistry::default()));

        // Both certs are self-signed with the same CN, so each one's issuer name equals the
        // other's subject name -- exercising the name-based dependency check without needing a
        // full CA-signed chain (see research.md).
        let ca = self_signed_cert_with_cn("shared-ca-name");
        let dependent = self_signed_cert_with_cn("shared-ca-name");
        let store = context.info.certificate_store.read();
        store.store_trusted_cert(&ca).expect("should store");
        store.store_trusted_cert(&dependent).expect("should store");
        drop(store);

        assert_eq!(
            h.handle_remove_certificate(
                &context,
                &[
                    Variant::from(UAString::from(ca.thumbprint().as_hex_string())),
                    Variant::from(true),
                ],
            ),
            Err(StatusCode::BadCertificateChainIncomplete)
        );
        assert_eq!(
            context
                .info
                .certificate_store
                .read()
                .read_trusted_certs()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn add_certificate_rejects_while_write_transaction_open_elsewhere() {
        let registry = Arc::new(GdsPushRegistry::default());
        let (context1, _h1) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
        let h = handler(registry.clone());
        h.handle_open(
            &context1,
            &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
        )
        .expect("open for write should succeed");

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

        // context1's Open(write) doesn't itself reserve the transaction (only CloseAndUpdate
        // does, per Part 12 §7.8.2.2) -- stage one directly to simulate an in-progress
        // transaction from session 1.
        *registry.transaction.write() = Some(PushTransaction {
            owning_session_id: 1,
            certificate_der: None,
            private_key_pem: None,
            pending_trust_list: None,
        });

        let cert = self_signed_cert_with_cn("blocked-add");
        let der = cert.to_der().expect("cert should encode");
        assert_eq!(
            h.handle_add_certificate(
                &context2,
                &[Variant::from(ByteString::from(der)), Variant::from(true)],
            ),
            Err(StatusCode::BadTransactionPending)
        );
    }
}
