//! `TemporaryFileTransferType` (OPC-10000-20 §4.4.1-§4.4.5): on-demand temporary file
//! generation with **no client-supplied path**.
//!
//! Three methods (CUs 3810/3811/3812/3813/5791):
//! - [`GenerateFileForRead`](https://reference.opcfoundation.org/specs/OPC-10000-20/4.4.3.md)
//!   (§4.4.3): the server creates a temp `FileType` under an operator-configured `temp_dir`, runs
//!   a producer callback to fill it, and returns the node + an already-open *read* handle.
//! - [`GenerateFileForWrite`](https://reference.opcfoundation.org/specs/OPC-10000-20/4.4.4.md)
//!   (§4.4.4): the server creates a writable temp `FileType` and returns the node + an open
//!   *write* handle. The client writes via feature 106's existing `Write`.
//! - [`CloseAndCommit`](https://reference.opcfoundation.org/specs/OPC-10000-20/4.4.5.md)
//!   (§4.4.5): invokes a consumer callback with the committed bytes, then deletes the temp file
//!   and its node.
//!
//! `completionStateMachine` is always a null `NodeId`: synchronous completion is explicitly valid
//! per §4.4.6 ("If the transactions are completed when the Method is returned, the optional
//! ... parameter returns a null NodeId").
//!
//! # Security (Constitution Principle IV -- Security Is Paramount)
//!
//! There is **no path-traversal surface**: the server creates temp files at server-chosen paths
//! under `temp_dir`, and `generateOptions` is type-checked, server-specific data -- never
//! interpreted as a path. Disk-exhaustion DoS is bounded three ways: a per-transfer
//! `max_total_bytes` cap enforced on *every* `Write` (not just at commit), moka idle-timeout
//! reaping abandoned handles, and session-disconnect cleanup (reusing `fota::cleanup`).

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Weak,
    },
    time::Duration,
};

use opcua_core::sync::RwLock;
use opcua_types::{
    Argument, DataTypeId, DateTime, LocalizedText, NodeId, ObjectTypeId, StatusCode,
    VariableTypeId, Variant,
};
use tracing::warn;

use crate::{
    address_space::{AddressSpace, MethodBuilder, ObjectBuilder, VariableBuilder},
    fota::{
        cleanup::register_session_file,
        file_access::{register_file_access_methods_full, FotaFileAccessHandler},
        file_node::{TemporaryFileNode, TemporaryFileNodeConfig},
    },
    node_manager::memory::SimpleNodeManager,
    node_manager::RequestContext,
};

/// Callback that fills a freshly-created temp file for `GenerateFileForRead`. Receives the
/// server-chosen backing path and the type-checked `generateOptions`. Returning `Err` fails the
/// whole `GenerateFileForRead` (the partially-created file and node are cleaned up first).
pub type ProducerFn = Arc<dyn Fn(&Path, &Variant) -> Result<(), StatusCode> + Send + Sync>;

/// Callback that receives the committed bytes for `CloseAndCommit`. Receives the committed
/// bytes and the `generateOptions` originally passed to `GenerateFileForWrite`. The application
/// owns content validation; returning `Err` fails the `CloseAndCommit` (the temp file is still
/// deleted -- the transaction completes regardless).
pub type ConsumerFn = Arc<dyn Fn(&[u8], &Variant) -> Result<(), StatusCode> + Send + Sync>;

