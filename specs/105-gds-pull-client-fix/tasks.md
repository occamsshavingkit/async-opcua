---

description: "Task list for feature 105: GDS Pull Model Client-Side Fix (Run 2)"
---

# Tasks: GDS Pull Model Client-Side Fix (Run 2)

**Input**: Design documents from `/specs/105-gds-pull-client-fix/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Single user story (P1). **Revised during implementation**:
what started as a client-only fix (FR-007) also required two small,
genuinely necessary server-side infrastructure fixes, discovered because
this feature's own integration test is the first thing to ever exercise
real-client-vs-real-server GDS namespace discovery end-to-end. See T005a.

## Path Conventions

`async-opcua-client/src/gds/{registration.rs,csr.rs,gds_client.rs}`; new
`async-opcua-client/tests/gds_pull_client_discovery.rs`. `async-opcua-server`
is already a dev-dependency of `async-opcua-client` (now with
`companion-gds` added, see T001). Also touches
`async-opcua-server/src/node_manager/memory/mod.rs` and
`async-opcua-server/src/gds/mod.rs` (T005a).

---

## Phase 1: Setup

- [X] T001 Added `features = ["companion-gds"]` to `async-opcua-server`'s dev-dependency entry in `async-opcua-client/Cargo.toml`.

---

## Phase 2: Foundational

- [X] T002 Re-verified `Session::get_namespace_index`/`translate_browse_paths_to_node_ids` and `BrowsePath`/`RelativePath`/`RelativePathElement`/`BrowsePathResult`/`BrowsePathTarget`/`ExpandedNodeId` shapes -- all matched research.md exactly.

**Checkpoint**: Discovery building blocks confirmed present and shaped as expected.

---

## Phase 3: User Story 1 - Client discovers and dispatches against real GDS NodeIds (Priority: P1) 🎯 MVP

**Goal**: Replace hardcoded namespace-0 NodeIds with real, dynamically
discovered ones (OPC UA Part 4 §5.8.4 `TranslateBrowsePathsToNodeIds`).

### Implementation for User Story 1

- [X] T003 [US1] Rewrote `registration.rs`: `GdsRegistrationClient::new(directory_object_id, register_method_id)`; removed `Default`/hardcoded `new()`. `register_application(...)` body unchanged.
- [X] T004 [US1] Rewrote `csr.rs`: renamed `certificate_manager_id` -> `directory_object_id`; `GdsCsrClient::new(directory_object_id, start_signing_request_id, finish_signing_request_id)`; removed `Default`/hardcoded `new()`. **Also found and fixed a fifth pre-existing defect while writing the real end-to-end test (T007)**: `start_signing_request` sent a bogus 5th argument (`regenerate_private_key: bool`) that does not exist in the real `StartSigningRequest` signature -- independently re-verified against the real NodeSet2.xml's `InputArguments` (`ArrayDimensions="4"`: `ApplicationId`/`CertificateGroupId`/`CertificateTypeId`/`CertificateRequest` only). Removed the parameter from `start_signing_request` and `GdsClient::request_signing_csr` (a justified signature change, superseding FR-006's letter for this one provably-wrong parameter, same precedent as feature 104's `certificate_manager_id` rename).
- [X] T005 [US1] Added `GdsClient::discover(session: &Session) -> Result<Self, Error>` exactly per research.md's plan; removed `GdsClient::new()`/`Default`; kept `from_parts`.
- [X] T005a **New, not in original plan**: while writing T007's real end-to-end test, discovery failed even though the server-side wiring was correct, surfacing two genuine, previously-undiscovered server-side bugs (server-side code was otherwise untouched by this feature until these were found):
  1. `Server_NamespaceArray` (`node_manager/memory/core.rs:1065`, Part 5 §6.3.4) is built from each node manager's `namespaces_for_user()`, but `InMemoryNodeManager::namespaces_for_user` (`memory/mod.rs`) delegated purely to the impl's own *static* `namespaces()` list (e.g. `CoreNodeManagerImpl::namespaces()` hardcodes only `["http://opcfoundation.org/UA/"]`), never reflecting a namespace added to the address space at runtime (like a companion import) -- meaning the array a *remote client* reads never contained the GDS namespace at all, regardless of this session's earlier `owns_node`/`refresh_namespaces` fix (feature 103), which only fixed server-side *dispatch*, not this client-facing *discovery* array. Fixed by having `namespaces_for_user` merge in any namespace from `Self::namespaces()` (the already-refreshable wrapper cache) not already reported by the impl.
  2. Even after (1), the GDS namespace still didn't appear correctly: `DiagnosticsNodeManager` independently claims a namespace index for the server's own application URI via `context.type_tree`'s *own, separate* `NamespaceMap` at construction time -- never registered into `AddressSpace.cold.namespaces` (the table feature 103's namespace-seeding fix reads from). Since these two namespace tables are maintained completely independently, the companion import's "next free index" calculation (seeded only from `AddressSpace::namespaces()`) collided with the index `context.type_tree` had already claimed for the app's own namespace, and the aggregation in `core.rs:1071`'s `HashMap`-collect silently dropped one of the two conflicting entries. Fixed by changing `register_gds_pull_methods_from_companion`'s signature to accept `type_tree: &DefaultTypeTree` and pre-seeding the address space with any namespace `type_tree` already knows before importing, so the companion import can never pick a colliding index. Updated the 2 existing call sites (`gds_pull_companion_integration.rs`, this feature's new test) to pass `&server_handle.type_tree().read()`.
- [X] T006 [US1] Removed the obsolete `default_client_uses_standard_gds_helpers` test; replaced the `apply_renewed_certificate` test's `GdsClient::new()` call with `GdsClient::from_parts(...)` + dummy explicit NodeIds (that test doesn't touch registration/csr NodeIds at all).

### Tests for User Story 1

- [X] T007 [P] [US1] New `async-opcua-client/tests/gds_pull_client_discovery.rs`: real server (companion NodeSet imported via `register_gds_pull_methods_from_companion`) + real client. Asserts: (a) `discover` succeeds; (b) every resolved NodeId is neither an old fabricated constant nor namespace 0; (c) `register_application` -> `Bad_NotSupported` (reaches real dispatch, no callback registered -- separate, already-tracked backlog item); (d) `request_signing_csr` -> `Bad_SecurityModeInsufficient` (**revised from the planned `Bad_NotFound`**: the anonymous/`None`-security test session is rejected by `StartSigningRequest`'s own encrypted-channel+SecurityAdmin check, Part 12 §7.9.3, before ever reaching the `ApplicationId` lookup -- equally strong proof the Call reached the real, resolved handler, just via a different but still spec-meaningful status).
- [X] T008 [US1] Same file: `discover_fails_closed_against_a_server_without_the_gds_namespace` -- a server with no GDS import returns a specific `Err`, no panic.
- [X] T009 [US1] `cargo test -p async-opcua-client --test gds_pull_client_discovery -- --nocapture` -- 2/2 pass.
- [X] T009a [US1] Realized within T007: `request_signing_csr` called twice in sequence on the one discovered `GdsClient` (no second `discover()`), both dispatch identically against the same NodeIds. `GdsClient::discover`'s doc comment states discovery is one-shot per instance with no internal re-discovery path.

**Checkpoint**: Client discovers and dispatches against real NodeIds end-to-end; fabricated constants fully removed; namespace-array staleness (T005a) no longer masks real discovery.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [X] T010 `cargo test -p async-opcua-client --all-features` (21+ tests) and `cargo test -p async-opcua-server --all-features` (379+ tests) -- both fully green, zero regression from T005a's server-side fixes.
- [X] T011 `grep -rn "22384\|22385\|22388\|22400\|22402" async-opcua-client/src/gds/` -- zero matches.
- [X] T012 Updated `TODO.md`: closed the "GDS Pull model client-side fix (Run 2)" entry.
- [X] T013 [P] Updated `tools/cu-coverage-report/src/lib.rs`'s CU 3582 entry noting the client-side sibling defect (and the two T005a server-side infrastructure bugs it surfaced) are now also fixed.
- [X] T014 [P] Mirrored into `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T015 `cargo clippy --all-targets --all-features` and `cargo fmt --all` (workspace-wide) -- clean.
- [X] T016 Ran the full local CI gate (4 attempts; the first 3 were killed externally mid-build with no real failures, each progressing further -- the 4th, launched with an explicit max Bash-tool timeout, completed) -- green: only the expected/spurious `verify-codegen: check clean` (uncommitted working tree, zero actual generated-code drift) failed; build matrix, clippy, fmt, footprint, feature-lattice, and all 4 interop stacks passed.

---

## Dependencies & Execution Order

Phase 2 blocked Phase 3. T003/T004 blocked T005. T005a was discovered *during* T007 (writing the real integration test), not planned in advance -- exactly the "verify empirically, don't assume" discipline this project's GDS work has consistently required. Polish (T010-T016) depends on Phase 3 being complete and green.

## Implementation Strategy

1. T001-T002 (setup + API re-verification).
2. T003-T006 (the rewrite, including T005a's server-side infrastructure fixes surfaced along the way) -> validate compiles.
3. T007-T009a (real client-vs-server integration test) -> validate green.
4. T010-T016 (regression, docs, CI gate) -> PR.
