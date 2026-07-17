//! Real Open/Close/Read/Write/GetPosition/SetPosition behavior for a `FileType` object
//! (OPC-10000-20 §4.2), against a real backing file on disk.
//!
//! Structurally identical to `gds/trust_list/mod.rs`'s `TrustListHandleRegistry` (a
//! session-scoped, idle-timeout-bounded handle registry), but backed by a real `std::fs::File`
//! rather than an in-memory buffer -- appropriate for potentially large files (e.g. firmware
//! images) rather than TrustList's small certificate lists. See
//! `specs/106-file-access-io/research.md`.

use std::{
    fs::{File, OpenOptions},
    io::{Read as _, Seek, SeekFrom, Write as _},
    path::PathBuf,
    sync::{
        atomic::{AtomicU16, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use opcua_core::sync::{Mutex, RwLock};
use opcua_types::{ByteString, NodeId, NumericRange, StatusCode, Variant};

use crate::{
    address_space::{AddressSpace, NodeType},
    node_manager::memory::SimpleNodeManager,
    node_manager::RequestContext,
};

use super::file_node::TemporaryFileNode;

/// `OpenFileMode` bit values (OPC-10000-20 §4.2.2). Re-declared locally rather than using the
/// generated `opcua_types::OpenFileMode` enum, matching `gds/trust_list/mod.rs`'s established
/// precedent -- the generated enum's variants aren't directly OR-able for bitmask decoding.
mod open_mode {
    pub(super) const READ: u8 = 1;
    pub(super) const WRITE: u8 = 2;
    pub(super) const ERASE_EXISTING: u8 = 4;
    pub(super) const APPEND: u8 = 8;
    pub(super) const RESERVED: u8 = !(READ | WRITE | ERASE_EXISTING | APPEND);
}

/// State shared by every handle open against one `FileType` instance -- the live open-mode
/// counters used for §4.2.2's conflict rule, and the address-space bindings used to keep the
/// `OpenCount`/`Size` properties live. Held behind an `Arc` by both the handler and every open
/// `FotaFileHandleState`, so a handle's `Drop` impl can always reach it to reconcile counters --
/// including when moka evicts an abandoned handle via `time_to_idle`, not just on an explicit
/// `Close`.
struct FileAccessShared {
    read_opens: AtomicU16,
    write_opens: AtomicU16,
    address_space: Arc<RwLock<AddressSpace>>,
    open_count_id: NodeId,
    size_id: NodeId,
    writable: bool,
}

impl FileAccessShared {
    fn update_open_count(&self) {
        let count =
            self.read_opens.load(Ordering::Acquire) + self.write_opens.load(Ordering::Acquire);
        self.set_property(&self.open_count_id, count);
    }

    fn update_size(&self, size: u64) {
        self.set_property(&self.size_id, size);
    }

    fn set_property(&self, node_id: &NodeId, value: impl Into<Variant>) {
        let address_space = self.address_space.read();
        if let Some(mut node) = address_space.find_mut(node_id) {
            if let NodeType::Variable(variable) = &mut *node {
                let _ = variable.set_value(&NumericRange::None, value);
            }
        };
    }
}

struct FotaFileHandleState {
    owning_session_id: u32,
    can_read: bool,
    can_write: bool,
    file: File,
    position: u64,
    shared: Arc<FileAccessShared>,
    /// Set once the open-mode counters have been reconciled -- either by an explicit `Close`
    /// (which decrements synchronously, since a client legitimately expects to `Open` again
    /// right after `Close` returns) or by this type's own `Drop` impl. Without this flag, a
    /// `Close` would eventually double-decrement once moka's housekeeper gets around to actually
    /// dropping the cache-evicted `Arc` (moka's `invalidate` only *schedules* removal; the value
    /// is dropped later, off the calling thread).
    reconciled: bool,
}

/// Reconciles the live open-mode counters for handles that are *not* explicitly `Close`d --
/// i.e. abandoned by a crashed or disconnected client and later dropped by moka's idle-timeout
/// eviction. Without this, an abandoned write-open would permanently block every future
/// write-open even after the handle itself expires, since nothing else ever decrements it.
impl Drop for FotaFileHandleState {
    fn drop(&mut self) {
        if self.reconciled {
            return;
        }
        if self.can_read {
            self.shared.read_opens.fetch_sub(1, Ordering::AcqRel);
        }
        if self.can_write {
            self.shared.write_opens.fetch_sub(1, Ordering::AcqRel);
        }
        self.shared.update_open_count();
    }
}

/// Session-scoped, idle-timeout-bounded registry of open FileType handles. Directly modeled on
/// `gds/trust_list/mod.rs`'s `TrustListHandleRegistry`; see research.md for why this feature
/// holds a real `std::fs::File` per handle instead of an in-memory buffer.
struct FotaFileHandleRegistry {
    handles: moka::sync::Cache<u32, Arc<Mutex<FotaFileHandleState>>>,
    next_handle: AtomicU32,
}

impl FotaFileHandleRegistry {
    fn new(idle_timeout: Duration) -> Self {
        Self {
            handles: moka::sync::Cache::builder()
                .time_to_idle(idle_timeout)
                .build(),
            next_handle: AtomicU32::new(1),
        }
    }

    fn insert(&self, state: FotaFileHandleState) -> u32 {
        loop {
            let handle = self.next_handle.fetch_add(1, Ordering::Relaxed).max(1);
            if !self.handles.contains_key(&handle) {
                self.handles.insert(handle, Arc::new(Mutex::new(state)));
                return handle;
            }
        }
    }

    /// Per OPC-10000-20 §4.2 (every FileType method other than `Open`), an unknown or
    /// foreign-session handle is `Bad_InvalidArgument` -- not `Bad_InvalidState` as
    /// `TrustListHandleRegistry` uses for its own, Part-12-specific convention.
    fn get(
        &self,
        handle: u32,
        session_id: u32,
    ) -> Result<Arc<Mutex<FotaFileHandleState>>, StatusCode> {
        let entry = self
            .handles
            .get(&handle)
            .ok_or(StatusCode::BadInvalidArgument)?;
        if entry.lock().owning_session_id != session_id {
            return Err(StatusCode::BadInvalidArgument);
        }
        Ok(entry)
    }

    fn remove(&self, handle: u32) {
        self.handles.invalidate(&handle);
    }
}

/// Handler for real File Access method calls against one `FileType` instance.
pub struct FotaFileAccessHandler {
    handles: FotaFileHandleRegistry,
    backing_path: PathBuf,
    max_byte_string_length: u32,
    shared: Arc<FileAccessShared>,
    /// Serializes the check-then-increment sequence in `handle_open` so two concurrent `Open`
    /// calls can't both observe a clear conflict check before either has incremented the
    /// counters (a TOCTOU race that would let two callers open the file for write at once).
    open_lock: Mutex<()>,
}

impl FotaFileAccessHandler {
    /// Creates a handler backed by `backing_path`, bounding `Read`/`Write` payloads at
    /// `max_byte_string_length` (OPC-10000-20 §4.2.1) and expiring abandoned handles after
    /// `idle_timeout` of inactivity (matching `TrustListHandleRegistry`'s `ActivityTimeout`-style
    /// precedent, see research.md).
    #[allow(clippy::too_many_arguments)]
    fn new(
        backing_path: PathBuf,
        max_byte_string_length: u32,
        address_space: Arc<RwLock<AddressSpace>>,
        open_count_id: NodeId,
        size_id: NodeId,
        writable: bool,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            handles: FotaFileHandleRegistry::new(idle_timeout),
            backing_path,
            max_byte_string_length,
            shared: Arc::new(FileAccessShared {
                read_opens: AtomicU16::new(0),
                write_opens: AtomicU16::new(0),
                address_space,
                open_count_id,
                size_id,
                writable,
            }),
            open_lock: Mutex::new(()),
        }
    }

    /// Handles `Open` (§4.2.2).
    pub fn handle_open(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let mode = byte_arg(args, 0)?;
        if mode & open_mode::RESERVED != 0 {
            return Err(StatusCode::BadInvalidArgument);
        }

        let want_read = mode & open_mode::READ != 0;
        let want_write = mode & open_mode::WRITE != 0;
        if !want_read && !want_write {
            return Err(StatusCode::BadInvalidArgument);
        }
        let want_erase = mode & open_mode::ERASE_EXISTING != 0;
        if want_erase && !want_write {
            // EraseExisting only has meaning alongside Write (§4.2.2's mode table).
            return Err(StatusCode::BadInvalidArgument);
        }
        let want_append = mode & open_mode::APPEND != 0;

        // Holds for the whole check-open-insert sequence below: without it, two concurrent Opens
        // could both read a clear `write_opens`/`read_opens` count before either call's increment
        // becomes visible to the other, letting both pass the conflict check.
        let _open_guard = self.open_lock.lock();

        // §4.2.2: a write-open is refused while the file is opened in any mode; a read-open is
        // refused only while the file is opened for writing. Multiple simultaneous read-opens
        // are allowed.
        if want_write
            && (self.shared.write_opens.load(Ordering::Acquire) > 0
                || self.shared.read_opens.load(Ordering::Acquire) > 0)
        {
            return Err(StatusCode::BadNotWritable);
        }
        if want_read && !want_write && self.shared.write_opens.load(Ordering::Acquire) > 0 {
            return Err(StatusCode::BadNotReadable);
        }
        if want_write && !self.shared.writable {
            return Err(StatusCode::BadNotWritable);
        }

        let mut open_options = OpenOptions::new();
        open_options.read(want_read);
        if want_write {
            open_options.write(true).create(true).truncate(want_erase);
        }
        let mut file = open_options.open(&self.backing_path).map_err(|err| {
            if !want_write && err.kind() == std::io::ErrorKind::NotFound {
                StatusCode::BadNotFound
            } else {
                StatusCode::BadUnexpectedError
            }
        })?;

        // §4.2.2: Append applies to the initial position regardless of whether the file was also
        // opened for reading.
        let position = if want_append {
            file.seek(SeekFrom::End(0))
                .map_err(|_| StatusCode::BadUnexpectedError)?
        } else {
            0
        };

        if want_read {
            self.shared.read_opens.fetch_add(1, Ordering::AcqRel);
        }
        if want_write {
            self.shared.write_opens.fetch_add(1, Ordering::AcqRel);
        }
        self.shared.update_open_count();

        let handle = self.handles.insert(FotaFileHandleState {
            owning_session_id: context.session_id(),
            can_read: want_read,
            can_write: want_write,
            file,
            position,
            shared: self.shared.clone(),
            reconciled: false,
        });

        Ok(vec![Variant::from(handle)])
    }

    /// Handles `Close` (§4.2.3). Decrements the open-mode counters synchronously (a client
    /// legitimately expects to `Open` again immediately after `Close` returns -- moka's
    /// `invalidate` only *schedules* removal, so relying solely on the handle's `Drop` here would
    /// leave a window where a subsequent `Open` sees stale counts). Marks the handle
    /// `reconciled` so `Drop` -- which still runs later once moka actually drops the evicted
    /// `Arc` -- does not double-decrement.
    pub fn handle_close(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let handle = u32_arg(args, 0)?;
        let entry = self.handles.get(handle, context.session_id())?;
        {
            let mut state = entry.lock();
            if state.can_read {
                self.shared.read_opens.fetch_sub(1, Ordering::AcqRel);
            }
            if state.can_write {
                self.shared.write_opens.fetch_sub(1, Ordering::AcqRel);
            }
            state.reconciled = true;
        }
        self.shared.update_open_count();
        self.handles.remove(handle);
        Ok(vec![])
    }

    /// Handles `Read` (§4.2.4).
    pub fn handle_read(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let handle = u32_arg(args, 0)?;
        let length = i32_arg(args, 1)?;
        if length <= 0 {
            return Err(StatusCode::BadInvalidArgument);
        }

        let entry = self.handles.get(handle, context.session_id())?;
        let mut state = entry.lock();
        if !state.can_read {
            return Err(StatusCode::BadInvalidState);
        }

        let position = state.position;
        let file_len = state
            .file
            .metadata()
            .map_err(|_| StatusCode::BadUnexpectedError)?
            .len();
        let remaining = file_len.saturating_sub(position);
        let capped_length = (length as u64)
            .min(self.max_byte_string_length as u64)
            .min(remaining) as usize;
        let mut buffer = vec![0u8; capped_length];
        state
            .file
            .seek(SeekFrom::Start(position))
            .map_err(|_| StatusCode::BadUnexpectedError)?;
        let bytes_read = read_up_to(&mut state.file, &mut buffer)?;
        buffer.truncate(bytes_read);
        state.position = state.position.saturating_add(bytes_read as u64);

        Ok(vec![Variant::from(ByteString::from(buffer))])
    }

    /// Handles `Write` (§4.2.5).
    pub fn handle_write(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let handle = u32_arg(args, 0)?;
        let data = byte_string_arg(args, 1)?;

        let Some(bytes) = data.value.as_ref() else {
            // Writing an empty/null ByteString is a no-op that still requires a valid handle.
            self.handles.get(handle, context.session_id())?;
            return Ok(vec![]);
        };
        if bytes.len() as u64 > self.max_byte_string_length as u64 {
            return Err(StatusCode::BadInvalidArgument);
        }

        let entry = self.handles.get(handle, context.session_id())?;
        let mut state = entry.lock();
        if !state.can_write {
            return Err(StatusCode::BadInvalidState);
        }

        let position = state.position;
        state
            .file
            .seek(SeekFrom::Start(position))
            .map_err(|_| StatusCode::BadUnexpectedError)?;
        state
            .file
            .write_all(bytes)
            .map_err(|_| StatusCode::BadUnexpectedError)?;
        state.position = state.position.saturating_add(bytes.len() as u64);
        let new_len = state
            .file
            .metadata()
            .map_err(|_| StatusCode::BadUnexpectedError)?
            .len();
        state.shared.update_size(new_len);

        Ok(vec![])
    }

    /// Handles `GetPosition` (§4.2.6).
    pub fn handle_get_position(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let handle = u32_arg(args, 0)?;
        let entry = self.handles.get(handle, context.session_id())?;
        let position = entry.lock().position;
        Ok(vec![Variant::from(position)])
    }

    /// Handles `SetPosition` (§4.2.7). A position beyond the file's actual length clamps to
    /// end-of-file rather than erroring.
    pub fn handle_set_position(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let handle = u32_arg(args, 0)?;
        let requested_position = u64_arg(args, 1)?;

        let entry = self.handles.get(handle, context.session_id())?;
        let mut state = entry.lock();
        let file_len = state
            .file
            .metadata()
            .map_err(|_| StatusCode::BadUnexpectedError)?
            .len();
        state.position = requested_position.min(file_len);

        Ok(vec![])
    }
}

/// Reads up to `buffer.len()` bytes, returning the number actually read (may be less than the
/// buffer length at end-of-file -- not an error, per §4.2.4).
fn read_up_to(file: &mut File, buffer: &mut [u8]) -> Result<usize, StatusCode> {
    let mut total = 0;
    while total < buffer.len() {
        match file.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(StatusCode::BadUnexpectedError),
        }
    }
    Ok(total)
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

fn i32_arg(args: &[Variant], index: usize) -> Result<i32, StatusCode> {
    match args.get(index) {
        Some(Variant::Int32(value)) => Ok(*value),
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

fn byte_string_arg(args: &[Variant], index: usize) -> Result<ByteString, StatusCode> {
    match args.get(index) {
        Some(Variant::ByteString(value)) => Ok(value.clone()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

/// Registers real `Open`/`Close`/`Read`/`Write`/`GetPosition`/`SetPosition` callbacks against
/// `file_node`'s already-built method NodeIds, backed by `backing_path` on disk. `writable`
/// should match the node's own `Writable`/`UserWritable` properties (this handler treats both as
/// one combined flag -- there's no per-user distinction to enforce here beyond what
/// `RequestContext` already grants via method-call authorization).
pub fn register_file_access_methods(
    node_manager: &SimpleNodeManager,
    file_node: &TemporaryFileNode,
    backing_path: PathBuf,
    max_byte_string_length: u32,
    writable: bool,
) -> Arc<FotaFileAccessHandler> {
    let handler = Arc::new(FotaFileAccessHandler::new(
        backing_path,
        max_byte_string_length,
        node_manager.address_space().clone(),
        file_node.open_count_id.clone(),
        file_node.size_id.clone(),
        writable,
        Duration::from_secs(60),
    ));

    type Handle =
        fn(&FotaFileAccessHandler, &RequestContext, &[Variant]) -> Result<Vec<Variant>, StatusCode>;
    let bindings: [(NodeId, Handle); 6] = [
        (
            file_node.open_id.clone(),
            FotaFileAccessHandler::handle_open,
        ),
        (
            file_node.close_id.clone(),
            FotaFileAccessHandler::handle_close,
        ),
        (
            file_node.read_id.clone(),
            FotaFileAccessHandler::handle_read,
        ),
        (
            file_node.write_id.clone(),
            FotaFileAccessHandler::handle_write,
        ),
        (
            file_node.get_position_id.clone(),
            FotaFileAccessHandler::handle_get_position,
        ),
        (
            file_node.set_position_id.clone(),
            FotaFileAccessHandler::handle_set_position,
        ),
    ];

    for (method_id, invoke) in bindings {
        let h = handler.clone();
        node_manager
            .inner()
            .add_method_callback_with_context(method_id, move |ctx, args| invoke(&h, ctx, args));
    }

    handler
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use opcua_core::sync::RwLock as CoreRwLock;
    use opcua_crypto::SecurityPolicy;
    use opcua_types::{
        AnonymousIdentityToken, ApplicationDescription, MessageSecurityMode, UAString,
    };

    use crate::{
        address_space::AddressSpace, authenticator::UserToken, identity_token::IdentityToken,
        node_manager::RequestContextInner, session::instance::Session, ServerBuilder,
    };

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            // `tempfile::Builder` (not `std::env::temp_dir()` directly) so the directory gets a
            // securely-permissioned, randomized name rather than a predictable one -- matching
            // `gds/pull_methods/tests.rs`'s `unique_test_pki_dir()` precedent.
            let path = tempfile::Builder::new()
                .prefix(&format!("async-opcua-file-access-test-{name}-"))
                .tempdir()
                .expect("failed to create a securely-permissioned test scratch directory")
                .keep();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn test_context(session_id: u32) -> (RequestContext, crate::ServerHandle) {
        let (_server, handle) = ServerBuilder::new_anonymous("file access test")
            .without_node_managers()
            .create_sample_keypair(true)
            .build()
            .expect("test server should build");
        let info = Arc::clone(handle.info());
        let session = Arc::new(CoreRwLock::new(Session::create(
            &info,
            opcua_types::NodeId::new(0, session_id),
            1,
            60_000,
            0,
            0,
            UAString::from("opc.tcp://localhost"),
            SecurityPolicy::None.to_uri().to_string(),
            IdentityToken::Anonymous(AnonymousIdentityToken {
                policy_id: UAString::from("anonymous"),
            }),
            None,
            opcua_types::ByteString::null(),
            UAString::from("file-access-test"),
            ApplicationDescription::default(),
            MessageSecurityMode::None,
        )));

        let context = RequestContext::new_test(Arc::new(RequestContextInner {
            session,
            session_id,
            authenticator: info.authenticator.clone(),
            token: UserToken("file-access-test".to_string()),
            user_roles: Arc::new(Vec::new()),
            type_tree: info.type_tree.clone(),
            type_tree_getter: info.type_tree_getter.clone(),
            subscriptions: handle.subscriptions().clone(),
            info,
        }));
        (context, handle)
    }

    fn test_handler(dir: &TempDir) -> (Arc<FotaFileAccessHandler>, NodeId) {
        test_handler_with_writable(dir, true)
    }

    fn test_handler_with_writable(
        dir: &TempDir,
        writable: bool,
    ) -> (Arc<FotaFileAccessHandler>, NodeId) {
        let address_space = Arc::new(CoreRwLock::new(AddressSpace::new()));
        let file_node = {
            let space = address_space.write();
            TemporaryFileNode::create(
                &space,
                super::super::file_node::TemporaryFileNodeConfig::new(
                    2,
                    NodeId::new(0, "test-session"),
                    "test.bin",
                ),
            )
            .expect("temporary file node should be created")
        };
        let open_count_id = file_node.open_count_id.clone();
        let backing_path = dir.path().join("test.bin");
        let handler = Arc::new(FotaFileAccessHandler::new(
            backing_path,
            1024,
            address_space,
            open_count_id.clone(),
            file_node.size_id.clone(),
            writable,
            Duration::from_secs(60),
        ));
        (handler, open_count_id)
    }

    fn open_args(mode: u8) -> Vec<Variant> {
        vec![Variant::from(mode)]
    }

    #[tokio::test]
    async fn write_then_read_round_trips_real_bytes() {
        let dir = TempDir::new("round-trip");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let Variant::UInt32(write_handle) = outputs[0] else {
            panic!("expected a UInt32 file handle");
        };

        handler
            .handle_write(
                &context,
                &[
                    Variant::from(write_handle),
                    Variant::from(ByteString::from(b"hello file access".to_vec())),
                ],
            )
            .expect("write should succeed");
        handler
            .handle_close(&context, &[Variant::from(write_handle)])
            .expect("close should succeed");

        let outputs = handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect("open for read should succeed");
        let Variant::UInt32(read_handle) = outputs[0] else {
            panic!("expected a UInt32 file handle");
        };

        let mut collected = Vec::new();
        loop {
            let outputs = handler
                .handle_read(
                    &context,
                    &[Variant::from(read_handle), Variant::from(64_i32)],
                )
                .expect("read should succeed");
            let Variant::ByteString(data) = &outputs[0] else {
                panic!("expected a ByteString");
            };
            let Some(bytes) = data.value.as_ref() else {
                break;
            };
            if bytes.is_empty() {
                break;
            }
            collected.extend_from_slice(bytes);
        }
        handler
            .handle_close(&context, &[Variant::from(read_handle)])
            .expect("close should succeed");

        assert_eq!(collected, b"hello file access".to_vec());
    }

    #[tokio::test]
    async fn second_write_open_is_rejected_while_first_is_open() {
        let dir = TempDir::new("write-conflict");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("first open for write should succeed");

        let err = handler
            .handle_open(&context, &open_args(open_mode::WRITE))
            .expect_err("second concurrent write-open should fail");
        assert_eq!(err, StatusCode::BadNotWritable);
    }

    #[tokio::test]
    async fn read_open_is_rejected_while_open_for_write() {
        let dir = TempDir::new("read-write-conflict");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");

        let err = handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect_err("read-open while write-open should fail");
        assert_eq!(err, StatusCode::BadNotReadable);
    }

    #[tokio::test]
    async fn multiple_simultaneous_read_opens_are_allowed() {
        let dir = TempDir::new("multi-read");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .and_then(|outputs| {
                let Variant::UInt32(h) = outputs[0] else {
                    unreachable!()
                };
                handler.handle_close(&context, &[Variant::from(h)])
            })
            .expect("seed file for reading");

        let first = handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect("first read-open should succeed");
        let second = handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect("second concurrent read-open should also succeed");
        assert_ne!(first[0], second[0]);
    }

    #[tokio::test]
    async fn handle_from_a_different_session_is_rejected() {
        let dir = TempDir::new("cross-session");
        let (handler, _) = test_handler(&dir);
        let (context_a, _server_a) = test_context(1);
        let (context_b, _server_b) = test_context(2);

        let outputs = handler
            .handle_open(
                &context_a,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open should succeed for session A");
        let handle = outputs[0].clone();

        let err = handler
            .handle_write(
                &context_b,
                &[handle, Variant::from(ByteString::from(b"x".to_vec()))],
            )
            .expect_err("a different session's handle must be rejected");
        assert_eq!(err, StatusCode::BadInvalidArgument);
    }

    #[tokio::test]
    async fn empty_write_is_a_noop_but_still_requires_a_valid_handle() {
        let dir = TempDir::new("empty-write");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let handle = outputs[0].clone();

        handler
            .handle_write(&context, &[handle, Variant::from(ByteString::null())])
            .expect("empty write should succeed as a no-op");

        let err = handler
            .handle_write(
                &context,
                &[Variant::from(999_u32), Variant::from(ByteString::null())],
            )
            .expect_err("an unknown handle should still fail even for an empty write");
        assert_eq!(err, StatusCode::BadInvalidArgument);
    }

    #[tokio::test]
    async fn write_larger_than_max_byte_string_length_is_rejected() {
        let dir = TempDir::new("write-too-large");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let handle = outputs[0].clone();

        let oversized = vec![0u8; 2048];
        let err = handler
            .handle_write(
                &context,
                &[handle, Variant::from(ByteString::from(oversized))],
            )
            .expect_err("oversized write should be rejected");
        assert_eq!(err, StatusCode::BadInvalidArgument);
    }

    #[tokio::test]
    async fn read_length_is_capped_at_max_byte_string_length() {
        let dir = TempDir::new("read-cap");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let write_handle = outputs[0].clone();
        // Two writes of 1024 bytes each (the handler's MaxByteStringLength) -- each individual
        // call respects the per-call cap; the resulting file is larger than the cap to prove
        // Read, not Write, is what's under test here.
        for _ in 0..4 {
            handler
                .handle_write(
                    &context,
                    &[
                        write_handle.clone(),
                        Variant::from(ByteString::from(vec![7u8; 1024])),
                    ],
                )
                .expect("write should succeed");
        }
        let Variant::UInt32(write_handle) = write_handle else {
            unreachable!()
        };
        handler
            .handle_close(&context, &[Variant::from(write_handle)])
            .expect("close should succeed");

        let outputs = handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect("open for read should succeed");
        let read_handle = outputs[0].clone();

        let outputs = handler
            .handle_read(&context, &[read_handle, Variant::from(4096_i32)])
            .expect("read should succeed");
        let Variant::ByteString(data) = &outputs[0] else {
            panic!("expected a ByteString");
        };
        assert_eq!(
            data.value.as_ref().map(|bytes| bytes.len()),
            Some(1024),
            "read should be capped at MaxByteStringLength, not the requested length"
        );
    }

    #[tokio::test]
    async fn read_length_is_capped_at_remaining_file_size() {
        let dir = TempDir::new("read-remaining-cap");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let write_handle = outputs[0].clone();
        handler
            .handle_write(
                &context,
                &[
                    write_handle.clone(),
                    Variant::from(ByteString::from(b"12345".to_vec())),
                ],
            )
            .expect("write should succeed");
        let Variant::UInt32(write_handle) = write_handle else {
            unreachable!()
        };
        handler
            .handle_close(&context, &[Variant::from(write_handle)])
            .expect("close should succeed");

        let outputs = handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect("open for read should succeed");
        let read_handle = outputs[0].clone();

        // Requesting far more than the file's 5 remaining bytes should not allocate a buffer
        // sized to MaxByteStringLength (1024) -- it should be bounded by what's actually left.
        let outputs = handler
            .handle_read(&context, &[read_handle, Variant::from(1024_i32)])
            .expect("read should succeed");
        let Variant::ByteString(data) = &outputs[0] else {
            panic!("expected a ByteString");
        };
        assert_eq!(data.value.as_ref().map(|bytes| bytes.len()), Some(5));
    }

    #[tokio::test]
    async fn set_position_beyond_end_of_file_clamps_to_eof() {
        let dir = TempDir::new("set-position-clamp");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let handle = outputs[0].clone();
        handler
            .handle_write(
                &context,
                &[
                    handle.clone(),
                    Variant::from(ByteString::from(b"12345".to_vec())),
                ],
            )
            .expect("write should succeed");

        handler
            .handle_set_position(&context, &[handle.clone(), Variant::from(999_u64)])
            .expect("set position should succeed");
        let outputs = handler
            .handle_get_position(&context, &[handle])
            .expect("get position should succeed");
        assert_eq!(outputs[0], Variant::from(5_u64));
    }

    #[tokio::test]
    async fn invalid_mode_bits_are_rejected() {
        let dir = TempDir::new("invalid-mode");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let err = handler
            .handle_open(&context, &open_args(0b1111_0000))
            .expect_err("reserved bits must be rejected");
        assert_eq!(err, StatusCode::BadInvalidArgument);
    }

    #[tokio::test]
    async fn erase_existing_without_write_is_rejected() {
        let dir = TempDir::new("erase-without-write");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let err = handler
            .handle_open(
                &context,
                &open_args(open_mode::READ | open_mode::ERASE_EXISTING),
            )
            .expect_err("EraseExisting without Write must be rejected");
        assert_eq!(err, StatusCode::BadInvalidArgument);
    }

    #[tokio::test]
    async fn write_open_is_rejected_when_not_writable() {
        let dir = TempDir::new("not-writable");
        std::fs::write(dir.path().join("test.bin"), b"seed").expect("seed file should be written");
        let (handler, _) = test_handler_with_writable(&dir, false);
        let (context, _server) = test_context(1);

        let err = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect_err("write-open on a non-writable file must be rejected");
        assert_eq!(err, StatusCode::BadNotWritable);

        // A pre-existing file should still be readable.
        handler
            .handle_open(&context, &open_args(open_mode::READ))
            .expect("read-open should still succeed on a non-writable file");
    }

    #[tokio::test]
    async fn a_handle_opened_for_both_read_and_write_supports_both_operations() {
        let dir = TempDir::new("read-write-combined");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::READ | open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("combined read+write open should succeed");
        let handle = outputs[0].clone();

        handler
            .handle_write(
                &context,
                &[
                    handle.clone(),
                    Variant::from(ByteString::from(b"rw".to_vec())),
                ],
            )
            .expect("write should succeed on a combined handle");
        handler
            .handle_set_position(&context, &[handle.clone(), Variant::from(0_u64)])
            .expect("set position should succeed");
        let outputs = handler
            .handle_read(&context, &[handle, Variant::from(2_i32)])
            .expect("read should succeed on the same combined handle");
        let Variant::ByteString(data) = &outputs[0] else {
            panic!("expected a ByteString");
        };
        assert_eq!(
            data.value.as_ref().map(|b| b.to_vec()),
            Some(b"rw".to_vec())
        );
    }

    #[tokio::test]
    async fn append_applies_to_read_only_opens_too() {
        let dir = TempDir::new("append-read-only");
        let (handler, _) = test_handler(&dir);
        let (context, _server) = test_context(1);

        handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .and_then(|outputs| {
                let Variant::UInt32(h) = outputs[0] else {
                    unreachable!()
                };
                handler.handle_write(
                    &context,
                    &[
                        Variant::from(h),
                        Variant::from(ByteString::from(b"12345".to_vec())),
                    ],
                )?;
                handler.handle_close(&context, &[Variant::from(h)])
            })
            .expect("seed file for reading");

        let outputs = handler
            .handle_open(&context, &open_args(open_mode::READ | open_mode::APPEND))
            .expect("read+append open should succeed");
        let handle = outputs[0].clone();
        let outputs = handler
            .handle_get_position(&context, &[handle])
            .expect("get position should succeed");
        assert_eq!(outputs[0], Variant::from(5_u64));
    }

    #[tokio::test]
    async fn write_updates_the_size_property() {
        let dir = TempDir::new("size-update");
        let address_space = Arc::new(CoreRwLock::new(AddressSpace::new()));
        let file_node = {
            let space = address_space.write();
            TemporaryFileNode::create(
                &space,
                super::super::file_node::TemporaryFileNodeConfig::new(
                    2,
                    NodeId::new(0, "test-session"),
                    "test.bin",
                ),
            )
            .expect("temporary file node should be created")
        };
        let backing_path = dir.path().join("test.bin");
        let handler = Arc::new(FotaFileAccessHandler::new(
            backing_path,
            1024,
            address_space.clone(),
            file_node.open_count_id.clone(),
            file_node.size_id.clone(),
            true,
            Duration::from_secs(60),
        ));
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open for write should succeed");
        let handle = outputs[0].clone();
        handler
            .handle_write(
                &context,
                &[handle, Variant::from(ByteString::from(b"12345".to_vec()))],
            )
            .expect("write should succeed");

        let size = {
            let space = address_space.read();
            let node = space.find(&file_node.size_id).expect("size node exists");
            let NodeType::Variable(var) = &*node else {
                panic!("expected a variable node");
            };
            var.value(
                opcua_types::TimestampsToReturn::Neither,
                &NumericRange::None,
                &opcua_types::DataEncoding::Binary,
                0.0,
            )
            .value
        };
        assert_eq!(size, Some(Variant::from(5_u64)));
    }

    #[tokio::test]
    async fn open_count_reflects_live_handle_count() {
        let dir = TempDir::new("open-count");
        let (handler, open_count_id) = test_handler(&dir);
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open should succeed");

        let count = {
            let space = handler.shared.address_space.read();
            let node = space.find(&open_count_id).expect("open count node exists");
            let opcua_nodes::NodeType::Variable(var) = &*node else {
                panic!("expected a variable node");
            };
            var.value(
                opcua_types::TimestampsToReturn::Neither,
                &NumericRange::None,
                &opcua_types::DataEncoding::Binary,
                0.0,
            )
            .value
        };
        assert_eq!(count, Some(Variant::from(1_u16)));

        let Variant::UInt32(handle) = outputs[0] else {
            unreachable!()
        };
        handler
            .handle_close(&context, &[Variant::from(handle)])
            .expect("close should succeed");

        let count = {
            let space = handler.shared.address_space.read();
            let node = space.find(&open_count_id).expect("open count node exists");
            let opcua_nodes::NodeType::Variable(var) = &*node else {
                panic!("expected a variable node");
            };
            var.value(
                opcua_types::TimestampsToReturn::Neither,
                &NumericRange::None,
                &opcua_types::DataEncoding::Binary,
                0.0,
            )
            .value
        };
        assert_eq!(count, Some(Variant::from(0_u16)));
    }

    #[tokio::test]
    async fn open_count_reconciles_when_a_handle_is_evicted_instead_of_closed() {
        let dir = TempDir::new("open-count-eviction");
        let address_space = Arc::new(CoreRwLock::new(AddressSpace::new()));
        let file_node = {
            let space = address_space.write();
            TemporaryFileNode::create(
                &space,
                super::super::file_node::TemporaryFileNodeConfig::new(
                    2,
                    NodeId::new(0, "test-session"),
                    "test.bin",
                ),
            )
            .expect("temporary file node should be created")
        };
        let backing_path = dir.path().join("test.bin");
        // A near-zero idle timeout so the handle is evicted on the next cache operation without
        // an explicit Close -- simulating an abandoned connection.
        let handler = Arc::new(FotaFileAccessHandler::new(
            backing_path,
            1024,
            address_space,
            file_node.open_count_id.clone(),
            file_node.size_id.clone(),
            true,
            Duration::from_millis(1),
        ));
        let (context, _server) = test_context(1);

        let outputs = handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("open should succeed");
        let Variant::UInt32(handle) = outputs[0] else {
            unreachable!()
        };

        // moka's idle-time eviction is checked lazily -- an access (or `run_pending_tasks`) is
        // what actually notices the entry is expired and schedules its removal, and the removed
        // value's `Drop` (which reconciles the counters) runs on a background housekeeper thread
        // some time after that. Poll with a bound rather than assuming a single call reconciles
        // synchronously.
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            handler.handles.handles.contains_key(&handle);
            handler.handles.handles.run_pending_tasks();
            if handler.shared.write_opens.load(Ordering::Acquire) == 0 {
                break;
            }
        }

        assert_eq!(handler.shared.write_opens.load(Ordering::Acquire), 0);
        // A second write-open should now succeed instead of being permanently locked out.
        handler
            .handle_open(
                &context,
                &open_args(open_mode::WRITE | open_mode::ERASE_EXISTING),
            )
            .expect("write-open should succeed after the abandoned handle is reconciled");
    }
}
