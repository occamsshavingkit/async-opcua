---

description: "Task list for feature 102: GDS Push Model TrustList Completion (Run 2)"
---

# Tasks: GDS Push Model TrustList Completion (Run 2)

**Input**: Design documents from `/specs/102-gds-push-trustlist/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Tasks are grouped by user story (P1→P2), each
independently implementable/testable.

## Path Conventions

New `async-opcua-server/src/gds/trust_list.rs`; extended
`async-opcua-server/src/gds/push_methods.rs` (`PushTransaction`,
`ApplyChanges`/`CancelChanges`); extended `async-opcua-server/src/gds/mod.rs`
(registration wiring); additions to
`async-opcua-crypto/src/certificate_store.rs`; extended
`async-opcua-server/tests/gds_push_integration.rs`.

---

## Phase 1: Setup

- [X] T001 Add write-side helpers to `async-opcua-crypto/src/certificate_store.rs`: `store_trusted_cert`, `remove_trusted_cert`, `store_issuer_cert`, `remove_issuer_cert`, `store_trusted_crl`, `store_issuer_crl` (DER write via `X509`/`CertificateList::to_der()`, following the existing `store_rejected_cert`/`cert_file_name` naming pattern; `trusted_certs_dir()`/`issuer_certs_dir()`/`trusted_crls_dir()`/`issuer_crls_dir()` already exist).

---

## Phase 2: Foundational

- [X] T002 Extend `PushTransaction` in `async-opcua-server/src/gds/push_methods.rs` (data-model.md) with `pending_trust_list: Option<TrustListDataType>`.
- [X] T003 Extend `ApplyChanges`'s handler in `push_methods.rs` to also commit `pending_trust_list` (if set) via the new `CertificateStore` write helpers from T001, applying only the lists whose `TrustListMasks` bit is set (data-model.md validation rules).
- [X] T004 Extend `CancelChanges`'s handler in `push_methods.rs` to also discard `pending_trust_list` (if set), with no store mutation.
- [X] T005 Create `async-opcua-server/src/gds/trust_list.rs`: `TrustListFileHandle` struct + a `moka::sync::Cache<u32, TrustListFileHandle>`-backed handle registry with `time_to_live` (ActivityTimeout, default 60000ms), modeled on `history::continuation::HistoryContinuationPointCache` (data-model.md, research.md).
- [X] T006 Add real NodeId constants in `trust_list.rs` (research.md, empirically verified): `TRUST_LIST_OBJECT_ID=12642`, `OPEN_METHOD_ID=12647`, `CLOSE_METHOD_ID=12650`, `READ_METHOD_ID=12652`, `WRITE_METHOD_ID=12655`, `GET_POSITION_METHOD_ID=12657`, `SET_POSITION_METHOD_ID=12660`, `OPEN_WITH_MASKS_METHOD_ID=12663`, `CLOSE_AND_UPDATE_METHOD_ID=12666`, `ADD_CERTIFICATE_METHOD_ID=12668`, `REMOVE_CERTIFICATE_METHOD_ID=12670`.

---

## Phase 3: User Story 1 - TrustList read/write/apply cycle (Priority: P1) 🎯 MVP

**Goal**: A real, working `Open`/`OpenWithMasks` → `Read` → `Close` read
cycle, and `Open` → `Write` → `CloseAndUpdate` → `ApplyChanges`/
`CancelChanges` write cycle (OPC-10000-12 §7.8.2.2-§7.8.2.5).

### Implementation for User Story 1

- [X] T007 [US1] Implement `Open` (§7.8.2.2): validate mode is `Read` (1) or `Write|EraseExisting` (6) else `Bad_NotSupported`; `Bad_TransactionPending` if Write-mode requested while the shared `PushTransaction` (Run 1 cert/key OR this run's TrustList) is open on another session; `Bad_SecurityModeInsufficient` if channel unauthenticated. Read mode pre-serializes the current `TrustListDataType` (all four lists, via `CertificateStore::read_trusted_certs`/`read_issuer_certs`/`read_trusted_crls`/`read_issuer_crls`) into the new handle's buffer.
- [X] T008 [US1] Implement `OpenWithMasks` (§7.8.2.3): same as read-mode Open, but filters the serialized `TrustListDataType` to only the lists set in the caller's `TrustListMasks` argument.
- [X] T009 [US1] Implement `Read` (§7.8.2.4): chunked read from the handle's buffer at its current position, honoring the caller's requested length; `Bad_InvalidState`/handle-not-found if the handle doesn't belong to the calling session or has expired.
- [X] T010 [US1] Implement `Write` (inherited FileType semantics): appends/overwrites bytes into the handle's buffer at its current position; only valid for a Write-mode handle.
- [X] T011 [US1] Implement `Close`: discards the handle (read mode: no side effects; write mode without `CloseAndUpdate`: pending bytes discarded, matching FileType semantics).
- [X] T012 [US1] **Revised during implementation**: `CloseAndUpdate` (§7.8.2.5) validates each certificate in the decoded `trusted_certificates` via parse + `X509::is_time_valid` (structural/temporal validity) rather than a full chain-validation simulation against the *proposed* new list via `validate_application_instance_cert` (that function validates an incoming peer cert against the currently-*persisted* store, not a not-yet-applied candidate list -- see research.md); on any failure, discards and returns `Bad_CertificateInvalid`; on success, stages it into the shared `PushTransaction.pending_trust_list` (T002) and returns `ApplyChangesRequired=true`. Also implemented `GetPosition`/`SetPosition` (Mandatory FileType methods, NodeIds verified present) as a small in-scope addition. Auth: authenticated channel + SecurityAdmin.
- [X] T013 [US1] Wired `Open`/`OpenWithMasks`/`Read`/`Write`/`GetPosition`/`SetPosition`/`Close`/`CloseAndUpdate` callbacks in `register_trust_list_methods` in `trust_list.rs` against the real NodeIds from T006, called from `gds/mod.rs`'s `register_gds_certificate_management_methods`/`_with_handle`.

### Tests for User Story 1

- [X] T014 [P] [US1] Unit tests in `trust_list.rs`: `Open`(Read)+`Read`+`Close` returns the actual current trusted certs; `OpenWithMasks` returns only the requested subset; `Open`(Write)+`Write`+`CloseAndUpdate` stages a pending change without mutating the store; `CloseAndUpdate` with an invalid certificate returns `Bad_CertificateInvalid` and leaves the store unchanged; auth requirements enforced (12 tests, all pass).
- [X] T015 [US1] Unit tests in `push_methods.rs`: `ApplyChanges` with a pending `pending_trust_list` updates `CertificateStore`'s trusted certs on disk (and leaves the server's own application cert untouched); `CancelChanges` discards it with no store mutation; `UpdateCertificate` from a second session while a TrustList transaction is open elsewhere returns `Bad_TransactionPending` (3 new tests, all pass).
- [X] T016 [US1] Extended `async-opcua-server/tests/gds_push_integration.rs`: a real running server + real client + real Call-service request against TrustList `Open`'s verified NodeId, proving the dispatch chain reaches the registered TrustList handler. Also fixed a latent pre-existing gap found in the process: this test file's own `#![cfg(...)]` gate was missing `generated-address-space`, which its (Run 1-era) `CoreNodeManager` import already implicitly required -- added it.
- [X] T017 [US1] Ran T014-T016; all pass.

