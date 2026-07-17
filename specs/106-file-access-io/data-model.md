# Data Model: File Access Real I/O (FileType Open/Read/Write/Close)

## New: `async-opcua-server/src/fota/file_access.rs`

### `FotaFileHandleState`

```rust
struct FotaFileHandleState {
    owning_session_id: u32,
    mode: HandleMode,      // Read | Write (derived from the Open mode byte)
    file: std::fs::File,   // real, OS-buffered backing file
    position: u64,
}
```

### `FotaFileHandleRegistry`

Directly modeled on `gds/trust_list/mod.rs`'s `TrustListHandleRegistry`:

```rust
struct FotaFileHandleRegistry {
    handles: moka::sync::Cache<u32, Arc<Mutex<FotaFileHandleState>>>,
    next_handle: AtomicU32,
}
```

- `insert(state) -> u32`: loop-allocate a non-colliding handle id (same as TrustList).
- `get(handle, session_id) -> Result<Arc<Mutex<FotaFileHandleState>>, StatusCode>`: `Bad_InvalidArgument` (not `Bad_InvalidState` -- see research.md) if missing or `owning_session_id` mismatch.
- `remove(handle)`: `invalidate`.
- Built with `time_to_idle` (a configurable per-handler duration; default matches TrustList's 60s ActivityTimeout precedent unless the caller overrides).

### `FotaFileAccessHandler`

```rust
pub struct FotaFileAccessHandler {
    handles: FotaFileHandleRegistry,
    backing_path: PathBuf,
    max_byte_string_length: u32,
    open_count: Arc<AtomicU16>,      // mirrors the live OpenCount property value
    node_ids: FotaFileAccessNodeIds, // for updating OpenCount's Variable value on Open/Close
}
```

Holds exactly one backing path (one handler per `FileType` instance -- matches
`TemporaryFileNode` being a single-file-per-instance builder). A future feature wiring a second,
differently-purposed `FileType` instance would construct a second `FotaFileAccessHandler`, not
extend this one to be multi-file.

## Extended: `TemporaryFileNodeConfig`/`TemporaryFileNode` (`fota/file_node.rs`)

No structural change to the nodes it builds. `register_file_access_methods` (new, in
`file_access.rs`) takes the already-built `TemporaryFileNode` plus a `backing_path: PathBuf`
supplied by the caller (the operator already tracks this for `fota::cleanup::register_session_file`,
so this is not new information the operator didn't already have -- just a new place it's also
passed).

## Public API surface (new)

```rust
// fota/file_access.rs
pub fn register_file_access_methods(
    node_manager: &SimpleNodeManager,
    file_node: &TemporaryFileNode,
    backing_path: PathBuf,
    max_byte_string_length: u32,
) -> Arc<FotaFileAccessHandler>;
```

Mirrors `gds/trust_list::register_trust_list_methods`'s shape exactly (node manager + already-
built node identity in, `Arc<Handler>` out) for consistency with this project's established
method-registration API convention.

## Status code mapping (grounded, not assumed -- see research.md)

| Condition | Status |
|---|---|
| `Open` mode byte has reserved bits (4-7) set | `Bad_InvalidArgument` |
| `Open` for write while already open for write | `Bad_NotWritable` |
| `Open` for read while open for write | `Bad_NotReadable` |
| `Open` for read, file does not exist | `Bad_NotFound` |
| Any method (not `Open`) with unknown/foreign-session handle | `Bad_InvalidArgument` |
| `Read`/`Write` on a handle not opened with the matching mode | `Bad_InvalidState` |
| `Read` with `length <= 0` | `Bad_InvalidArgument` |
| `Write` with `data.len() > max_byte_string_length` | `Bad_InvalidArgument` |
| `Write` with empty/null data | `Good` (no-op) |
| Underlying OS I/O error (disk full, permissions, etc.) | `Bad_UnexpectedError` |
