# Research: File Access Real I/O (FileType Open/Read/Write/Close)

## Spec grounding (OPC-10000-20, local PDF `~/opcua-specs`, §4.2 -- re-extracted this session,
not assumed)

`FileType` itself is defined in **Part 20** (File Transfer), not Part 5 as the name might
suggest -- Part 5 only has a subtype (`AddressSpaceFileType`, §6.3.12). Confirmed via
`pdftotext -layout` on the local PDF.

### Open (§4.2.2)

`Open(mode: Byte) -> fileHandle: UInt32`. Mode is a bitmask (`OpenFileMode`,
`async-opcua-types::generated::types::enums::OpenFileMode`, already generated: `Read=1`,
`Write=2`, `EraseExisting=4`, `Append=8`; bits 4-7 reserved, must be zero -- reject with
`Bad_InvalidArgument` if set).

- Clients can open the same file multiple times **for read**.
- A second `Open` for **write** while already open for write -> `Bad_NotWritable`.
- A second `Open` for **read** while already open for **write** -> `Bad_NotReadable`.
- `fileHandle` is unique per Session; never transferable to another Session.
- Status codes: `Bad_NotReadable`, `Bad_NotWritable`, `Bad_InvalidState`, `Bad_InvalidArgument`
  (bad mode), `Bad_NotFound` (file doesn't exist and wasn't opened for write), `Bad_UnexpectedError`.

### Close (§4.2.3)

`Close(fileHandle: UInt32)`. Invalidates the handle. `Bad_InvalidArgument` for an unknown handle.

### Read (§4.2.4)

`Read(fileHandle: UInt32, length: Int32) -> data: ByteString`. Reads from the current position,
advances position by bytes actually returned. **Server is allowed to return less than
`length`** (never an error for that alone). **Empty `ByteString` means end-of-file** (not an
error). `length` must be positive. Status codes: `Bad_InvalidArgument` (bad handle or
non-positive length), `Bad_UnexpectedError`, `Bad_InvalidState` (handle not opened for read).

### Write (§4.2.5)

`Write(fileHandle: UInt32, data: ByteString)`. Writes at current position, advances position by
bytes written. **Writing empty/null data returns `Good` with no effect** (not an error). Status
codes: `Bad_InvalidArgument` (bad handle), `Bad_NotWritable` (locked by another writer),
`Bad_InvalidState` (handle not opened for write).

### GetPosition/SetPosition (§4.2.6/§4.2.7)

Plain position accessors on the handle. `SetPosition` beyond end-of-file **clamps to
end-of-file**, does not error. Only `Bad_InvalidArgument` (bad handle) for either.

### FileType properties (§4.2.1)

`Size: UInt64` (Mandatory -- `Bad_NotSupported` on read if the server can't determine it
accurately, e.g. mid-write; this feature always can, since it's backed by a real
`std::fs::File`), `Writable`/`UserWritable: Boolean` (Mandatory), `OpenCount: UInt16`
(Mandatory -- must reflect live handle count), `MimeType: String` (Optional),
`MaxByteStringLength: UInt32` (Optional -- caps Read/Write buffer size; if absent, the server's
own `ServerCapabilitiesType.MaxByteStringLength` default applies per spec, but `fota/file_node.rs`
already always sets a concrete value, so a fallback path isn't needed for this feature's one real
consumer), `LastModifiedTime: DateTime` (Optional).

## Reusable pattern: `TrustListHandleRegistry` (`gds/trust_list/mod.rs:69-124`)

