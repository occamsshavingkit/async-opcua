---

description: "Task list for feature 103: GDS Pull Model Fix (Run 1)"
---

# Tasks: GDS Pull Model Fix (Run 1)

**Input**: Design documents from `/specs/103-gds-pull-fix/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Tasks are grouped by user story (P1→P2), each
independently implementable/testable.

## Path Conventions

New `async-opcua-server/src/gds/directory_instance.rs`; rewritten
`async-opcua-server/src/gds/pull_methods.rs`; extended
`async-opcua-server/src/gds/mod.rs` and `async-opcua-server/src/lib.rs`
(`pub mod companion;`); new
`async-opcua-server/tests/gds_pull_companion_integration.rs` (feature-gated
on `companion-gds`).

---

## Phase 1: Setup

- [X] T001 Expose the companion module: change `mod companion;` to `pub mod companion;` in `async-opcua-server/src/lib.rs` so `import_gds` is reachable from `gds/` (also added `missing_docs` to `companion/mod.rs`'s existing lint-allow list, since making it `pub` surfaced ~60 missing-doc warnings on the macro-generated `import_*` functions).
- [X] T002 Fetched `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` locally for development/testing; added `/schemas/companion/*/` to `.gitignore` (was not previously ignored -- a real gap, now fixed) to guarantee it's never committed.

---

## Phase 2: Foundational

- [X] T003 Created `async-opcua-server/src/gds/directory_instance.rs`: `resolve_gds_namespace` reverse-looks-up the GDS namespace index via `AddressSpace::namespaces()` against `"http://opcfoundation.org/UA/GDS/"`; fails closed (returns `None`, logs) if absent.
- [X] T004 `instantiate_certificate_directory` confirms `CertificateDirectoryType` exists at `NodeId::new(gds_ns, 63)` via `AddressSpace::find`; fails closed if absent. Note: discovered mid-implementation that `CertificateGroupFolderType`/`CertificateGroupType`/`TrustListType` (the `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree's types) are already *core* namespace-0 types (already in the generated nodeset) -- only `CertificateDirectoryType` itself needs the companion namespace.
- [X] T005 Instantiated the "Directory" object graph (data-model.md `DirectoryInstanceNodeIds`): the `Directory` object (`HasTypeDefinition` -> the imported `CertificateDirectoryType`), its six Mandatory methods with spec-authored `InputArguments`/`OutputArguments` (§7.9.3-§7.9.10 signatures from research.md, using core `ObjectTypeId`s for the subtree), via `ObjectBuilder`/`MethodBuilder` (the `fota/file_node.rs` pattern). Empirically verified end-to-end against the real GDS NodeSet2.xml: `instantiates_a_real_directory_object_when_companion_xml_is_present` test imports the real XML and confirms every resolved NodeId is a real, findable node (2/2 tests pass).
- [X] T006 Added `GdsApplicationRecord` registry to `pull_methods/mod.rs`: `moka::sync::Cache<NodeId, GdsApplicationRecord>` bounded via `GDS_REGISTRY_CAPACITY` (mirroring Run 2's TrustList handle-cache pattern, more directly reusable here than the old `push_bounded_fifo`/`VecDeque` pattern since this is keyed lookup, not FIFO), with `register_application(application_uri, default_application_group_id)` (not the full `RegisterApplication` Method -- see research.md).
- [X] T007 Added `GdsPullRequest`/`PullRequestState` (data-model.md) to `pull_methods/mod.rs` (also `moka`-backed, same capacity bound), replacing the old `GdsCertificateUpdate`/`GdsFinishedSigningRequest` types entirely. Converted `pull_methods.rs` to a `pull_methods/mod.rs` + `pull_methods/tests.rs` directory module proactively (avoiding the Codacy file-size finding Run 2 hit).

---

## Phase 3: User Story 1 - Certificate issuance workflow (Priority: P1) 🎯 MVP

**Goal**: A real, working `StartSigningRequest`/`StartNewKeyPairRequest` →
`FinishRequest` workflow (OPC-10000-12 §7.9.3/§7.9.4/§7.9.5).

### Implementation for User Story 1

- [X] T008 [US1] Implemented `StartSigningRequest` (§7.9.3): validates `ApplicationId` is registered (`Bad_NotFound`); extracts the CSR's public key (new `X509::public_key_from_signing_request`); issues a real certificate for it signed by this server's own key (new `X509::issue_certificate_for_public_key`, a genuinely new CA-issuance primitive -- this server acting as issuer for a *third party's* public key, distinct from every existing self-signed-cert code path in this crate); stores a `Completed` request; returns the `RequestId`. **Revised during implementation**: strict CSR-`ApplicationUri`-matches-registered-application validation (`Bad_CertificateUriInvalid`) was descoped -- parsing a CSR's own SubjectAltName extension is separate, non-trivial work from parsing a *finished certificate's* SAN (which `is_application_uri_valid` already does); the issued certificate's own SAN is instead authored directly from the registered application's `application_uri` (spec-sanctioned: "The subject in the CSR may be ignored by the CertificateManager"), and `is_application_uri_valid` is applied to the *output* certificate as a correctness check instead. Auth: encrypted channel + SecurityAdmin (see module docs on GDS-specific-role simplification).
- [X] T009 [US1] Implemented `StartNewKeyPairRequest` (§7.9.4): validates `ApplicationId`; generates a new RSA key pair (`PrivateKey::new`); issues a certificate for its public key via the same `X509::issue_certificate_for_public_key`; rejects `PFX` (`Bad_NotSupported`, unimplemented packaging) and any format other than empty/`PEM`/`PFX` (`Bad_InvalidArgument`); stores a `Completed` request including the PEM private key; returns the `RequestId`. Auth: encrypted channel + SecurityAdmin.
- [X] T010 [US1] Implemented `FinishRequest` (§7.9.5): `Bad_NotFound` for an unregistered `ApplicationId`; `Bad_InvalidArgument` if `RequestId` unknown or doesn't match `ApplicationId`; `Bad_NothingToDo` if `Pending`; otherwise returns `(Certificate, PrivateKey, IssuerCertificates)` and evicts the request from the registry. Auth: encrypted channel + SecurityAdmin.
- [X] T011 [US1] Wired all six methods' callbacks in `register_pull_method_callbacks` (`pull_methods/mod.rs`, table-driven per the Run 2 `register_trust_list_methods` pattern) and `gds::register_gds_pull_methods_from_companion` (`gds/mod.rs`) -- imports the companion XML, instantiates the Directory, registers callbacks, all against `CoreNodeManager` (not `SimpleNodeManager`: the Directory instance and its methods are namespace-0-managed nodes structurally, matching Run 1's `CoreNodeManager` correction for `ServerConfigurationType`). Explicit opt-in call, not auto-wired into `ServerBuilder`.

### Tests for User Story 1

- [X] T012 [P] [US1] Unit tests in `pull_methods/tests.rs`: `StartNewKeyPairRequest` → `FinishRequest` returns a real, valid, non-self-signed certificate + private key; `StartSigningRequest` with a real CSR → `FinishRequest` returns a certificate with an empty private key slot; `FinishRequest` on a directly-constructed `Pending` request → `Bad_NothingToDo`; `FinishRequest` with an unknown RequestId → `Bad_InvalidArgument`; unregistered `ApplicationId` → `Bad_NotFound`; auth requirements enforced (role + encrypted channel). All against the real companion XML (skipped gracefully if not present locally). 8 tests, all pass.
- [X] T013 [US1] Unit tests in `directory_instance.rs`: instantiation succeeds and produces real, distinct, findable NodeIds when the companion XML is present; fails closed (`None`, no panic) when it is not. 2/2 pass.
- [X] T014 [US1] New `async-opcua-server/tests/gds_pull_companion_integration.rs` (feature-gated on `companion-gds`, skipped gracefully if the XML isn't present locally): a real running server + real client + real Call-service request against `StartNewKeyPairRequest`'s resolved NodeId, proving the dispatch chain reaches the registered handler.
- [X] T015 [US1] Ran T012-T014; all pass. This test surfaced two genuine, previously-undiscovered bugs in shared node-manager infrastructure, both now fixed: (1) `import_companion_xml` seeded a disconnected, empty `NamespaceMap::default()` instead of one reflecting the address space's already-registered namespaces, risking silent namespace-index collisions on any real (non-fresh) server -- fixed by seeding from `AddressSpace::namespaces()`. (2) `InMemoryNodeManager::owns_node` checked a `namespaces: HashMap` snapshot taken once at construction time, never refreshed -- so a companion NodeSet imported after server startup was never recognized as "owned," and Call/Read/Write/Browse dispatch would never route to it. Fixed by making the field a `RwLock<HashMap>` with a new `refresh_namespaces()` method, called from `register_gds_pull_methods_from_companion` right after `import_gds`. (3) A third, independent bug in `CoreNodeManagerImpl::call_builtin_method` (`node_manager/memory/core.rs`): under the (default-on) `subscriptions-standard` feature, the function unconditionally `return`ed early whenever `method_id.as_method_id()` failed (i.e. for any method outside the core namespace-0 `MethodId` enum), *before* ever consulting the generic `method_with_context_cbs` registry -- meaning any custom method callback registered via `add_method_callback_with_context` for a non-namespace-0 method (companion-spec methods, or any future custom-namespace method) was silently unreachable. Push-model methods happened to dodge this because `ServerConfigurationType`'s methods are real namespace-0 `MethodId` variants; Pull-model's companion-namespaced methods are not. Fixed by changing the early `return` into an `if let` so a non-namespace-0 method id simply falls through to the registry check instead of short-circuiting. All three fixes verified via the full `async-opcua-server --all-features` test suite (378+ tests, 0 failures) plus targeted `gds_push_integration`/`gds_integration` regression checks and a `companion-gds`-disabled build (zero warnings, zero behavior change).

**Checkpoint**: Certificate issuance workflow closes the majority of Run 1's scope.

---

## Phase 4: User Story 2 - Discovery and status methods (Priority: P2)

**Goal**: `GetCertificateGroups`, `GetTrustList`, `GetCertificateStatus`
(OPC-10000-12 §7.9.7/§7.9.9/§7.9.10).

### Implementation for User Story 2

- [X] T016 [US2] Implemented `GetCertificateGroups` (§7.9.7): returns the registered application's `certificate_group_ids` (always `[default_application_group_id]` this run). `Bad_NotFound` for an unregistered application. Auth: authenticated channel + SecurityAdmin.
- [X] T017 [US2] Implemented `GetTrustList` (§7.9.9): returns `default_application_group_trust_list_id`; `Bad_InvalidArgument` for a non-null, unrecognized `CertificateGroupId`. Auth: authenticated channel + SecurityAdmin.
- [X] T018 [US2] Implemented `GetCertificateStatus` (§7.9.10): **Revised during implementation**: this run doesn't track per-issued-certificate expiry/status state (no such tracking exists yet for Pull-model-issued certs), so it always reports `UpdateRequired=false` once the application is confirmed registered (`Bad_NotFound` otherwise) -- documented as a simplification rather than fabricating a status check with no real backing state. Auth: authenticated channel + SecurityAdmin.
- [X] T019 [US2] Wired via the same table-driven `register_pull_method_callbacks` as T011 (all six methods registered together, not a separate pass).

### Tests for User Story 2

- [X] T020 [P] [US2] Unit tests: `GetCertificateGroups` returns the real `DefaultApplicationGroup` NodeId; `GetTrustList` returns the real TrustList NodeId; `GetCertificateStatus` reports `false`; unregistered application → `Bad_NotFound`. 3 tests, all pass.
- [X] T021 [US2] Ran T020; all pass.

**Checkpoint**: Closes the Mandatory Pull-model surface for CU 2230.

---

## Phase 5: Polish & Cross-Cutting Concerns

- [X] T022 Investigated `RevokeCertificate`/`GetCertificates`/`CheckRevocationStatus` (Optional, §7.9.6/§7.9.8/§7.9.11): all three require genuinely new, non-trivial infrastructure this run didn't build -- `GetCertificates` needs a persistent per-application issuance ledger (this run's `GdsApplicationRecord` tracks only `certificate_group_ids`, not issued-certificate history); `RevokeCertificate` needs real CRL mutation (adding a serial number to a CertificateGroup's revocation list, correctly re-signed); `CheckRevocationStatus` needs a revocation-status lookup across those CRLs. None of this is unambiguous from the spec text alone without designing that ledger/CRL-mutation model, so all three are deferred (matching Run 1's `CreateSelfSignedCertificate`/`DeleteCertificate`/`GetCertificates` Optional-not-implemented pattern) -- recorded in TODO.md as a follow-up.
- [X] T023 Ran the full `async-opcua-server --all-features` test suite (378+ tests across all integration files, 0 failed), the `gds::` unit suite with `--no-default-features --features companion-gds,gds` (42/42 pass), and `gds_push_integration.rs`/`gds_integration.rs` explicitly (both pass) -- zero regression to features 101/102. Built with `--no-default-features --features gds` (companion-gds off) and with `--all-features`: both compile with zero warnings after the cfg-gating fixes in T024 below.
- [X] T024 [P] Updated `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE`. Correction during this task: the spec's original "CU 2230" citation was wrong -- this codebase's own register tracks "GDS Certificate Manager Pull Model" as **CU 3582** (confirmed: no CU 2230 entry exists anywhere in AUDIT_TABLE or CU-COVERAGE.md; CU 3582's pre-existing `Partial` row already described the old, buggy pull_methods.rs). Updated CU 3582 to `Implemented` with full evidence; updated CU 2231's "sibling bug" note (3 identical entries existed at the time) from "CU 2230 ... remains out of scope" to "CU 3582 ... fixed by Feature 103 (Run 1)". `audit_table_is_sorted_by_cu_id_for_binary_search` test still passes (no row insertion needed, only text/status updates).
- [X] T025 [P] Hand-updated `specs/conformance-tester/CU-COVERAGE.md`'s CU 3582 row (`partial` -> `implemented`, full evidence) and all 3 occurrences of CU 2231's sibling-bug note, matching AUDIT_TABLE -- no local normalized-snapshot JSON was available to regenerate the file via `cargo run -p async-opcua-cu-coverage-report`, so this mirrors the exact text change by hand (consistent with how the file is otherwise a checked-in snapshot of a point-in-time audit, not continuously regenerated).
- [X] T026 Updated `TODO.md`: removed the "GDS Pull model sibling bug" open-backlog entry, added a `Done` entry for feature 103 (correcting the CU-number citation to 3582), added a new open-backlog entry for the client-side Run 2 follow-up (`async-opcua-client/src/gds/`) plus the deferred Optional methods from T022, and fixed the two other stale "CU 2230" mentions (features 101/102's Done entries) to the correct "CU 3582".
- [X] T027 `cargo clippy --all-targets --all-features`: clean after adding a `CompletedRequestBundle` type alias to fix a `type_complexity` finding in `pull_methods/mod.rs` (and cfg-gating two dead-code/unused-import warnings surfaced by disabling `companion-gds`, see T023). `cargo fmt --all`: clean. `companion-gds`-disabled build (`--no-default-features --features gds`) and `--all-features` build both zero-warning. `tools/ci-playbook.sh --ci` launched in the background; awaiting result before opening the PR.

---

## Dependencies & Execution Order

Phase 2 (companion wiring + instantiation + registries) blocks both user
stories -- neither can dispatch a real Call without real NodeIds to
register against. US1 is the MVP (the actual certificate-issuance
workflow); US2 is independent of US1 but shares the same file and
registration function, so implement serially.

## Implementation Strategy

1. T001-T002 (setup) → T003-T007 (foundation) → validate compiles, companion import works locally → commit.
2. US1 (T008-T015) → validate → commit.
3. US2 (T016-T021) → validate → commit.
4. Polish (T022-T027) → PR.
