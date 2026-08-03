//! Tests for `TemporaryFileTransferType`. Covers the three methods, the generateOptions
//! type-check, the per-transfer size cap, session-scoping of handles, and cleanup of temp
//! files/nodes on commit.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use opcua_core::sync::RwLock as CoreRwLock;
use opcua_types::{
    AnonymousIdentityToken, ApplicationDescription, ByteString, DataTypeId, MessageSecurityMode,
    NodeId, StatusCode, UAString, Variant,
};

use crate::{
    address_space::AddressSpace,
    authenticator::UserToken,
    fota::temporary_transfer::{TemporaryFileTransferConfig, TemporaryFileTransferHandler},
    identity_token::IdentityToken,
    node_manager::{
        memory::{InMemoryNodeManager, SimpleNodeManager, SimpleNodeManagerImpl},
        NodeManagersRef, RequestContext, RequestContextInner,
    },
    session::instance::Session,
    ServerBuilder,
};

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = tempfile::Builder::new()
            .prefix(&format!("async-opcua-tft-test-{name}-"))
            .tempdir()
            .expect("failed to create a securely-permissioned test scratch directory")
            .keep();
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn make_node_manager() -> Arc<SimpleNodeManager> {
    let address_space = AddressSpace::new();
    let inner = SimpleNodeManagerImpl::new(
        Vec::new(),
        "temporary-transfer-test",
        NodeManagersRef::new_empty(),
    );
    Arc::new(InMemoryNodeManager::new(inner, address_space))
}

fn test_context(session_id: u32) -> (RequestContext, crate::ServerHandle) {
    let (_server, handle) = ServerBuilder::new_anonymous("tft-test")
        .without_node_managers()
        .create_sample_keypair(true)
        .build()
        .expect("test server should build");
    let info = Arc::clone(handle.info());
    let session = Arc::new(CoreRwLock::new(Session::create(
        &info,
        NodeId::new(0, session_id),
        1,
        60_000,
        0,
        0,
        UAString::from("opc.tcp://localhost"),
        opcua_crypto::SecurityPolicy::None.to_uri().to_string(),
        IdentityToken::Anonymous(AnonymousIdentityToken {
            policy_id: UAString::from("anonymous"),
        }),
        None,
        ByteString::null(),
        UAString::from("tft-test"),
        ApplicationDescription::default(),
        MessageSecurityMode::None,
    )));

    let context = RequestContext::new_test(Arc::new(RequestContextInner {
        session,
        session_id,
        authenticator: info.authenticator.clone(),
        token: UserToken("tft-test".to_string()),
        user_roles: Arc::new(Vec::new()),
        type_tree: info.type_tree.clone(),
        type_tree_getter: info.type_tree_getter.clone(),
        subscriptions: handle.subscriptions().clone(),
        info,
    }));
    (context, handle)
}

fn basic_config(dir: &TempDir) -> TemporaryFileTransferConfig {
    TemporaryFileTransferConfig::new(2, "urn:async-opcua:tft-test", dir.path().to_path_buf())
}

/// Extracts `(fileNodeId, fileHandle)` from a GenerateFileForRead result, asserting the
/// completion state machine is null (synchronous completion).
fn read_outputs(out: &[Variant]) -> (NodeId, u32) {
    assert_eq!(out.len(), 3, "GenerateFileForRead returns 3 outputs");
    let file_node_id = match &out[0] {
        Variant::NodeId(n) => (**n).clone(),
        other => panic!("expected NodeId fileNodeId, got {other:?}"),
    };
    let file_handle = match &out[1] {
        Variant::UInt32(h) => *h,
        other => panic!("expected UInt32 fileHandle, got {other:?}"),
    };
    assert_eq!(out[2], Variant::from(NodeId::null()));
    (file_node_id, file_handle)
}

fn write_outputs(out: &[Variant]) -> (NodeId, u32) {
    assert_eq!(out.len(), 2, "GenerateFileForWrite returns 2 outputs");
    let file_node_id = match &out[0] {
        Variant::NodeId(n) => (**n).clone(),
        other => panic!("expected NodeId fileNodeId, got {other:?}"),
    };
    let file_handle = match &out[1] {
        Variant::UInt32(h) => *h,
        other => panic!("expected UInt32 fileHandle, got {other:?}"),
    };
    (file_node_id, file_handle)
}

