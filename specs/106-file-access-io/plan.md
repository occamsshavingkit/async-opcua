# Implementation Plan: File Access Real I/O (FileType Open/Read/Write/Close)

**Branch**: `106-file-access-io` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/106-file-access-io/spec.md`

## Summary

`fota::file_node::TemporaryFileNode` already builds a structurally-correct `FileType` object
(OPC-10000-20 §4.2) with real Method nodes for `Open`/`Close`/`Read`/`Write`/`GetPosition`/
`SetPosition` -- but nothing registers callbacks against them, so every call fails or does
nothing. This feature wires real behavior: a session-scoped, disk-backed file handle registry
(directly modeled on `gds/trust_list/mod.rs`'s `TrustListHandleRegistry`, the established pattern
for exactly this "OPC UA file-handle abstraction over Open/Read/Write/Close" shape), real
`std::fs::File` I/O, and the exact open-conflict/status-code semantics grounded against the local
OPC-10000-20 PDF §4.2.

## Technical Context

**Language/Version**: Rust 2021, workspace crate `async-opcua-server`, `fota` module (existing,
gated behind the `fota` Cargo feature)
**Primary Dependencies**: `std::fs`/`std::io` (real file I/O), `moka::sync::Cache` (handle
registry, same crate already used by GDS features 101-105 and TrustList), existing
`node_manager::memory::SimpleNodeManager::add_method_callback_with_context`
**Storage**: Real files on the local filesystem (server-operator-controlled path)
**Testing**: `cargo test` -- new integration test spins up a real server with a real FOTA file
node, connects a real client, and does a full write-then-read-back round trip via real Call
dispatch (mirrors every GDS feature's testing discipline this session)
**Target Platform**: Linux (matches project CI matrix)
**Project Type**: Library (OPC UA server SDK), `fota` feature
**Performance Goals**: N/A -- file I/O is inherently the bottleneck, not a hot dispatch path
**Constraints**: No panics on malformed/adversarial input (Security Is Paramount: attacker-
controlled `FileHandle`, `Length`, `Position`, `Data` values must all fail closed, not crash or
allocate unbounded memory); zero behavior change to any other subsystem
**Scale/Scope**: One new file (`fota/file_access.rs`), no changes to `file_node.rs`'s existing
structure-building code (only to how it's registered), a small, optional extension to
`TemporaryFileNodeConfig`/`TemporaryFileNode` construction if a backing path needs to be
threaded through

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Every status code and open-conflict rule in spec.md's FR-006
  is grounded against the real OPC-10000-20 §4.2 text (Phase 0), not assumed from the method
  names. PASS.
- **II. Do It Right Once**: Reuses the already-proven `TrustListHandleRegistry` shape rather than
  inventing a new pattern; real disk I/O (not another inert/mock layer) the first time. PASS.
- **III. Individual Task Discipline**: Tasks scoped one method/concern at a time (registry, Open,
  Close, Read, Write, GetPosition/SetPosition, registration, tests). PASS.
- **IV. Security Is Paramount**: This is the first feature in the project putting a client-
  addressable path to *arbitrary byte read/write against a real file* behind an OPC UA Method --
  explicit design attention to: bounding `Read` length and `Write` payload size against
  `MaxByteStringLength` (FR-008); handles are session-scoped and never usable cross-session
  (FR-007); no path-traversal surface (the backing path is fixed at object-creation time by the
  *operator*, never derived from client input -- this run does not add any "open an arbitrary
  path a client names" capability, matching spec.md's Assumptions); no panics on adversarial
  `Position`/`Length` values (checked arithmetic, no unchecked casts). PASS, with explicit design
  attention documented in research.md.
- **V. Leave It Better Than You Found It**: Makes an existing, half-built structural precedent
  (`fota/file_node.rs`) actually work rather than leaving a second inert facade alongside it. PASS.

No violations. No Complexity Tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/106-file-access-io/
├── plan.md              # This file
├── research.md          # Phase 0 output -- exact status codes/semantics, TrustList pattern re-verification
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/
├── src/fota/
│   ├── mod.rs            # Add `pub mod file_access;`
│   ├── file_node.rs       # Unchanged structure; TemporaryFileNodeConfig may gain a backing-path field
│   ├── file_access.rs     # New: handle registry + Open/Close/Read/Write/GetPosition/SetPosition handlers + registration
│   └── cleanup.rs         # Unchanged -- handle registry self-expires via moka time_to_idle, same as TrustList
├── tests/
│   └── fota_file_access_integration.rs   # New: real client vs. real server, full write+read round trip
```

**Structure Decision**: One new file inside the existing `fota` module, following the exact
precedent of `gds/trust_list/mod.rs` sitting alongside `gds/push_methods.rs`. No new crate, no
new top-level module, no changes to `cleanup.rs`.

## Complexity Tracking

*No violations -- table omitted.*
