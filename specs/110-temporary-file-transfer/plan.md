# Implementation Plan: TemporaryFileTransferType

**Branch**: `110-temporary-file-transfer` | **Date**: 2026-08-02 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/110-temporary-file-transfer/spec.md`

## Summary

Implement `TemporaryFileTransferType` (OPC-10000-20 §4.4.1-§4.4.5), CUs 3810/3811/3812/3813/5791,
as a single new module `async-opcua-server/src/fota/temporary_transfer.rs` plus two small plumbing
changes to feature 106's `fota::file_access` (a shared handle-counter and an optional per-transfer
`max_file_size` cap). Three methods: `GenerateFileForRead` (creates a temp `FileType`, runs a
producer callback, returns node + open read handle), `GenerateFileForWrite` (creates a writable
temp `FileType`, returns node + open write handle; client writes via feature 106's existing
`Write`), and `CloseAndCommit` (invokes a consumer callback with the committed bytes, then deletes
the temp file and its node). `completionStateMachine` is always null (synchronous completion is
explicitly valid per §4.4.6). NO client-supplied path ever; the server creates temp files at
server-chosen paths under the operator-configured `temp_dir`, and `generateOptions` is
type-checked server-specific data.

## Technical Context

**Language/Version**: Rust (workspace MSRV, matches rest of `async-opcua-server`)
**Primary Dependencies**: existing `fota::file_node` (`TemporaryFileNode::create`,
`TemporaryFileNodeConfig`), existing `fota::file_access` (`FotaFileAccessHandler`,
`register_file_access_methods_full`, `FotaFileHandleRegistry`), existing `fota::cleanup`
(`register_session_file` -- session-disconnect reap), `moka::sync::Cache` (idle-timeout-bounded
handle/transfer registry), `opcua_core::sync` (parking_lot-based `Mutex`/`RwLock`),
`opcua_types` (`Argument`, `ObjectTypeId::TemporaryFileTransferType`, `Variant`,
`StatusCode`, `NodeId`).
**Storage**: In-memory only (`moka` cache). No persistence across restarts (spec.md Assumption).
**Testing**: TDD -- the 13 unit tests in `fota/temporary_transfer/tests.rs` were written first and
drove the implementation. Gates: `cargo test -p async-opcua-server fota::temporary_transfer
--all-features` (13/13 green), `cargo clippy -p async-opcua-server --all-targets --all-features
-- -Dwarnings` (clean), `cargo fmt --all -- --check` (clean).
**Target Platform**: Same as rest of workspace.
**Project Type**: Library (Rust workspace crate feature addition).
**Performance Goals**: None new. Temp-file and handle operations are simple cache lookups / one
filesystem op per transfer; the hot path is the existing feature-106 `Read`/`Write`, unchanged.
**Constraints**: NO client-supplied path (security); per-transfer `max_total_bytes` enforced on
every `Write` (disk-exhaustion DoS bound); reuse feature 106 verbatim (no parallel file-access
code); synchronous completion only (no `CompletionStateMachine`).
**Scale/Scope**: 1 new module + its tests submodule; 2 additive changes to
`fota/file_access/mod.rs` (shared `Arc<AtomicU32>` counter ctor; optional `max_file_size` on
`FotaFileAccessHandler` + `register_file_access_methods_full`); 1 new `pub mod` line in
`fota/mod.rs`. No new crate, no new external dependency.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Method semantics, the three `Argument` signatures, and the
  synchronous-completion rule verified against the real §4.4.3-§4.4.6 text via the OPC UA
  reference MCP, not assumption. The `Bad_TypeMismatch` path has a dedicated test proving it
  rejects (not panics) on a downcast mismatch. PASS.
- **II. Do It Right Once**: Reuses feature 106 (`TemporaryFileNode::create`,
  `register_file_access_methods_full`, `register_session_file`) and its `moka` idle-timeout
  pattern rather than re-implementing any of them. The per-transfer cap is enforced inside
  feature 106's existing `handle_write` (the single chokepoint), not duplicated in the new
  module. PASS.
- **III. Individual Task Discipline**: One user story, one module, one commit unit. PASS.
- **IV. Security Is Paramount**: No client path (no traversal surface to sanitize);
  `generateOptions` type-checked; per-transfer write cap + idle-timeout reaping + disconnect
  cleanup bound disk-exhaustion DoS; the type-mismatch downcast path is verified non-panicking.
  PASS.
- **V. No Surprises**: Builds only on already-shipped FileType machinery; no new external deps.
  PASS.

## Files

**New**:
- `async-opcua-server/src/fota/temporary_transfer.rs` -- the handler, config, callbacks, node
  creation, and the three method handlers.
- `async-opcua-server/src/fota/temporary_transfer/tests.rs` -- 13 unit tests (TDD).
- `specs/110-temporary-file-transfer/{plan.md, tasks.md, spec.md}` -- this documentation.

**Modified** (additive only):
- `async-opcua-server/src/fota/mod.rs` -- one `pub mod temporary_transfer;` line.
- `async-opcua-server/src/fota/file_access/mod.rs` --
  - `FotaFileHandleRegistry::with_counter(idle_timeout, counter: Arc<AtomicU32>)` so the new
    handler can share one globally-unique handle counter across all per-transfer file-access
    handlers (the old `new()` is removed; everything delegates to `with_counter`).
  - `FotaFileAccessHandler::new_full(..., max_file_size: Option<u64>, counter: Arc<AtomicU32>)`
    plus `register_file_access_methods_full(..., max_file_size, counter)`. `handle_write` now
    enforces `position + len <= max_file_size` when set, returning `BadInvalidArgument`. The
    old `new()` is removed; everything delegates to `new_full`.
- `async-opcua-server/src/fota/file_access/tests.rs` -- the 3 in-crate `FotaFileAccessHandler::new(`
  call sites updated to `new_full(` with `, None, Arc::new(AtomicU32::new(1))`.

## Verification

```
cargo test   -p async-opcua-server fota::temporary_transfer --all-features   # 13/13
cargo clippy -p async-opcua-server --all-targets --all-features -- -Dwarnings # clean
cargo fmt    --all -- --check                                                # clean
```