#[tokio::test]
async fn generate_file_for_read_runs_producer_and_returns_open_read_handle() {
    let dir = TempDir::new("gfr-basic");
    let (context, _handle) = test_context(1);
    let mut config = basic_config(&dir);
    config.max_byte_string_length = 1024;
    let producer: crate::fota::temporary_transfer::ProducerFn = Arc::new(|path, _opts| {
        std::fs::write(path, b"generated-content").map_err(|_| StatusCode::BadUnexpectedError)
    });
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, Some(producer), None, nm, None, "TFT")
            .expect("register should succeed");

    let out = handler
        .inner
        .handle_generate_file_for_read(&context, &[Variant::Empty])
        .expect("GenerateFileForRead should succeed");
    let (file_node_id, file_handle) = read_outputs(&out);

    // The temp FileType node must exist in the address space.
    assert!(handler
        .inner
        .address_space
        .read()
        .find(&file_node_id)
        .is_some());

    // The producer content is readable through the returned handle via the file access handler.
    let record = handler
        .inner
        .transfers
        .get(&file_handle)
        .expect("transfer recorded");
    let read = record
        .file_access
        .handle_read(
            &context,
            &[Variant::from(file_handle), Variant::from(1024_i32)],
        )
        .expect("read should succeed");
    match &read[0] {
        Variant::ByteString(bs) => {
            assert_eq!(
                bs.value.as_ref().map(|b| b.to_vec()),
                Some(b"generated-content".to_vec())
            );
        }
        other => panic!("expected ByteString, got {other:?}"),
    }
}

#[tokio::test]
async fn generate_file_for_write_returns_open_write_handle_then_close_and_commit_consumes() {
    let dir = TempDir::new("gfw-commit");
    let (context, _handle) = test_context(1);
    let mut config = basic_config(&dir);
    config.max_byte_string_length = 1024;
    config.max_total_bytes = 4096;

    let committed = Arc::new(AtomicUsize::new(0));
    let committed_bytes = committed.clone();
    let consumer: crate::fota::temporary_transfer::ConsumerFn = Arc::new(move |bytes, _opts| {
        committed_bytes.store(bytes.len(), Ordering::SeqCst);
        assert_eq!(bytes, b"uploaded-via-write");
        Ok(())
    });
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, None, Some(consumer), nm, None, "TFT")
            .expect("register should succeed");

    let out = handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::Empty])
        .expect("GenerateFileForWrite should succeed");
    let (file_node_id, file_handle) = write_outputs(&out);

    // Client writes via the temp FileType's standard Write method (feature 106).
    let record = handler
        .inner
        .transfers
        .get(&file_handle)
        .expect("transfer recorded");
    record
        .file_access
        .handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(ByteString::from(b"uploaded-via-write".to_vec())),
            ],
        )
        .expect("write should succeed");

    let commit = handler
        .inner
        .handle_close_and_commit(&context, &[Variant::from(file_handle)])
        .expect("CloseAndCommit should succeed");
    assert_eq!(commit.len(), 1);
    assert_eq!(commit[0], Variant::from(NodeId::null()));

    assert_eq!(
        committed.load(Ordering::SeqCst),
        b"uploaded-via-write".len()
    );

    // Commit deletes the temp file node.
    assert!(handler
        .inner
        .address_space
        .read()
        .find(&file_node_id)
        .is_none());
    // And removes the transfer record.
    assert!(handler.inner.transfers.get(&file_handle).is_none());
}

#[tokio::test]
async fn close_and_commit_on_unknown_handle_is_bad_invalid_argument() {
    let dir = TempDir::new("commit-unknown");
    let (context, _handle) = test_context(1);
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(basic_config(&dir), None, None, nm, None, "TFT")
            .expect("register should succeed");

    let err = handler
        .inner
        .handle_close_and_commit(&context, &[Variant::from(999_u32)])
        .expect_err("unknown handle should fail");
    assert_eq!(err, StatusCode::BadInvalidArgument);
}