/// Configuration for a [`TemporaryFileTransferHandler`].
#[derive(Debug, Clone)]
pub struct TemporaryFileTransferConfig {
    /// Namespace index used for generated transfer and temp-file nodes.
    pub namespace_index: u16,
    /// Namespace URI registered in the address space for `namespace_index`.
    pub namespace_uri: String,
    /// Operator-configured directory under which server-chosen temp files are created. The
    /// handler ensures it exists; it is never derived from client input.
    pub temp_dir: PathBuf,
    /// Per-transfer cap on total committed file size, enforced on every `Write` and re-checked
    /// at `CloseAndCommit`. Bounds disk-exhaustion DoS.
    pub max_total_bytes: u64,
    /// Per-call cap on `Read`/`Write` payload size, forwarded to each temp `FileType`
    /// (OPC-10000-20 §4.2.1 `MaxByteStringLength`).
    pub max_byte_string_length: u32,
    /// Idle timeout after which an abandoned file handle (and its transfer record) self-expires.
    pub idle_timeout: Duration,
    /// Value of the `ClientProcessingTimeout` property (§4.4.1), in milliseconds.
    pub client_processing_timeout_ms: u64,
    /// Optional `DataType` NodeId that `generateOptions` must match. `None` accepts any
    /// non-empty `generateOptions` (no type-check); `Some(_)` rejects a mismatched type with
    /// `Bad_TypeMismatch`. An empty/absent `generateOptions` is always accepted (the parameter
    /// is optional per §4.4.3/§4.4.4).
    pub generate_options_type: Option<NodeId>,
}

impl TemporaryFileTransferConfig {
    /// Create a config with sensible defaults for the given namespace and temp directory.
    pub fn new(namespace_index: u16, namespace_uri: impl Into<String>, temp_dir: PathBuf) -> Self {
        Self {
            namespace_index,
            namespace_uri: namespace_uri.into(),
            temp_dir,
            max_total_bytes: 16 * 1024 * 1024,
            max_byte_string_length: 64 * 1024,
            idle_timeout: Duration::from_secs(60),
            client_processing_timeout_ms: 60_000,
            generate_options_type: None,
        }
    }
}

/// NodeIds created for a `TemporaryFileTransferType` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryFileTransferNode {
    /// The transfer object itself.
    pub transfer_object_id: NodeId,
    /// `ClientProcessingTimeout` property.
    pub client_processing_timeout_id: NodeId,
    /// `GenerateFileForRead` method.
    pub generate_file_for_read_id: NodeId,
    /// `GenerateFileForWrite` method.
    pub generate_file_for_write_id: NodeId,
    /// `CloseAndCommit` method.
    pub close_and_commit_id: NodeId,
}

impl TemporaryFileTransferNode {
    /// Create and insert a `TemporaryFileTransferType` object with its mandatory property and
    /// three method components, attached under `parent_id` (if given).
    pub fn create(
        address_space: &AddressSpace,
        config: &TemporaryFileTransferConfig,
        parent_id: Option<NodeId>,
        browse_name: &str,
    ) -> Result<Self, StatusCode> {
        address_space.add_namespace(&config.namespace_uri, config.namespace_index);

        let base = format!(
            "TFT_{}_{}",
            sanitize(config.namespace_uri.as_str()),
            sanitize(browse_name)
        );
        let ns = config.namespace_index;
        let node = Self {
            transfer_object_id: NodeId::new(ns, base.clone()),
            client_processing_timeout_id: NodeId::new(
                ns,
                format!("{base}_ClientProcessingTimeout"),
            ),
            generate_file_for_read_id: NodeId::new(ns, format!("{base}_GenerateFileForRead")),
            generate_file_for_write_id: NodeId::new(ns, format!("{base}_GenerateFileForWrite")),
            close_and_commit_id: NodeId::new(ns, format!("{base}_CloseAndCommit")),
        };

        let mut object_builder =
            ObjectBuilder::new(&node.transfer_object_id, browse_name, browse_name)
                .has_type_definition(ObjectTypeId::TemporaryFileTransferType);
        if let Some(parent) = parent_id {
            object_builder = object_builder.component_of(parent);
        }
        if !object_builder.insert(address_space) {
            return Err(StatusCode::BadNodeIdExists);
        }

        insert_property(
            address_space,
            &node.transfer_object_id,
            &node.client_processing_timeout_id,
            "ClientProcessingTimeout",
            DataTypeId::Duration,
            config.client_processing_timeout_ms as f64,
        )?;

        insert_method(
            address_space,
            &node.transfer_object_id,
            &node.generate_file_for_read_id,
            "GenerateFileForRead",
            &[argument_with_type(
                "generateOptions",
                config
                    .generate_options_type
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| DataTypeId::BaseDataType.into()),
            )],
            &[
                argument("fileNodeId", DataTypeId::NodeId),
                argument("fileHandle", DataTypeId::UInt32),
                argument("completionStateMachine", DataTypeId::NodeId),
            ],
        )?;
        insert_method(
            address_space,
            &node.transfer_object_id,
            &node.generate_file_for_write_id,
            "GenerateFileForWrite",
            &[argument_with_type(
                "generateOptions",
                config
                    .generate_options_type
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| DataTypeId::BaseDataType.into()),
            )],
            &[
                argument("fileNodeId", DataTypeId::NodeId),
                argument("fileHandle", DataTypeId::UInt32),
            ],
        )?;
        insert_method(
            address_space,
            &node.transfer_object_id,
            &node.close_and_commit_id,
            "CloseAndCommit",
            &[argument("fileHandle", DataTypeId::UInt32)],
            &[argument("completionStateMachine", DataTypeId::NodeId)],
        )?;

        Ok(node)
    }
}

