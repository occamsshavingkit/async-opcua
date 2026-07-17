---

description: "Task list for feature 101: GDS Push Model Fix + Completion (Run 1)"
---

# Tasks: GDS Push Model Fix + Completion (Run 1)

**Input**: Design documents from `/specs/101-gds-push-fix/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Tasks are grouped by user story (P1→P2), each
independently implementable/testable.

## Path Conventions

`async-opcua-server/src/gds/push_methods.rs` (rewritten); small addition
to `async-opcua-crypto/src/certificate_store.rs`; new
`async-opcua-server/tests/gds_push_integration.rs` (actual location; see T011's revision note below).

---

## Phase 1: Setup

- [X] T001 Add `CertificateStore::read_rejected_certs()` in `async-opcua-crypto/src/certificate_store.rs`, mirroring `read_trusted_certs`/`read_issuer_certs`.

---

## Phase 2: Foundational

- [X] T002 In `push_methods.rs`, remove `CERTIFICATE_MANAGER_OBJECT_ID`/`START_SIGNING_REQUEST_METHOD_ID`/`CREATE_SIGNING_REQUEST_METHOD_ID` (fabricated NodeIds), `GdsSigningRequest`/`GdsCreatedSigningRequest`/`GdsSigningRequestRegistry` types, and `handle_start_signing_request` (misplaced Pull-model method) entirely.
- [X] T003 Replace with verified real NodeId constants (OPC-10000-12 §7.10, empirically confirmed): `SERVER_CONFIGURATION_OBJECT_ID=12637`, `CREATE_SIGNING_REQUEST_METHOD_ID=12737`, `UPDATE_CERTIFICATE_METHOD_ID=13737`, `APPLY_CHANGES_METHOD_ID=12740`, `CANCEL_CHANGES_METHOD_ID=25708`, `GET_REJECTED_LIST_METHOD_ID=12777`, `RESET_TO_SERVER_DEFAULTS_METHOD_ID=25709`.
- [X] T004 Add `PushTransaction` struct + `GdsPushRegistry { transaction: RwLock<Option<PushTransaction>> }` (data-model.md), replacing `GdsSigningRequestRegistry`.

---

## Phase 3: User Story 1 - Certificate rotation workflow (Priority: P1) 🎯 MVP

**Goal**: A real, working CreateSigningRequest → UpdateCertificate →
ApplyChanges/CancelChanges workflow (OPC-10000-12 §7.10.5/§7.10.9/§7.10.10/§7.10.11).

### Implementation for User Story 1

- [X] T005 [US1] Implement real `CreateSigningRequest` (§7.10.10): build a PKCS#10 DER CSR via `x509_cert::builder::RequestBuilder` + `pkcs1v15::SigningKey<Sha256>` signed with the server's own key (mirroring `X509::create_from_pkey`'s signer pattern); handle `RegeneratePrivateKey`/`Nonce`/`SubjectName` per spec. Auth: encrypted channel + SecurityAdmin.
- [X] T006 [US1] Implement `UpdateCertificate` (§7.10.5): parse+validate the incoming DER certificate, stage it (+ optional PEM private key) as a `PushTransaction` bound to the calling session; `Bad_TransactionPending` if another session's transaction is open; return `ApplyChangesRequired=true`. Auth: authenticated channel + SecurityAdmin.
- [X] T007 [US1] Implement `ApplyChanges` (§7.10.9): verify caller owns the open transaction; commit via `opcua_crypto::gds_reload::save_new_credentials` + `reload_store_from_disk`, update `ServerInfo::endpoint_certificates`; clear the transaction; `Bad_NothingToDo` if none open. Auth: authenticated channel + SecurityAdmin.
- [X] T008 [US1] Implement `CancelChanges` (§7.10.11): verify caller owns the open transaction, discard it; `Bad_NothingToDo` if none open. Auth: authenticated channel + SecurityAdmin.
- [X] T009 [US1] Wire all four methods' callbacks in `register_gds_push_methods_with_registry` against the real NodeIds from T003.

### Tests for User Story 1

- [X] T010 [P] [US1] Unit tests in `push_methods.rs`: CreateSigningRequest returns a parseable CSR; UpdateCertificate stages + returns ApplyChangesRequired=true; ApplyChanges commits and reloads; CancelChanges discards; a second session's UpdateCertificate while one is open returns Bad_TransactionPending; ApplyChanges/CancelChanges with no transaction return Bad_NothingToDo; each method's auth requirements are enforced (unencrypted / non-SecurityAdmin rejected before any state change).
- [X] T011 [US1] **Revised during implementation**: rather than a wire-level round-trip for the full transaction workflow (no established pattern for granting a real connecting client SecurityAdmin exists anywhere in this codebase yet -- would have required building new RBAC role-mapping test infra), added `async-opcua-server/tests/gds_push_integration.rs`: a real running server + real client + real Call-service request against `CreateSigningRequest`'s verified NodeIds, proving the dispatch chain (the exact layer the original bug broke) reaches the registered handler. The full CreateSigningRequest→UpdateCertificate→ApplyChanges/CancelChanges workflow, including the actual on-disk certificate change, is proven at the handler level by the T010 unit tests (which exercise real `CertificateStore`/`gds_reload` state, not mocks).
- [X] T012 [US1] Run T010-T011; confirm all pass.

**Checkpoint**: Certificate rotation workflow closes the majority of CU 2231.

---

## Phase 4: User Story 2 - Rejected-list review and reset (Priority: P2)

**Goal**: `GetRejectedList` and `ResetToServerDefaults` (OPC-10000-12 §7.10.12/§7.10.13).

### Implementation for User Story 2

- [X] T013 [US2] Implement `GetRejectedList` (§7.10.12) directly against `CertificateStore::read_rejected_certs()` (the DefaultApplicationGroup CertificateGroup delegation target doesn't exist in this server's address space, per research.md). Auth: authenticated channel + SecurityAdmin.
- [X] T014 [US2] Implement `ResetToServerDefaults` (§7.10.13) via the existing shutdown-scheduling mechanism (`ShutdownTarget`/`ServerHandle::shutdown_after_with_return_time`, feature 097), setting a warning shutdown reason. Auth: authenticated channel + SecurityAdmin.
- [X] T015 [US2] Wire both methods' callbacks against the real NodeIds from T003.

### Tests for User Story 2

- [X] T016 [P] [US2] Unit tests: GetRejectedList returns rejected certs from the store; ResetToServerDefaults schedules a shutdown with a warning message; both enforce SecurityAdmin + authenticated-channel.
- [X] T017 [US2] **Revised during implementation**: rejected-cert surfacing is proven at the handler level (`get_rejected_list_returns_rejected_certificates`, using `CertificateStore::store_rejected_cert`+`read_rejected_certs` directly), consistent with T011's revision -- no separate wire-level rejection-flow test added this run.
- [X] T018 [US2] Run T016-T017; confirm all pass.

**Checkpoint**: Closes the remainder of CU 2231's implementable surface.

---

## Phase 5: Polish & Cross-Cutting Concerns

- [X] T019 Run the full `async-opcua-server` test suite (including `gds_push_integration.rs`, `gds_integration.rs`, `gds_pull_methods.rs`), the `async-opcua` facade integration suite, and the `async-opcua-crypto` lib suite; confirm zero regressions (especially `pull_methods.rs`, left untouched).
- [X] T020 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CU 2231 with verified NodeIds and test evidence.
- [X] T021 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T022 Update `TODO.md`: close CU 2231's implementable surface; record the sibling `pull_methods.rs`/CU 2230 bug and the deferred TrustList/CertificateGroup methods (Run 2) as explicit follow-ups.
- [X] T023 Run `cargo clippy --all-targets --all-features`, `cargo fmt --all`, then the full CI gate (`tools/ci-playbook.sh --ci`, launched detached).

---

## Dependencies & Execution Order

Phase 2 (fix + constants + registry) blocks both user stories. US1 is the
MVP (the actual certificate-rotation workflow); US2 is independent of US1
but shares the same file, so implement serially.

## Implementation Strategy

1. T001 → T002-T004 (fix + foundation) → validate compiles → commit.
2. US1 (T005-T012) → validate → commit.
3. US2 (T013-T018) → validate → commit.
4. Polish → PR.