#[tokio::test]
async fn close_and_commit_on_a_read_transfer_is_rejected() {
    let dir = TempDir::new("commit-read");
    let (context, _handle) = test_context(1);
    let producer: crate::fota::temporary_transfer::ProducerFn =
        Arc::new(|path, _| std::fs::write(path, b"x").map_err(|_| StatusCode::BadUnexpectedError));
    let nm = make_node_manager();
    let (handler, _node) = TemporaryFileTransferHandler::register(
        basic_config(&dir),
        Some(producer),
        None,
        nm,
        None,
        "TFT",
    )
    .expect("register should succeed");

    let out = handler
        .inner
        .handle_generate_file_for_read(&context, &[Variant::Empty])
        .expect("generate should succeed");
    let (_, file_handle) = read_outputs(&out);

    let err = handler
        .inner
        .handle_close_and_commit(&context, &[Variant::from(file_handle)])
        .expect_err("CloseAndCommit on a read transfer must fail");
    assert_eq!(err, StatusCode::BadInvalidArgument);
}

#[tokio::test]
async fn handle_from_a_different_session_is_rejected() {
    let dir = TempDir::new("cross-session");
    let (ctx_a, _handle_a) = test_context(1);
    let (ctx_b, _handle_b) = test_context(2);
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(basic_config(&dir), None, None, nm, None, "TFT")
            .expect("register should succeed");

    let out = handler
        .inner
        .handle_generate_file_for_write(&ctx_a, &[Variant::Empty])
        .expect("generate should succeed for session A");
    let (_, file_handle) = write_outputs(&out);

    let err = handler
        .inner
        .handle_close_and_commit(&ctx_b, &[Variant::from(file_handle)])
        .expect_err("a different session's handle must be rejected");
    assert_eq!(err, StatusCode::BadInvalidArgument);
}

#[tokio::test]
async fn generate_options_type_mismatch_is_rejected() {
    let dir = TempDir::new("opts-mismatch");
    let (context, _handle) = test_context(1);
    let mut config = basic_config(&dir);
    // Server declares generateOptions must be a String.
    config.generate_options_type = Some(NodeId::from(DataTypeId::String));
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, None, None, nm, None, "TFT")
            .expect("register should succeed");

    // Wrong type (UInt32) -> Bad_TypeMismatch. Must NOT panic.
    let err = handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::from(7_u32)])
        .expect_err("mismatched generateOptions type must be rejected");
    assert_eq!(err, StatusCode::BadTypeMismatch);

    // Correct type (String) -> succeeds.
    handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::from(UAString::from("ok"))])
        .expect("matching generateOptions type should succeed");

    // Empty/absent generateOptions is always accepted (parameter is optional).
    handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::Empty])
        .expect("empty generateOptions should be accepted");
}

#[tokio::test]
async fn generate_options_not_checked_when_no_type_declared() {
    let dir = TempDir::new("opts-any");
    let (context, _handle) = test_context(1);
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(basic_config(&dir), None, None, nm, None, "TFT")
            .expect("register should succeed");

    // No type declared: any non-empty generateOptions is accepted.
    handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::from(42_u32)])
        .expect("any generateOptions accepted when no type declared");
}

#[tokio::test]
async fn per_transfer_write_cap_is_enforced_during_write() {
    let dir = TempDir::new("write-cap");
    let (context, _handle) = test_context(1);
    let mut config = basic_config(&dir);
    config.max_byte_string_length = 1024;
    config.max_total_bytes = 10; // tiny total cap

    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, None, None, nm, None, "TFT")
            .expect("register should succeed");

    let out = handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::Empty])
        .expect("generate should succeed");
    let (_, file_handle) = write_outputs(&out);
    let record = handler
        .inner
        .transfers
        .get(&file_handle)
        .expect("transfer recorded");

    // First 10 bytes are within the cap.
    record
        .file_access
        .handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(ByteString::from(vec![0u8; 10])),
            ],
        )
        .expect("write within cap should succeed");

    // An 11th byte exceeds the per-transfer total cap.
    let err = record
        .file_access
        .handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(ByteString::from(vec![0u8; 1])),
            ],
        )
        .expect_err("write exceeding total cap must be rejected");
    assert_eq!(err, StatusCode::BadInvalidArgument);
}

#[tokio::test]
async fn producer_output_exceeding_cap_is_rejected() {
    let dir = TempDir::new("producer-cap");
    let (context, _handle) = test_context(1);
    let mut config = basic_config(&dir);
    config.max_total_bytes = 4;
    let producer: crate::fota::temporary_transfer::ProducerFn = Arc::new(|path, _| {
        std::fs::write(path, b"way-too-long-for-the-cap")
            .map_err(|_| StatusCode::BadUnexpectedError)
    });
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, Some(producer), None, nm, None, "TFT")
            .expect("register should succeed");

    let err = handler
        .inner
        .handle_generate_file_for_read(&context, &[Variant::Empty])
        .expect_err("producer output exceeding cap must fail GenerateFileForRead");
    assert_eq!(err, StatusCode::BadInvalidArgument);
}

