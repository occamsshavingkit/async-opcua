# Feature Specification: File Access Real I/O (FileType Open/Read/Write/Close)

**Feature Branch**: `106-file-access-io`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "File Access / Temporary File Access: FileType node metadata exists but no add_method_cb callback wired anywhere — Open/Read/Write/Close Methods are inert nodes, not functional I/O. CUs 3210, 3211, 3213, 3810-3813, 5791."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Client reads and writes a real file through the standard OPC UA File Access methods (Priority: P1)

A client connects to a server exposing a `FileType` object (today, this SDK's only real instance is the FOTA firmware-upload temporary file). The client calls `Open`, then `Write` to upload data, `Close` to finish, then in a later session `Open` for read, `Read` to retrieve the bytes back, and confirms they match what was written. Today every one of these method calls fails or does nothing, because the methods exist as inert address-space nodes with no server-side behavior behind them at all.

**Why this priority**: This is the entire point of the feature — a `FileType` object with structurally-correct nodes but no working `Open`/`Read`/`Write`/`Close`/`GetPosition`/`SetPosition` is not "File Access," it's a facade. Nothing else in this feature is meaningful until real bytes can move through these methods.

**Independent Test**: Run a real server exposing a real `FileType`-backed file, connect a real client, `Open` for write, `Write` a known byte sequence, `Close`, `Open` for read, `Read` it back in full, `Close`, and diff the bytes.

**Acceptance Scenarios**:

1. **Given** a `FileType` object backed by a real file on disk, **When** a client calls `Open` with the write bit set, **Then** it receives a session-scoped file handle and may then call `Write` to append bytes at the current position.
2. **Given** an open write handle, **When** the client calls `Write` repeatedly then `Close`, **Then** the bytes are durably persisted to the backing file in the order written.
3. **Given** a file with existing content, **When** a client calls `Open` with only the read bit set, `Read`s in a loop, **Then** it receives the file's bytes in order and an empty result once the end of the file is reached.
4. **Given** a file already open for writing, **When** a second client attempts to `Open` it for writing, **Then** the second `Open` fails with `Bad_NotWritable`; **when** a second client attempts to `Open` it for reading while it is open for writing, **Then** that fails with `Bad_NotReadable` — both per OPC-10000-20 §4.2.2.
5. **Given** an open handle, **When** the client calls `GetPosition`/`SetPosition`, **Then** the reported/effective position matches actual byte offset, and setting a position beyond the end of file clamps to end-of-file rather than erroring.
6. **Given** a `FileHandle` from a different session (or an already-closed handle), **When** any method other than `Open` is called with it, **Then** the call fails with `Bad_InvalidArgument` — handles are never valid outside the session that opened them, and never valid after `Close`.

### Edge Cases

- Client disconnects (session ends) without calling `Close`: the open handle and any partially-written data must not leak or corrupt the backing file; the existing FOTA session-cleanup path (`fota::cleanup::cleanup_session`) is the natural place this is already anticipated.
- A `Read`/`Write` request whose length/data exceeds the file's advertised `MaxByteStringLength`: must not allocate unbounded memory or trust an attacker-supplied size uncritically (Security Is Paramount) — capped/rejected, not silently honored.
- Writing an empty/null `ByteString`: per spec, must return `Good` with no effect on the file (not an error).
- `SetPosition` to a value larger than `u64` file offsets a real OS file can represent, or a `Read`/`Write` racing a concurrent truncation from outside the server process: must fail safely (an I/O error status), never panic.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST implement real `Open` behavior for a `FileType` object backed by a file on disk: given a mode byte (Read/Write/EraseExisting/Append bits per OPC-10000-20 §4.2.2), it MUST open the backing file accordingly and return a new, session-scoped `FileHandle`.
- **FR-002**: The system MUST implement real `Close`: given a valid `FileHandle`, it MUST release the handle and flush/finalize any pending writes; the handle MUST become invalid for all further use.
- **FR-003**: The system MUST implement real `Read`: given a valid, read-opened `FileHandle` and a requested length, it MUST return up to that many bytes starting at the handle's current position, advance the position by the amount actually returned, and return an empty `ByteString` at end-of-file (never an error for EOF itself).
- **FR-004**: The system MUST implement real `Write`: given a valid, write-opened `FileHandle` and data, it MUST write those bytes at the handle's current position and advance the position by the amount written; writing empty/null data MUST succeed with no effect.
- **FR-005**: The system MUST implement real `GetPosition`/`SetPosition` reflecting the handle's actual current offset into the backing file, with `SetPosition` beyond end-of-file clamping to end-of-file rather than erroring.
- **FR-006**: The system MUST enforce the OPC-10000-20 §4.2.2 open-conflict rules: a second `Open` for writing while already open for writing MUST fail with `Bad_NotWritable`; a second `Open` for reading while open for writing MUST fail with `Bad_NotReadable`; multiple simultaneous read-opens MUST be allowed.
- **FR-007**: The system MUST reject any method call carrying a `FileHandle` that is invalid, expired, or scoped to a different session with `Bad_InvalidArgument`, and MUST NOT allow a handle to be used successfully outside the session that opened it.
- **FR-008**: The system MUST bound the memory/data size honored per `Read`/`Write` call by the file's advertised `MaxByteStringLength` (or a safe server default if that property isn't set), rejecting or truncating oversized requests rather than trusting a client-supplied size unconditionally.
- **FR-009**: File handles and any per-handle state MUST be released when the owning session ends, even if the client never calls `Close` (reusing/extending the existing FOTA session-cleanup path rather than building a parallel mechanism).
- **FR-010**: The `OpenCount` property MUST reflect the number of currently valid handles on the file, and `Writable`/`UserWritable` MUST continue to reflect actual write permission (not silently diverge from real behavior once these methods are functional).

