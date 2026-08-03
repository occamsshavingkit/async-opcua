---

description: "Task list for feature 110: TemporaryFileTransferType (on-demand temp file generation)"
---

# Tasks: TemporaryFileTransferType

**Input**: Design documents from `/specs/110-temporary-file-transfer/`
**Prerequisites**: plan.md, spec.md

**Tests**: Included, per this repo's constitution (Principle I). Written FIRST (TDD); the
implementation was iterated against them until green.

**Organization**: Single user story (P1).

## Path Conventions

New `async-opcua-server/src/fota/temporary_transfer.rs` (+ its `tests.rs` submodule). Additive
changes to `async-opcua-server/src/fota/mod.rs`, `async-opcua-server/src/fota/file_access/mod.rs`,
`async-opcua-server/src/fota/file_access/tests.rs`.

---

## Phase 1: Foundation -- extend feature 106 with shared counter + per-transfer cap

- [X] T001 [P] Cite OPC-10000-20 §4.4.1-§4.4.6 (TemporaryFileTransferType: the three methods,
      their Argument signatures, the `ClientProcessingTimeout` property, and the synchronous-
      completion rule for `completionStateMachine`) and the relevant CUs 3810/3811/3812/3813/5791
      before writing any code. (Grounded via the OPC UA reference MCP.)
- [X] T002 [P] In `fota/file_access/mod.rs`, replace `FotaFileHandleRegistry::next_handle:
      AtomicU32` with `Arc<AtomicU32>` and add `with_counter(idle_timeout, counter)` (remove the
      old `new()`; all callers delegate through `with_counter`). This lets a
      `TemporaryFileTransferHandler` share one globally-unique handle counter across every
      per-transfer file-access handler it spawns.
- [X] T003 In `fota/file_access/mod.rs`, add `max_file_size: Option<u64>` to
      `FotaFileAccessHandler`, plumbed via `new_full(...)` and
      `register_file_access_methods_full(...)` (remove the old `new()`; all callers delegate
      through `new_full`). Enforce inside `handle_write`: if `position.checked_add(len) >
      max_file_size`, return `BadInvalidArgument` (no overflow: use `checked_add`). Update the
      3 in-crate `FotaFileAccessHandler::new(` call sites in `fota/file_access/tests.rs` to
      `new_full(` with `, None, Arc::new(AtomicU32::new(1))`.

**Checkpoint**: feature 106 still builds and its existing tests still pass; the shared-counter
and per-transfer cap machinery is in place but unused by the new module yet.

---

## Phase 2: The TemporaryFileTransferType module (TDD -- tests first)

- [X] T004 [P] Write the test submodule `fota/temporary_transfer/tests.rs` FIRST (13 tests,
      mirroring `fota/file_access/tests.rs`'s `test_context(session_id)` helper). Cover: read
      producer happy path; write + commit happy path; unknown handle -> `BadInvalidArgument`;
      commit on a read transfer -> `BadInvalidArgument`; cross-session handle -> rejected;
      `generateOptions` type mismatch -> `Bad_TypeMismatch` (and *not* a panic); no-type-declared
      accepts any `generateOptions`; per-transfer write cap enforced during `Write`; producer
      output exceeding cap -> `BadInvalidArgument`; producer failure surfaces its status and
      creates no node; consumer failure surfaces its status but still deletes the temp file;
      the transfer object node exposes the 3 standard methods + `ClientProcessingTimeout`
      property; two concurrent write transfers get distinct globally-unique handles.
- [X] T005 Create `fota/temporary_transfer.rs`: `TemporaryFileTransferConfig` (with `new(ns, uri,
      temp_dir)` and the per-transfer `max_total_bytes`, `max_byte_string_length`,
      `idle_timeout`, `client_processing_timeout_ms`, optional `generate_options_type` fields);
      `pub type ProducerFn = Arc<dyn Fn(&Path, &Variant) -> Result<(), StatusCode> + Send + Sync>`;
      `pub type ConsumerFn = Arc<dyn Fn(&[u8], &Variant) -> Result<(), StatusCode> + Send + Sync>`;
      `TemporaryFileTransferNode::create` building the object (typed
      `ObjectTypeId::TemporaryFileTransferType`), the `ClientProcessingTimeout` property
      (§4.4.1), and the three methods with their real `Argument` signatures (§4.4.3-§4.4.5).
- [X] T006 `TemporaryFileTransferHandler::register(config, producer, consumer, node_manager,
      parent_id, browse_name)`: extract `idle_timeout` then move `config` into `HandlerInner`;
      `transfers = moka::sync::Cache::builder().time_to_live/idle(idle_timeout).build()`;
      `handle_counter = Arc::new(AtomicU32::new(1))`; register the three method callbacks via
      `add_method_callback_with_context`, each delegating to the matching `handle_*` inner.
- [X] T007 `handle_generate_file_for_read`: validate `generateOptions`, `create_dir_all(temp_dir)`,
      pick a unique name+path (`next_name_and_path`, ONE counter value keys both so the temp node
      NodeId and the on-disk file share an identity), run the producer (cleanup partial file on
      err), re-check producer output size <= `max_total_bytes`, create the temp `FileType` node
      (writable=false) via feature 106, `register_file_access_methods_full(... None,
      handle_counter)` (read transfer needs no per-Write cap -- producer output already capped),
      `handle_open(READ_MODE=1)`, `register_session_file` (disconnect cleanup), store a
      `TransferRecord`, return `[fileNodeId, fileHandle, NodeId::null()]`.
- [X] T008 `handle_generate_file_for_write`: validate `generateOptions`, `create_dir_all`,
      unique name+path, create the temp `FileType` node (writable=true),
      `register_file_access_methods_full(... Some(max_total_bytes), handle_counter)` (this is the
      primary disk-exhaustion DoS bound -- enforced on EVERY `Write`), `handle_open(WRITE_MODE=6
      = 2|4)`, `register_session_file`, store `TransferRecord`, return `[fileNodeId, fileHandle]`
      (only 2 outputs per §4.4.4).
- [X] T009 `handle_close_and_commit`: lookup handle (`BadInvalidArgument` if missing/expired),
      session check, kind==Write check, `handle_close` (ignore close errors), `fs::read` the
      committed bytes, re-check size <= `max_total_bytes`, invoke the consumer (surface its
      status), `finalize_transfer` (mark committed, cleanup file + node, invalidate cache),
      return `[NodeId::null()]`.
- [X] T010 `validate_generate_options(args, expected: Option<&NodeId>)`: empty/absent Variant ->
      Ok; `None` expected -> Ok(any non-empty); else compare `options.data_type().node_id` to
      `expected`, returning `Bad_TypeMismatch` on mismatch (NO panic -- verified by T004's
      mismatch test).
- [X] T011 `TransferRecord` with a `Drop` impl that calls `cleanup_transfer` when not committed
      (covers moka idle-timeout eviction of abandoned transfers); `next_name_and_path(prefix)`
      consuming the shared `file_name_counter` so concurrent transfers in one session never
      collide on `FOTA_{session}_{file_name}` NodeIds.
- [X] T012 Add `pub mod temporary_transfer;` to `fota/mod.rs`.

**Checkpoint**: all 13 tests green; clippy clean; fmt clean.

---

## Phase 3: Verification

- [X] T013 `cargo test   -p async-opcua-server fota::temporary_transfer --all-features`  -> 13/13.
- [X] T014 `cargo clippy -p async-opcua-server --all-targets --all-features -- -Dwarnings` -> clean.
- [X] T015 `cargo fmt    --all -- --check` -> clean.