/// What kind of transfer a [`TransferRecord`] represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferKind {
    /// Started by `GenerateFileForRead`; the handle is open for reading.
    Read,
    /// Started by `GenerateFileForWrite`; the handle is open for writing and may be committed.
    Write,
}

/// Per-transfer state, keyed in [`HandlerInner::transfers`] by the (globally-unique) file handle.
struct TransferRecord {
    session_id: u32,
    kind: TransferKind,
    file_node: TemporaryFileNode,
    backing_path: PathBuf,
    /// The per-file handler bound to the temp `FileType`'s Open/Read/Write/Close methods. Kept
    /// alive for the transfer's lifetime so the client's `Read`/`Write` calls resolve.
    #[allow(dead_code)]
    file_access: Arc<FotaFileAccessHandler>,
    file_access_handle: u32,
    generate_options: Variant,
    address_space: Weak<RwLock<AddressSpace>>,
    /// Set by `CloseAndCommit` so a later eviction-driven `Drop` does not redo cleanup.
    committed: AtomicBool,
}

impl Drop for TransferRecord {
    /// Best-effort cleanup of an abandoned transfer's backing file and address-space nodes. This
    /// mirrors feature 106's reliance on moka's idle-timeout eviction to drop abandoned handles:
    /// when a transfer record is evicted without being committed (client crashed or stopped
    /// mid-transaction without disconnecting), its temp file and nodes are reaped. All operations
    /// are idempotent -- `register_session_file` may also clean up on disconnect.
    fn drop(&mut self) {
        if self.committed.load(Ordering::Acquire) {
            return;
        }
        cleanup_transfer(&self.address_space, &self.file_node, &self.backing_path);
    }
}

fn cleanup_transfer(
    address_space: &Weak<RwLock<AddressSpace>>,
    file_node: &TemporaryFileNode,
    backing_path: &Path,
) {
    if let Some(address_space) = address_space.upgrade() {
        let address_space = opcua_core::trace_write_lock!(address_space);
        for node_id in file_node.node_ids() {
            let _ = address_space.delete(&node_id, true);
        }
    }
    if let Err(err) = std::fs::remove_file(backing_path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(
                "TemporaryFileTransfer cleanup failed to delete {}: {err}",
                backing_path.display()
            );
        }
    }
}

struct HandlerInner {
    config: TemporaryFileTransferConfig,
    producer: Option<ProducerFn>,
    consumer: Option<ConsumerFn>,
    node_manager: Arc<SimpleNodeManager>,
    address_space: Arc<RwLock<AddressSpace>>,
    /// Active transfers keyed by their globally-unique file handle. Evicted by moka after
    /// `idle_timeout` of inactivity -- the value's `Drop` reaps an uncommitted transfer's file
    /// and nodes (matching feature 106's abandoned-handle reaping).
    transfers: moka::sync::Cache<u32, Arc<TransferRecord>>,
    /// Shared across every per-transfer `FotaFileAccessHandler` so a client's `fileHandle`
    /// resolves to exactly one transfer regardless of which temp file issued it.
    handle_counter: Arc<AtomicU32>,
    /// Per-handler uniquifier for temp file names (independent of the handle counter, so a name
    /// stays unique even after an evicted handle's number is recycled past the wrap point).
    file_name_counter: AtomicU64,
}