### Key Entities

- **File handle registry**: session-scoped, bounded, in-memory mapping from `FileHandle` (`UInt32`) to open-file state (backing `std::fs::File`, current position, open mode, owning session) — modeled on the existing `TrustListHandleRegistry` pattern (`gds/trust_list/mod.rs`).
- **FileType instance**: an address-space object of `FileType` with `Size`/`Writable`/`UserWritable`/`OpenCount`/`MimeType`/`MaxByteStringLength`/`LastModifiedTime` properties and the six methods — the existing `fota::file_node::TemporaryFileNode` builder already produces this structure; this feature makes its methods real.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A real client can `Open`/`Write`/`Close` a file, then in a separate `Open`/`Read`/`Close` sequence retrieve byte-for-byte identical content, through a real running server (no server-internal shortcuts).
- **SC-002**: The open-conflict rules (FR-006) are independently verified: a concurrent write-open attempt while already open for write fails with the exact spec-mandated status; likewise for read-while-write.
- **SC-003**: A session that disconnects mid-transfer without calling `Close` leaves no dangling handle usable by a later session, and the existing FOTA cleanup test suite continues to pass unchanged.
- **SC-004**: An oversized `Read` length or `Write` payload (larger than `MaxByteStringLength`) is rejected/bounded rather than causing unbounded memory allocation.
- **SC-005**: This closes real, verifiable evidence for CU `3210` (Base Info FileType Write) and CU `3213` (Base Info FileType Base) against a concrete, tested instance.

## Assumptions

- Scope is the base `FileType` (OPC-10000-20 §4.2: `Open`/`Close`/`Read`/`Write`/`GetPosition`/`SetPosition`) only. `FileDirectoryType` (CU `3211`, §4.3 — `CreateFile`/directory browsing, a new object type with its own arbitrary-filesystem-exposure security design questions) and `TemporaryFileTransferType` (CUs `3810`-`3813`/`5791`, §4.4 — `GenerateFileForRead`/`GenerateFileForWrite`/`CloseAndCommit`, a structurally distinct on-demand-file-generation pattern) are explicitly out of scope, documented as follow-ups.
- The concrete `FileType` instance this feature makes real I/O work against is the existing FOTA temporary file (`fota::file_node::TemporaryFileNode`) — the only real, already-instantiable `FileType` object in this codebase today. This feature does not build a new, separate "create an arbitrary file object for arbitrary server data" facility; it makes the *existing* structural builder's methods functional. A generic, reusable file-handle-registry implementation is still built (Key Entities), so a future feature wiring a second `FileType` instance for a different purpose can reuse it without rework.
- The backing storage is a real file on the local filesystem at a path the server operator controls (consistent with `fota::cleanup`'s existing `file_path` concept) — not an in-memory buffer that vanishes on restart or a network/cloud storage abstraction.
- No new public "start a file transfer" orchestration Method is added in this run — an operator constructs a `TemporaryFileNode` (as `fota` already allows) and this feature makes its methods real; a client-facing "request server create me a new file" entry point beyond what FOTA already anticipates is out of scope.