#[tokio::test]
async fn producer_failure_surfaces_its_status_and_creates_no_node() {
    let dir = TempDir::new("producer-fail");
    let (context, _handle) = test_context(1);
    let producer: crate::fota::temporary_transfer::ProducerFn =
        Arc::new(|_, _| Err(StatusCode::BadOutOfMemory));
    let nm = make_node_manager();
    let (handler, _node) = TemporaryFileTransferHandler::register(
        basic_config(&dir),
        Some(producer),
        None,
        nm,
        None,
        "TFT",
    )
    .expect("register should succeed");

    let err = handler
        .inner
        .handle_generate_file_for_read(&context, &[Variant::Empty])
        .expect_err("producer failure should surface");
    assert_eq!(err, StatusCode::BadOutOfMemory);
}

#[tokio::test]
async fn consumer_failure_surfaces_its_status_but_still_deletes_temp_file() {
    let dir = TempDir::new("consumer-fail");
    let (context, _handle) = test_context(1);
    let consumer: crate::fota::temporary_transfer::ConsumerFn =
        Arc::new(|_, _| Err(StatusCode::BadNotSupported));
    let nm = make_node_manager();
    let mut config = basic_config(&dir);
    config.max_byte_string_length = 1024;
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, None, Some(consumer), nm, None, "TFT")
            .expect("register should succeed");

    let out = handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::Empty])
        .expect("generate should succeed");
    let (file_node_id, file_handle) = write_outputs(&out);
    let record = handler
        .inner
        .transfers
        .get(&file_handle)
        .expect("transfer recorded");
    record
        .file_access
        .handle_write(
            &context,
            &[
                Variant::from(file_handle),
                Variant::from(ByteString::from(b"data".to_vec())),
            ],
        )
        .expect("write should succeed");

    let err = handler
        .inner
        .handle_close_and_commit(&context, &[Variant::from(file_handle)])
        .expect_err("consumer failure should surface");
    assert_eq!(err, StatusCode::BadNotSupported);
    // Transaction still completes: node + record gone.
    assert!(handler
        .inner
        .address_space
        .read()
        .find(&file_node_id)
        .is_none());
    assert!(handler.inner.transfers.get(&file_handle).is_none());
}

#[tokio::test]
async fn transfer_object_node_has_standard_methods_and_property() {
    let dir = TempDir::new("node-shape");
    let (_context, _handle) = test_context(1);
    let nm = make_node_manager();
    let (_handler, node) = TemporaryFileTransferHandler::register(
        basic_config(&dir),
        None,
        None,
        nm.clone(),
        None,
        "TFT",
    )
    .expect("register should succeed");

    let as_ = nm.address_space().read();
    assert!(as_.find(&node.transfer_object_id).is_some());
    assert!(as_.find(&node.client_processing_timeout_id).is_some());
    assert!(as_.find(&node.generate_file_for_read_id).is_some());
    assert!(as_.find(&node.generate_file_for_write_id).is_some());
    assert!(as_.find(&node.close_and_commit_id).is_some());
}

#[tokio::test]
async fn two_concurrent_write_transfers_get_distinct_globally_unique_handles() {
    let dir = TempDir::new("unique-handles");
    let (context, _handle) = test_context(1);
    let mut config = basic_config(&dir);
    config.max_byte_string_length = 1024;
    let nm = make_node_manager();
    let (handler, _node) =
        TemporaryFileTransferHandler::register(config, None, None, nm, None, "TFT")
            .expect("register should succeed");

    let out1 = handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::Empty])
        .expect("first generate should succeed");
    let (_, h1) = write_outputs(&out1);

    let out2 = handler
        .inner
        .handle_generate_file_for_write(&context, &[Variant::Empty])
        .expect("second generate should succeed");
    let (_, h2) = write_outputs(&out2);

    assert_ne!(h1, h2, "concurrent transfers must get distinct handles");

    // CloseAndCommit resolves each handle to the right transfer (no aliasing).
    handler
        .inner
        .handle_close_and_commit(&context, &[Variant::from(h1)])
        .expect("commit h1");
    handler
        .inner
        .handle_close_and_commit(&context, &[Variant::from(h2)])
        .expect("commit h2");
}
