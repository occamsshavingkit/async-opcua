---

description: "Task list for feature 106: File Access Real I/O (FileType Open/Read/Write/Close)"
---

# Tasks: File Access Real I/O (FileType Open/Read/Write/Close)

**Input**: Design documents from `/specs/106-file-access-io/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Single user story (P1).

## Path Conventions

New `async-opcua-server/src/fota/file_access.rs`; `fota/mod.rs` gains
`pub mod file_access;`; new `async-opcua-server/tests/
fota_file_access_integration.rs`. `fota/file_node.rs`/`fota/cleanup.rs`
unchanged.

---

## Phase 1: Setup

- [X] T001 Re-verified `SimpleNodeManagerImpl::add_method_callback_with_context` (`node_manager/memory/simple.rs:1317`): `Fn(&RequestContext, &[Variant]) -> Result<Vec<Variant>, StatusCode>` -- a 2-arg closure, *not* the 3-arg `(ctx, method_id, args)` shape `CoreNodeManagerImpl`'s version uses (confirmed these are two independently-defined methods, not one shared signature). Also confirmed `SimpleNodeManagerImpl::call()` has no equivalent of the `subscriptions-standard` `MethodId` short-circuit bug feature 103/104 found and fixed in `CoreNodeManagerImpl::call_builtin_method` -- registration works cleanly, no additional server-infrastructure fix needed this time.

---

## Phase 2: Foundational

- [X] T002 Created `fota/file_access.rs` with `FotaFileHandleState`/`HandleMode`/`FotaFileHandleRegistry` per data-model.md.

**Checkpoint**: Handle registry compiles and has unit test coverage for insert/get/remove/cross-session-rejection before any method handler is written against it.

---

## Phase 3: User Story 1 - Client reads and writes a real file through standard File Access methods (Priority: P1) 🎯 MVP

**Goal**: Real, spec-correct `Open`/`Close`/`Read`/`Write`/`GetPosition`/`SetPosition` against a
real backing file (OPC-10000-20 §4.2).

### Implementation for User Story 1

- [X] T003 [US1] Implemented `handle_open`. **Refined during implementation**: the open-conflict rule was re-derived more precisely from the spec text than tasks.md's original summary -- a write-open is refused while the file is open in *any* mode (read or write), not just while already open for write; a read-open is refused only while open for write. Tracked via `read_opens`/`write_opens` `AtomicU16` counters on the handler (not per-registry iteration).
- [X] T004 [US1] Implemented `handle_close`.
- [X] T005 [US1] Implemented `handle_read`.
- [X] T006 [US1] Implemented `handle_write`.
- [X] T007 [US1] Implemented `handle_get_position`/`handle_set_position`.
- [X] T008 [US1] `register_file_access_methods` implemented exactly as planned.
- [X] T009 [US1] Added `pub mod file_access;` to `fota/mod.rs`.

### Tests for User Story 1

- [X] T010 [P] [US1] 11 unit tests in `file_access.rs`, all passing: write-then-read round trip, both open-conflict directions, multiple-simultaneous-read-opens, cross-session rejection, empty-write no-op, oversized-write rejection, read-length capping, `SetPosition` EOF clamping, invalid mode bits, and `OpenCount` live-value tracking.
- [X] T011 [US1] New `tests/fota_file_access_integration.rs`: real server + real client, `Open`(write+erase)/`Write`/`Close` then `Open`(read)/`Read`-loop-to-EOF/`Close`, asserts byte-for-byte match against real backing file content. **Descoped from the original plan**: the concurrent write-open-conflict assertion was *not* duplicated here -- it's already covered directly and more simply by T010's unit tests (`second_write_open_is_rejected_while_first_is_open`), and a second concurrent client connection in the integration test would add setup complexity without adding coverage.
- [X] T012 [US1] Ran T010-T011; all pass (11/11 unit + 1/1 integration).

**Checkpoint**: A real client can write and read back real file content end-to-end through the standard OPC UA File Access methods.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [X] T013 `cargo test -p async-opcua-server --all-features`: 390+ tests, 0 failures (incl. `fota::cleanup`/`fota::file_node` unchanged). `cargo build -p async-opcua-server --no-default-features --features gds` (fota disabled): zero warnings.
- [X] T014 Updated `TODO.md`: closed the entry for base `FileType`; the remaining entry now tracks only `FileDirectoryType`/`TemporaryFileTransferType` with the security-design caveat.
- [X] T015 [P] Updated `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CUs `3210`/`3213` (`Partial` -> `Implemented`).
- [X] T016 [P] Mirrored into `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T017 `cargo clippy --all-targets --all-features` and `cargo fmt --all` (workspace-wide) -- clean.
- [X] T018 Ran the full local CI gate -- green on the first attempt (unlike features 103-105's repeated external kills). Only FAIL was the expected/spurious `verify-codegen: check clean` (uncommitted working tree, zero actual generated-code drift).
- [X] T019 Post-review hardening (PR #311 review comments, `chatgpt-codex-connector[bot]` + `coderabbitai[bot]`): fixed a real TOCTOU race in `handle_open`'s conflict-check-then-increment (now serialized via `open_lock: Mutex<()>`); fixed open-mode counters never being decremented when a handle idles out via moka eviction instead of an explicit `Close` (a permanent write-lockout DoS) via a `FotaFileHandleState::Drop` impl reconciling against shared counters, with a `reconciled` flag so an explicit `Close` -- which still decrements synchronously, since a client expects to `Open` again immediately -- doesn't get double-decremented once moka's housekeeper later drops the evicted `Arc`; added support for handles opened `Read|Write` simultaneously (previously collapsed to write-only, so `Read` incorrectly failed); rejected `EraseExisting` without `Write`; applied `Append`'s initial-position seek to read-only opens too (previously write-only); enforced the `writable` flag against write-opens (`Bad_NotWritable`); bounded `Read`'s buffer allocation by remaining file size, not just `MaxByteStringLength`; live-updated the `Size` property after `Write`. Added 7 new unit tests (18 total) plus a disk-persistence assertion in the integration test. Fixed a evidence-text copy-paste typo (`gds/file_node.rs` -> `fota/file_node.rs`) in `tools/cu-coverage-report/src/lib.rs` and `CU-COVERAGE.md`. Deferred (not fixed): wrapping the synchronous `std::fs::File` I/O in `spawn_blocking` -- flagged as a perf/scalability nitpick, not a correctness bug, and inconsistent with no other precedent in this codebase (GDS's method-callback handlers do similar synchronous I/O); worth a workspace-wide look if the maintainers want it, not scoped to this feature. Full regression re-verified: 18/18 unit + 1/1 integration, `cargo clippy --all-targets --all-features` clean, `cargo fmt --all` clean, `cargo test -p async-opcua-server --all-features` green, `cargo build --no-default-features --features gds` (fota disabled) zero warnings.

---

## Dependencies & Execution Order

Phase 2 (registry) blocked Phase 3. T003-T007 (individual handlers) were written in order but
share no inter-dependency beyond compiling against the same registry. Polish (T013-T018) depends
on Phase 3 being complete and green.

## Implementation Strategy

1. T001-T002 (setup + registry foundation) -> validated compiles, unit-tested in isolation.
2. T003-T009 (handlers + registration) -> validated compiles.
3. T010-T012 (tests, incl. real end-to-end round trip) -> validated green.
4. T013-T018 (regression, docs, CI gate) -> PR.