impl HandlerInner {
    /// Picks a fresh, unique `file_name` (used for the temp `FileType` node's browse name and
    /// NodeId) and backing path under `temp_dir`. The same unique number keys both so a node and
    /// its file share an unambiguous identity -- this is what lets two concurrent transfers in
    /// one session coexist without colliding on the address-space NodeId
    /// (`FOTA_{session}_{file_name}`, derived from `file_name`).
    fn next_name_and_path(&self, prefix: &str) -> (String, PathBuf) {
        let n = self.file_name_counter.fetch_add(1, Ordering::Relaxed);
        (
            format!("{prefix}_{n}"),
            self.config
                .temp_dir
                .join(format!("async-opcua-tft-{}-{n}.bin", std::process::id())),
        )
    }

    /// Builds and inserts a temp `FileType` node for this transfer, returning the node and its
    /// per-file access handler.
    fn create_temp_file_node(
        &self,
        session_node_id: NodeId,
        file_name: &str,
        writable: bool,
    ) -> Result<(TemporaryFileNode, TemporaryFileNodeConfig), StatusCode> {
        let config = TemporaryFileNodeConfig {
            namespace_index: self.config.namespace_index,
            namespace_uri: self.config.namespace_uri.clone(),
            session_id: session_node_id,
            file_name: file_name.to_owned(),
            parent_id: None,
            size: 0,
            writable,
            user_writable: writable,
            mime_type: "application/octet-stream".to_owned(),
            max_byte_string_length: self.config.max_byte_string_length,
            last_modified_time: DateTime::now(),
        };
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        let file_node = TemporaryFileNode::create(&address_space, config.clone())?;
        Ok((file_node, config))
    }

    fn handle_generate_file_for_read(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let options = validate_generate_options(args, self.config.generate_options_type.as_ref())?;
        std::fs::create_dir_all(&self.config.temp_dir)
            .map_err(|_| StatusCode::BadUnexpectedError)?;

        // One unique number keys both the backing path and the temp FileType node name. Sharing
        // the counter value guarantees the node's NodeId (derived from session_id + file_name)
        // is distinct across concurrent transfers in the same session -- otherwise the second
        // GenerateFileForRead in a session would collide on `FOTA_{session}_GeneratedFile`.
        let (file_name, backing_path) = self.next_name_and_path("GeneratedFile");

        // Run the (server-trusted) producer first. On any failure, remove the partial file and
        // surface the producer's status code -- no node has been created yet to clean up.
        if let Some(producer) = &self.producer {
            if let Err(status) = producer(&backing_path, &options) {
                let _ = std::fs::remove_file(&backing_path);
                return Err(status);
            }
            // Cap the producer's output: a misbehaving producer must not exceed the per-transfer
            // bound, and an adversarial one (if the operator wired untrusted logic) is bounded.
            let len = std::fs::metadata(&backing_path)
                .map(|m| m.len())
                .map_err(|_| StatusCode::BadUnexpectedError)?;
            if len > self.config.max_total_bytes {
                let _ = std::fs::remove_file(&backing_path);
                return Err(StatusCode::BadInvalidArgument);
            }
        }

        let session_node_id = context.session().read().session_id().clone();
        let (file_node, _config) =
            self.create_temp_file_node(session_node_id.clone(), &file_name, false)?;

        let file_access = register_file_access_methods_full(
            &self.node_manager,
            &file_node,
            backing_path.clone(),
            self.config.max_byte_string_length,
            false,
            // Read transfer: not writable, so no per-Write cap is needed (the producer output was
            // capped above).
            None,
            self.handle_counter.clone(),
        );

        // Open the now-populated file for reading and hand back the handle. The client reads via
        // the temp FileType's standard Read/Close methods.
        let open_outputs = file_access.handle_open(context, &[Variant::from(READ_MODE)])?;
        let file_handle = match open_outputs.first() {
            Some(Variant::UInt32(h)) => *h,
            _ => {
                cleanup_transfer(
                    &Arc::downgrade(&self.address_space),
                    &file_node,
                    &backing_path,
                );
                return Err(StatusCode::BadUnexpectedError);
            }
        };

        // Register disconnect cleanup (file + nodes). Idempotent with eviction-driven Drop.
        register_session_file(
            &context.info,
            session_node_id,
            &self.address_space,
            &file_node,
            Some(backing_path.clone()),
        );

        self.transfers.insert(
            file_handle,
            Arc::new(TransferRecord {
                session_id: context.session_id(),
                kind: TransferKind::Read,
                file_node,
                backing_path,
                file_access: file_access.clone(),
                file_access_handle: file_handle,
                generate_options: options.clone(),
                address_space: Arc::downgrade(&self.address_space),
                committed: AtomicBool::new(false),
            }),
        );

        Ok(vec![
            // fileNodeId, fileHandle, completionStateMachine (null: synchronous completion).
            Variant::from(self.lookup_file_node_id(file_handle)),
            Variant::from(file_handle),
            Variant::from(NodeId::null()),
        ])
    }

