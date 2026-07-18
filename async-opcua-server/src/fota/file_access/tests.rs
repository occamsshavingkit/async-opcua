use std::path::{Path, PathBuf};

use opcua_core::sync::RwLock as CoreRwLock;
use opcua_crypto::SecurityPolicy;
use opcua_types::{AnonymousIdentityToken, ApplicationDescription, MessageSecurityMode, UAString};

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
