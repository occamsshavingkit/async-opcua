# Feature Specification: TemporaryFileTransferType (On-Demand Temp File Generation)

**Feature Branch**: `110-temporary-file-transfer`
**Created**: 2026-08-02
**Status**: Implemented
**Spec reference**: OPC-10000-20 v1.05 §4.4.1-§4.4.6 (TemporaryFileTransferType)
**Conformance Units**: 3810 (GenerateFileForRead), 3811 (GenerateFileForWrite), 3812 (CloseAndCommit), 3813 (transfer lifecycle), 5791 (multisession cleanup/abort)
**Input**: "Implement `TemporaryFileTransferType`, CUs 3810/3811/3812/3813/5791. On-demand file generation with NO client-supplied path; build directly on the existing FileType machinery (feature 106)."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Operator serves generated files and accepts uploads without exposing the filesystem (Priority: P1)

A server operator wants to expose on-demand file generation and upload flows (firmware images,
log exports, configuration blobs, diagnostics snapshots) through the standard OPC UA File Transfer
model, *without* ever letting a client name a filesystem path. The server generates a temporary
`FileType` object per request, returns it together with an already-open handle, and tears it down
deterministically once the client commits (or abandons) the transfer.

**Why this priority**: This is the entire scope of the feature. Every method, property, and
cleanup path exists to make these temp-file transactions correct, bounded, and self-cleaning.

**Independent Test**: Register a `TemporaryFileTransferType` with a producer and a consumer.
Call `GenerateFileForRead` and read the producer's bytes through the returned handle; then call
`GenerateFileForWrite`, write bytes through the returned handle, and call `CloseAndCommit` to
confirm the consumer received exactly the written bytes and the temp node is gone. This is fully
exercisable without any other OPC UA subsystem.

**Acceptance Scenarios**:

1. **Given** a handler with a producer callback, **When** a client calls `GenerateFileForRead`,
   **Then** the server runs the producer into a server-chosen temp file and returns the temp
   `FileType` NodeId plus an open *read* handle, with `completionStateMachine` null (synchronous
   completion per §4.4.6).
2. **Given** a handler with a consumer callback, **When** a client calls `GenerateFileForWrite`,
   **Then** the server creates a writable temp `FileType` and returns the NodeId plus an open
   *write* handle.
3. **Given** an open write handle from `GenerateFileForWrite`, **When** the client writes bytes
   via the standard `Write` method (feature 106) and then calls `CloseAndCommit`, **Then** the
   server invokes the consumer with the committed bytes, returns a null `completionStateMachine`,
   and removes the temp file and its node.
4. **Given** a server that declares `generateOptions` must be a `String`, **When** a client
   passes any other type, **Then** the server rejects the call with `Bad_TypeMismatch` (never
   panics). Empty/absent `generateOptions` is always accepted (the parameter is optional).
5. **Given** a per-transfer `max_total_bytes` cap configured by the operator, **When** a write
   would cause the committed file to exceed that cap, **Then** the offending `Write` itself is
   rejected with `BadInvalidArgument` -- not deferred to commit.
6. **Given** a producer whose output exceeds `max_total_bytes`, **When** `GenerateFileForRead`
   runs, **Then** the call fails with `BadInvalidArgument` and the partial file is removed.
7. **Given** a handle returned to session A, **When** session B attempts `CloseAndCommit` on it,
   **Then** the server rejects it with `BadInvalidArgument` (handles are session-scoped).
8. **Given** a `CloseAndCommit` on a read-only transfer (or on an unknown handle), **Then** the
   server rejects it with `BadInvalidArgument`.
9. **Given** two concurrent `GenerateFileForWrite` calls in the same session, **Then** the
   returned handles and temp-file NodeIds are all globally distinct.
10. **Given** a consumer that returns `Err`, **When** `CloseAndCommit` runs, **Then** the error
    status is surfaced to the caller *and* the temp file + node are still removed (the
    transaction completes regardless).
11. **Given** an abandoned handle (no commit) past the configured idle timeout, or a session
    disconnect, **Then** the temp file and node are reaped (reusing `fota::cleanup` +
    `moka::time_to_idle`).

## Assumptions

1. **No client-supplied path** ever. `generateOptions` is type-checked, server-specific data;
   it is never interpreted as a path. Temp files are created at server-chosen paths under the
   operator-configured `temp_dir`. There is therefore no path-traversal surface to sanitize.
2. **Synchronous completion only**: `completionStateMachine` is always a null `NodeId`. This is
   explicitly valid per §4.4.6 ("If the transactions are completed when the Method is returned,
   the optional ... parameter returns a null NodeId"). Asynchronous completion state machines
   are out of scope.
3. **In-memory handle registry** (`moka`); no persistence across server restarts.
4. **Reuses feature 106** (`fota::file_node`, `fota::file_access`, `fota::cleanup`) verbatim --
   this feature adds only the `TemporaryFileTransferType` object, its three methods, the
   per-transfer size cap, and the producer/consumer callback wiring. No new file-access code.
5. **Content validation is the application's job**: the consumer callback receives only
   size-validated bytes; whatever the app does inside the callback (reject, parse, sign) is its
   concern.

## Out of Scope

- Asynchronous completion via a real `CompletionStateMachine` (§4.4.6's other branch).
- Persistent temp-file registry surviving restart.
- Any path-mapping or virtual-filesystem layer -- temp files live and die with their transfer.
- RBAC role enforcement on the three methods (the opt-in RBAC feature gates method dispatch
  generically; this feature does not add role-specific logic).

## Constitution Check

- **I. Correctness Over Completion**: Method semantics, argument signatures, and the
  synchronous-completion rule are grounded against the real §4.4.3-§4.4.6 text (verified via
  the OPC UA reference MCP). The `Bad_TypeMismatch` path is exercised by a dedicated test that
  proves it rejects rather than panicking on a downcast failure. PASS.
- **II. Do It Right Once**: Reuses feature 106's `TemporaryFileNode::create`,
  `register_file_access_methods_full`, `register_session_file`, and the `moka` idle-timeout
  pattern rather than re-implementing any of them. The per-transfer `max_total_bytes` cap is
  plumbed through feature 106's existing `handle_write` so the bound is enforced once, at the
  single chokepoint, instead of duplicated here. PASS.
- **III. Individual Task Discipline**: Single user story, single module, single commit unit.
  PASS.
- **IV. Security Is Paramount**: No client path; type-checked `generateOptions`; per-transfer
  write cap + idle-timeout reaping + disconnect cleanup bound disk-exhaustion DoS. The
  `Bad_TypeMismatch` downcast path is verified not to panic. PASS.
- **V. No Surprises**: Builds on the already-shipped FileType machinery; no new external deps.
  PASS.