    fn handle_generate_file_for_write(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let options = validate_generate_options(args, self.config.generate_options_type.as_ref())?;
        std::fs::create_dir_all(&self.config.temp_dir)
            .map_err(|_| StatusCode::BadUnexpectedError)?;

        // Same per-transfer uniquifying as the read path (see comment there): the unique number
        // keys both the path and the temp node name so concurrent write transfers in one session
        // get distinct NodeIds and handles.
        let (file_name, backing_path) = self.next_name_and_path("UploadFile");
        let session_node_id = context.session().read().session_id().clone();
        let (file_node, _config) =
            self.create_temp_file_node(session_node_id.clone(), &file_name, true)?;

        let file_access = register_file_access_methods_full(
            &self.node_manager,
            &file_node,
            backing_path.clone(),
            self.config.max_byte_string_length,
            true,
            // Per-transfer total cap, enforced on every Write -- the primary disk-exhaustion DoS
            // bound. Without it a client could grow the file without limit by issuing many small
            // individually-legal Write calls.
            Some(self.config.max_total_bytes),
            self.handle_counter.clone(),
        );

        // Open the (still-empty) file for write, creating it on disk.
        let open_outputs = file_access.handle_open(context, &[Variant::from(WRITE_MODE)])?;
        let file_handle = match open_outputs.first() {
            Some(Variant::UInt32(h)) => *h,
            _ => {
                cleanup_transfer(
                    &Arc::downgrade(&self.address_space),
                    &file_node,
                    &backing_path,
                );
                return Err(StatusCode::BadUnexpectedError);
            }
        };

        register_session_file(
            &context.info,
            session_node_id,
            &self.address_space,
            &file_node,
            Some(backing_path.clone()),
        );

        let file_node_id = file_node.file_id.clone();
        self.transfers.insert(
            file_handle,
            Arc::new(TransferRecord {
                session_id: context.session_id(),
                kind: TransferKind::Write,
                file_node,
                backing_path,
                file_access: file_access.clone(),
                file_access_handle: file_handle,
                generate_options: options,
                address_space: Arc::downgrade(&self.address_space),
                committed: AtomicBool::new(false),
            }),
        );

        Ok(vec![
            // fileNodeId, fileHandle (no completionStateMachine output for GenerateFileForWrite).
            Variant::from(file_node_id),
            Variant::from(file_handle),
        ])
    }

