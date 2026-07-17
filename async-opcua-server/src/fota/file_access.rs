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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum HandleMode {
    Read,
    Write,
}

struct FotaFileHandleState {
    owning_session_id: u32,
    mode: HandleMode,
    file: File,
    position: u64,
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
    read_opens: AtomicU16,
    write_opens: AtomicU16,
    address_space: Arc<RwLock<AddressSpace>>,
    open_count_id: NodeId,
}

impl FotaFileAccessHandler {
    /// Creates a handler backed by `backing_path`, bounding `Read`/`Write` payloads at
    /// `max_byte_string_length` (OPC-10000-20 §4.2.1) and expiring abandoned handles after
    /// `idle_timeout` of inactivity (matching `TrustListHandleRegistry`'s `ActivityTimeout`-style
    /// precedent, see research.md).
    fn new(
        backing_path: PathBuf,
        max_byte_string_length: u32,
        address_space: Arc<RwLock<AddressSpace>>,
        open_count_id: NodeId,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            handles: FotaFileHandleRegistry::new(idle_timeout),
            backing_path,
            max_byte_string_length,
            read_opens: AtomicU16::new(0),
            write_opens: AtomicU16::new(0),
            address_space,
            open_count_id,
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

        // §4.2.2: a write-open is refused while the file is opened in any mode; a read-open is
        // refused only while the file is opened for writing. Multiple simultaneous read-opens
        // are allowed.
        if want_write
            && (self.write_opens.load(Ordering::Acquire) > 0
                || self.read_opens.load(Ordering::Acquire) > 0)
        {
            return Err(StatusCode::BadNotWritable);
        }
        if want_read && !want_write && self.write_opens.load(Ordering::Acquire) > 0 {
            return Err(StatusCode::BadNotReadable);
        }

        let mut open_options = OpenOptions::new();
        open_options.read(want_read);
        if want_write {
            open_options
                .write(true)
                .create(true)
                .truncate(mode & open_mode::ERASE_EXISTING != 0);
        }
        let mut file = open_options.open(&self.backing_path).map_err(|err| {
            if !want_write && err.kind() == std::io::ErrorKind::NotFound {
                StatusCode::BadNotFound
            } else {
                StatusCode::BadUnexpectedError
            }
        })?;

        let position = if want_write && mode & open_mode::APPEND != 0 {
            file.seek(SeekFrom::End(0))
                .map_err(|_| StatusCode::BadUnexpectedError)?
        } else {
            0
        };

        let handle_mode = if want_write {
            HandleMode::Write
        } else {
            HandleMode::Read
        };
        let handle = self.handles.insert(FotaFileHandleState {
            owning_session_id: context.session_id(),
            mode: handle_mode,
            file,
            position,
        });

        match handle_mode {
            HandleMode::Read => self.read_opens.fetch_add(1, Ordering::AcqRel),
            HandleMode::Write => self.write_opens.fetch_add(1, Ordering::AcqRel),
        };
        self.update_open_count();

        Ok(vec![Variant::from(handle)])
    }

    /// Handles `Close` (§4.2.3).
    pub fn handle_close(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let handle = u32_arg(args, 0)?;
        let entry = self.handles.get(handle, context.session_id())?;
        let mode = entry.lock().mode;
        self.handles.remove(handle);

        match mode {
            HandleMode::Read => self.read_opens.fetch_sub(1, Ordering::AcqRel),
            HandleMode::Write => self.write_opens.fetch_sub(1, Ordering::AcqRel),
        };
        self.update_open_count();

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
        if state.mode != HandleMode::Read {
            return Err(StatusCode::BadInvalidState);
        }

        let capped_length = (length as u64).min(self.max_byte_string_length as u64) as usize;
        let mut buffer = vec![0u8; capped_length];
        let position = state.position;
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
        if state.mode != HandleMode::Write {
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

    fn update_open_count(&self) {
        let count =
            self.read_opens.load(Ordering::Acquire) + self.write_opens.load(Ordering::Acquire);
        let address_space = self.address_space.read();
        if let Some(mut node) = address_space.find_mut(&self.open_count_id) {
            if let NodeType::Variable(variable) = &mut *node {
                let _ = variable.set_value(&NumericRange::None, count);
            }
        };
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
/// `file_node`'s already-built method NodeIds, backed by `backing_path` on disk.
pub fn register_file_access_methods(
    node_manager: &SimpleNodeManager,
    file_node: &TemporaryFileNode,
    backing_path: PathBuf,
    max_byte_string_length: u32,
) -> Arc<FotaFileAccessHandler> {
    let handler = Arc::new(FotaFileAccessHandler::new(
        backing_path,
        max_byte_string_length,
        node_manager.address_space().clone(),
        file_node.open_count_id.clone(),
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
            let path = std::env::temp_dir().join(format!(
                "async-opcua-file-access-test-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create temp dir");
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
            let space = handler.address_space.read();
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
            let space = handler.address_space.read();
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
}