**Checkpoint**: TrustList read/write/apply cycle closes the majority of Run 2's scope.

---

## Phase 4: User Story 2 - Single-certificate Add/Remove (Priority: P2)

**Goal**: `AddCertificate`/`RemoveCertificate` (OPC-10000-12 §7.8.2.6/§7.8.2.7).

### Implementation for User Story 2

- [X] T018 [US2] Implement `AddCertificate` (§7.8.2.6): `Bad_TransactionPending` if a write-mode TrustList transaction is open elsewhere; `Bad_CertificateInvalid` if `IsTrustedCertificate=false` (per spec, this method cannot add issuer certs) or the DER fails to parse/validate; otherwise immediately store via T001's `store_trusted_cert`, no transaction involved.
- [X] T019 [US2] Implement `RemoveCertificate` (§7.8.2.7): `Bad_InvalidArgument` if the thumbprint (via `X509::thumbprint()`) doesn't match any stored trusted/issuer certificate; `Bad_CertificateChainIncomplete` if it's a CA still needed to validate another certificate in the same list; otherwise immediately remove via T001's `remove_trusted_cert`/`remove_issuer_cert`.
- [X] T020 [US2] Wire both methods' callbacks in `register_trust_list_methods` (T013) against the real NodeIds from T006.

### Tests for User Story 2

- [X] T021 [P] [US2] Unit tests (included in T014's `trust_list.rs` suite): `AddCertificate` immediately adds a trusted cert with no `ApplyChanges` call; `IsTrustedCertificate=false` is rejected; `RemoveCertificate` immediately removes a trusted cert; removing a still-needed CA cert (two same-CN self-signed certs, exercising the name-based dependency check without a full CA-signed chain -- see research.md) returns `Bad_CertificateChainIncomplete`; `AddCertificate` returns `Bad_TransactionPending` while a write-mode transaction is open elsewhere.
- [X] T022 [US2] Ran T021; all pass.

**Checkpoint**: Closes the remainder of this run's CU 2231 scope.

---

## Phase 5: Polish & Cross-Cutting Concerns

- [X] T023 Ran the full `async-opcua-server` test suite (366 pass, incl. `gds_push_integration.rs`, `gds_integration.rs`, `gds_pull_methods.rs`), the `async-opcua` facade integration suite (390 pass), and the `async-opcua-crypto` lib suite (150 pass); zero regressions.
- [X] T024 [P] Updated `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CU 2231 to `Implemented` with verified NodeIds and test evidence reflecting full closure of the `DefaultApplicationGroup` surface.
- [X] T025 [P] Regenerated `specs/conformance-tester/CU-COVERAGE.md` (cu-coverage-report's own 5 tests still pass).
- [X] T026 Updated `TODO.md`: closed CU 2231's `DefaultApplicationGroup` surface; kept `DefaultHttpsGroup`/`DefaultUserTokenGroup` and the CU 2230 Pull-model sibling bug recorded as explicit follow-ups.
- [X] T027 Ran `cargo clippy --all-targets --all-features` (clean workspace-wide) and `cargo fmt --all` (applied); verified the specific feature-gating combination that regressed Run 1 (`base-server` without `generated-address-space`, via the release-footprint minimal-server build) still compiles clean; full CI gate next.

---

## Dependencies & Execution Order

Phase 2 (transaction extension + handle cache + NodeId constants) blocks
both user stories. US1 is the MVP (the actual read/write/apply workflow);
US2 is independent of US1 but shares the same file and the same
`Bad_TransactionPending` check against US1's write-mode handles, so
implement serially.

## Implementation Strategy

1. T001 → T002-T006 (foundation) → validate compiles → commit.
2. US1 (T007-T017) → validate → commit.
3. US2 (T018-T022) → validate → commit.
4. Polish → PR.