    fn handle_close_and_commit(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let file_handle = match args.first() {
            Some(Variant::UInt32(h)) => *h,
            Some(_) => return Err(StatusCode::BadTypeMismatch),
            None => return Err(StatusCode::BadArgumentsMissing),
        };

        let record = self
            .transfers
            .get(&file_handle)
            .ok_or(StatusCode::BadInvalidArgument)?;

        // Per §4.2 / §4.4.5: an unknown or foreign-session handle is Bad_InvalidArgument.
        if record.session_id != context.session_id() {
            return Err(StatusCode::BadInvalidArgument);
        }
        // CloseAndCommit only applies to write transfers (§4.4.5: "used to apply the content of
        // the written file"). A read-transfer handle is the wrong kind.
        if record.kind != TransferKind::Write {
            return Err(StatusCode::BadInvalidArgument);
        }

        // Closing the underlying handle first is portable (releases any OS write lock on
        // Windows); ignoring errors tolerates the case where moka already reaped an idle handle.
        let _ = record
            .file_access
            .handle_close(context, &[Variant::from(record.file_access_handle)]);

        let bytes =
            std::fs::read(&record.backing_path).map_err(|_| StatusCode::BadUnexpectedError)?;

        // Backstop re-check of the total cap (the per-Write cap is the primary bound; this catches
        // any path that grew the file outside the handler, e.g. a future SetPosition+Write edge).
        if bytes.len() as u64 > self.config.max_total_bytes {
            self.finalize_transfer(file_handle, &record);
            return Err(StatusCode::BadInvalidArgument);
        }

        if let Some(consumer) = &self.consumer {
            if let Err(status) = consumer(&bytes, &record.generate_options) {
                // The transaction still completes (temp file deleted); the consumer's status is
                // surfaced to the client.
                self.finalize_transfer(file_handle, &record);
                return Err(status);
            }
        }

        self.finalize_transfer(file_handle, &record);

        // completionStateMachine is null: synchronous completion (§4.4.6).
        Ok(vec![Variant::from(NodeId::null())])
    }

    /// Marks the transfer committed, deletes its backing file and address-space nodes, and
    /// removes it from the cache (so its `Drop` does not redo the cleanup).
    fn finalize_transfer(&self, handle: u32, record: &TransferRecord) {
        record.committed.store(true, Ordering::Release);
        cleanup_transfer(
            &record.address_space,
            &record.file_node,
            &record.backing_path,
        );
        self.transfers.invalidate(&handle);
    }

    fn lookup_file_node_id(&self, handle: u32) -> NodeId {
        self.transfers
            .get(&handle)
            .map(|r| r.file_node.file_id.clone())
            .unwrap_or_else(NodeId::null)
    }
}

/// Handler for `TemporaryFileTransferType` method calls. Construct via [`Self::register`].
pub struct TemporaryFileTransferHandler {
    inner: Arc<HandlerInner>,
}

impl TemporaryFileTransferHandler {
    /// Create the `TemporaryFileTransferType` object node (with its property and three methods)
    /// under `parent_id`, bind the method callbacks on `node_manager`, and return the handler
    /// plus the created node.
    ///
    /// `node_manager` is held by the handler so that each `GenerateFileForRead`/`GenerateFileForWrite`
    /// call can create a fresh temp `FileType` and bind its Open/Read/Write/Close callbacks at
    /// runtime (feature 106's `register_file_access_methods_full`).
    pub fn register(
        config: TemporaryFileTransferConfig,
        producer: Option<ProducerFn>,
        consumer: Option<ConsumerFn>,
        node_manager: Arc<SimpleNodeManager>,
        parent_id: Option<NodeId>,
        browse_name: &str,
    ) -> Result<(Self, TemporaryFileTransferNode), StatusCode> {
        let address_space = node_manager.address_space().clone();
        let node = {
            let address_space = opcua_core::trace_write_lock!(address_space);
            TemporaryFileTransferNode::create(&address_space, &config, parent_id, browse_name)?
        };

        let idle_timeout = config.idle_timeout;
        let inner = Arc::new(HandlerInner {
            config,
            producer,
            consumer,
            node_manager,
            address_space: address_space.clone(),
            transfers: moka::sync::Cache::builder()
                .time_to_idle(idle_timeout)
                .build(),
            handle_counter: Arc::new(AtomicU32::new(1)),
            file_name_counter: AtomicU64::new(0),
        });

        let handler = Self {
            inner: inner.clone(),
        };
        handler.bind_methods(&node);
        Ok((handler, node))
    }