Directly re-read this session (not assumed from memory). Shape: `moka::sync::Cache<u32,
Arc<Mutex<State>>>` keyed by handle, `AtomicU32` handle counter (`insert` loops on collision),
`get(handle, session_id)` checks `owning_session_id` match (`Bad_InvalidState` otherwise --
**this feature uses `Bad_InvalidArgument` instead**, since that's what §4.2's tables actually
specify for every FileType method, not `Bad_InvalidState`; TrustList's choice was a
Part-12-specific TrustList convention, not the base FileType spec this feature implements), and
`remove(handle)` (`invalidate`). Registry uses `time_to_idle` (not `time_to_live`) so an
abandoned handle self-expires without an explicit session-disconnect hook -- **this feature reuses
that exact mechanism** for FR-009 (handles released even if the client never calls `Close`):
dropping the `moka` entry drops the held `std::fs::File`, closing the OS file descriptor, and a
different session can never satisfy the `owning_session_id` check even before expiry. No changes
to `fota::cleanup` are needed.

**Difference from TrustList**: TrustList's `buffer: Vec<u8>` is in-memory (appropriate for a
small certificate list). This feature's files can be large (firmware images) -- FR-008's
`MaxByteStringLength` bound is about *per-call* buffer size, not total file size, so the handle
state holds a real `std::fs::File` (OS-buffered, not loaded fully into process memory) plus the
open mode, not a `Vec<u8>`.

## Registration target: `SimpleNodeManager`, not `CoreNodeManager`

`fota::file_node::TemporaryFileNode` creates nodes in an **operator-chosen custom namespace**
(default `urn:async-opcua:fota`), structurally identical to any third-party server's own
application namespace -- not a companion-spec or core namespace-0 concern. This is exactly what
`SimpleNodeManager` (`node_manager::memory::SimpleNodeManager`) exists for (confirmed:
`add_method_callback_with_context` exists on it via the same generic
`InMemoryNodeManager<TImpl>` this project's other method-registration features already use).
`register_file_access_methods` therefore takes `&SimpleNodeManager`, not `&CoreNodeManager`
(the opposite of every GDS feature, which correctly used `CoreNodeManager` because those methods
live on namespace-0/companion-namespace nodes).

## Security design (Constitution Principle IV, explicit per plan.md's Constitution Check)

- **No path-traversal surface**: the backing filesystem path is fixed by the *operator* at
  `TemporaryFileNode`/registration time (a `PathBuf` parameter this feature adds), never derived
  from any client-supplied string. No `Open`/`Read`/`Write`/`Close` argument is ever interpreted
  as a path.
- **Bounded `Read` length**: `length` is clamped to `min(requested, MaxByteStringLength)` before
  any allocation -- a client requesting `i32::MAX` bytes cannot force a proportional allocation.
- **Bounded `Write` payload**: `data.len() > MaxByteStringLength` -> `Bad_InvalidArgument`,
  rejected before the write syscall, not truncated silently (silent truncation would corrupt the
  client's intended byte sequence without any signal).
- **No panics on adversarial input**: `Position` (`u64`) arithmetic for `SetPosition`/read-write
  advancement uses checked/saturating operations, never a raw cast or unchecked `as` that could
  wrap; `length <= 0` is rejected (`Bad_InvalidArgument`) before any use as a buffer size.
- **Handle scoping**: `owning_session_id` check (already proven by TrustList) prevents any
  cross-session handle reuse, including a maliciously guessed/enumerated handle number from a
  different session.

## Alternatives considered

- *In-memory `Vec<u8>` buffer (matching TrustList exactly)*: rejected -- firmware images can be
  tens of megabytes; buffering entire file contents per open handle in process memory is an
  unnecessary and unbounded-by-request-count memory-exhaustion risk this feature can trivially
  avoid by using real `std::fs::File` I/O instead, which is also more honestly "real" per spec.md's
  own framing (a temp buffer that evaporates isn't durable in the sense FOTA's use case needs).
- *A new, generic `add_method_cb`-based dispatch instead of `add_method_callback_with_context`*:
  rejected -- the latter is the established, already-proven pattern (GDS features 101-105,
  TrustList) and provides `RequestContext` access (needed for `context.session_id`), which the
  plain `add_method_cb` seen only in test scaffolding does not obviously provide in the same form.