    fn bind_methods(&self, node: &TemporaryFileTransferNode) {
        let inner = self.inner.clone();
        self.inner
            .node_manager
            .inner()
            .add_method_callback_with_context(
                node.generate_file_for_read_id.clone(),
                move |ctx, args| inner.handle_generate_file_for_read(ctx, args),
            );
        let inner = self.inner.clone();
        self.inner
            .node_manager
            .inner()
            .add_method_callback_with_context(
                node.generate_file_for_write_id.clone(),
                move |ctx, args| inner.handle_generate_file_for_write(ctx, args),
            );
        let inner = self.inner.clone();
        self.inner
            .node_manager
            .inner()
            .add_method_callback_with_context(
                node.close_and_commit_id.clone(),
                move |ctx, args| inner.handle_close_and_commit(ctx, args),
            );
    }
}

// OpenFileMode bits (OPC-10000-20 §4.2.2), re-declared locally to match file_access::open_mode.
const READ_MODE: u8 = 1;
const WRITE_MODE: u8 = 2 | 4; // Write | EraseExisting: a fresh temp file is created empty.

/// Validates the `generateOptions` argument (args[0]) against the server-declared DataType.
///
/// An empty/absent `generateOptions` is always accepted (the parameter is optional). When
/// `expected` is `None`, any non-empty value is accepted (no type-check). When `expected` is
/// `Some(_)`, a value whose DataType does not match is rejected with `Bad_TypeMismatch`. This
/// never panics on a downcast -- it inspects the `Variant`'s declared type via
/// [`Variant::data_type`].
fn validate_generate_options(
    args: &[Variant],
    expected: Option<&NodeId>,
) -> Result<Variant, StatusCode> {
    let options = args.first().cloned().unwrap_or(Variant::Empty);
    if matches!(options, Variant::Empty) {
        return Ok(Variant::Empty);
    }
    let Some(expected) = expected else {
        return Ok(options);
    };
    let actual = options.data_type().ok_or(StatusCode::BadTypeMismatch)?;
    if &actual.node_id != expected {
        return Err(StatusCode::BadTypeMismatch);
    }
    Ok(options)
}

fn insert_property(
    address_space: &AddressSpace,
    parent_id: &NodeId,
    node_id: &NodeId,
    name: &str,
    data_type: DataTypeId,
    value: impl Into<Variant>,
) -> Result<(), StatusCode> {
    if !VariableBuilder::new(node_id, name, name)
        .property_of(parent_id.clone())
        .has_type_definition(VariableTypeId::PropertyType)
        .data_type(data_type)
        .value(value)
        .insert(address_space)
    {
        return Err(StatusCode::BadNodeIdExists);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_method(
    address_space: &AddressSpace,
    parent_id: &NodeId,
    node_id: &NodeId,
    name: &str,
    input_args: &[Argument],
    output_args: &[Argument],
) -> Result<(), StatusCode> {
    let input_args_id = NodeId::new(node_id.namespace, format!("{node_id}_InputArguments"));
    let output_args_id = NodeId::new(node_id.namespace, format!("{node_id}_OutputArguments"));
    let mut builder = MethodBuilder::new(node_id, name, name).component_of(parent_id.clone());
    if !input_args.is_empty() {
        builder = builder.input_args(address_space, &input_args_id, input_args);
    }
    if !output_args.is_empty() {
        builder = builder.output_args(address_space, &output_args_id, output_args);
    }
    if !builder.insert(address_space) {
        return Err(StatusCode::BadNodeIdExists);
    }
    Ok(())
}

fn argument(name: &str, data_type: DataTypeId) -> Argument {
    Argument {
        name: name.into(),
        data_type: data_type.into(),
        value_rank: -1,
        array_dimensions: None,
        description: LocalizedText::null(),
    }
}

/// Like [`argument`] but takes a raw `NodeId` for the DataType — used for operator-configured
/// generateOptions types (Part 20 §4.4.3/§4.4.4: the Server SHALL specify a concrete DataType in
/// the Argument when it expects non-Null options).
fn argument_with_type(name: &str, data_type: NodeId) -> Argument {
    Argument {
        name: name.into(),
        data_type,
        value_rank: -1,
        array_dimensions: None,
        description: LocalizedText::null(),
    }
}

fn sanitize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "tft".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests;
