---

description: "Task list for feature 108: GDS Directory Application-Registry Services"
---

# Tasks: GDS Directory Application-Registry Services

**Input**: Design documents from `/specs/108-gds-directory-app-registry/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Single user story (P1).

## Path Conventions

New `async-opcua-server/src/gds/application_record.rs` (hand-authored `ApplicationRecordDataType`
+ its `TypeLoader`), new `async-opcua-server/src/gds/like_match.rs` (LIKE-operator matcher).
Extends `async-opcua-server/src/gds/directory_instance.rs`, `async-opcua-server/src/gds/
pull_methods/mod.rs`, `async-opcua-server/src/gds/mod.rs`. Extends
`async-opcua-server/tests/gds_pull_companion_integration.rs`.

---

## Phase 1: Setup

- [X] T001 Re-verify (do not trust research.md alone) the exact Part 12 v1.05.07 wording for all 7 methods (§6.5.4, §6.5.6-§6.5.11) and the Part 4 v1.05.07 LIKE-operator grammar (Table 120) against the real local PDFs at `~/opcua-specs/` via `pdftotext -layout`, before writing any code. Re-confirm the 8 NodeIds (146/151/992/143/200/149/216/142) directly against `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml`.

---

## Phase 2: Foundational

- [X] T002 [P] Create `async-opcua-server/src/gds/like_match.rs`: a `pub(crate) fn like_match(pattern: &str, value: &str) -> bool` implementing Part 4 Table 120 exactly (`%`, `_`, `\`-escape, `[...]`/`[^...]` char sets with ranges, case-sensitive). Unit tests covering every construct in the table, including the exact worked examples the spec text itself gives (`'Th[ia][ts]%'` matching `'That is fine'`/`'This is fine'`/etc., `'main%'`, `'%en%'`, `'_ould'`, `5[%]`, `5[_]`, `abc[13-68]`, `xyz[c-f]`, `ABC[^13-5]`, `xyz[^dgh]`).
- [X] T003 Create `async-opcua-server/src/gds/application_record.rs`: hand-authored `ApplicationRecordDataType` struct (`application_id: NodeId`, `application_uri: UAString`, `application_type: ApplicationType`, `application_names: Option<Vec<LocalizedText>>`, `product_uri: UAString`, `discovery_urls: Option<Vec<UAString>>`, `server_capabilities: Option<Vec<UAString>>`), `#[derive(BinaryEncodable, BinaryDecodable, Debug, Clone, PartialEq, Default)]` (+ `#[cfg_attr(feature = "json", derive(JsonEncodable, JsonDecodable))]` + `#[cfg_attr(feature = "xml", derive(XmlEncodable, XmlDecodable, XmlType))]`), matching `ApplicationDescription`'s exact generated shape/field order (research.md R2/R8). Implement `ExpandedMessageInfo` by hand, using the GDS companion namespace's runtime-resolved URI and the DataType's own NodeId (`ns=1;i=1`) reused as its own binary/json/xml encoding id (research.md R8's documented convention -- add a doc comment explaining why, citing R8).
- [X] T004 In the same file, add `GdsApplicationRecordTypeLoader` mirroring `samples/custom-codegen/src/generated/types/mod.rs`'s `GeneratedTypeLoader` exactly: a `TypeLoaderInstance` (lazily built) with one `add_binary_type`/`add_json_type`/`add_xml_type` entry for `ApplicationRecordDataType`, and an `impl TypeLoader` checking the GDS companion namespace URI before delegating. Unit test: round-trip encode a value through `ExtensionObject::new(...)`, decode it back via the loader, assert equality.
- [X] T005 [P] Extend `DirectoryInstanceNodeIds` (`directory_instance.rs`) with the 8 new fields (`register_application_id`, `query_servers_id`, `query_applications_id`, `find_applications_id`, `update_application_id`, `unregister_application_id`, `get_application_id`, `applications_folder_id`), resolved the exact same fail-closed way the existing 9 fields already are (constants + `AddressSpace::find` verification loop). Extend the existing unit test asserting all 17 fields resolve to their real, re-verified NodeIds (T001).
- [X] T006 Extend `GdsApplicationRecord` (`pull_methods/mod.rs`) with the new fields from data-model.md (`record_id: u64`, `application_type`, `application_names`, `product_uri`, `discovery_urls`, `server_capabilities`), defaulted sensibly in the existing `register_application(application_uri, default_application_group_id)` convenience constructor (unchanged call sites/behavior). Add `registry_created_at: DateTime` to `GdsPullMethodRegistryInner`, captured once in its `Default` impl (data-model.md's `LastCounterResetTime` source, research.md R9). Assign `record_id` from the existing `next_id: AtomicU64` counter (reuse, don't add a second counter).

**Checkpoint**: The hand-authored type round-trips through its own `TypeLoader`, NodeIds resolve, and the extended registry compiles with existing Pull-model tests still green -- before any of the 7 new method handlers are written against them.

---

## Phase 3: User Story 1 - Operator manages the registered-application inventory (Priority: P1) 🎯 MVP

**Goal**: Real, spec-correct RegisterApplication/UpdateApplication/UnregisterApplication/
GetApplication/FindApplications/QueryApplications/QueryServers against the real Directory object,
closing CUs 2232 and 3581.

### Implementation for User Story 1

- [X] T007 [US1] Implement `handle_register_application` in `pull_methods/mod.rs`: decode the `ApplicationRecordDataType` input argument (via the `TypeLoader`-backed `ExtensionObject`), reject with `Bad_InvalidArgument` if `ApplicationUri` is empty (FR-011), check `WellKnownRole::SecurityAdmin` (research.md R4) -> `Bad_UserAccessDenied` if absent, check for an existing record with the same `ApplicationUri` (FR-002) -> `Bad_EntryExists`, otherwise insert a new record (assigning both a fresh `ApplicationId` NodeId key and the next `record_id`) and return the `ApplicationId`.
- [X] T008 [US1] Implement `handle_update_application`: decode input, require `SecurityAdmin` (R4), look up by `ApplicationId` -> `Bad_NotFound` if absent, reject with `Bad_WriteNotSupported` if `ApplicationUri` differs from the stored record's (Part 12 §6.5.7), otherwise update the record's other fields in place and bump its `record_id` to the next counter value (a record's identifier changes on update too, per §6.5.10's "assign a monotonically increasing identifier... each time the GDS creates OR updates").
- [X] T009 [US1] Implement `handle_unregister_application`: require `SecurityAdmin` (R4), look up by `ApplicationId` -> `Bad_NotFound` if absent, remove the record. Do NOT implement certificate revocation (research.md R6) -- add a doc comment on the handler citing R6/CU 3582 explaining why, so this isn't mistaken for an oversight later.
- [X] T010 [US1] Implement `handle_get_application`: no role restriction (R2/R4), look up by `ApplicationId` -> `Bad_NotFound` if absent, encode the found record as `ApplicationRecordDataType` and return it.
- [X] T011 [US1] Implement `handle_find_applications`: no role restriction, exact (non-LIKE) match on `ApplicationUri` per §6.5.4's own wording (distinct from QueryApplications' LIKE-based `ApplicationUri` filter -- verify this distinction against the real spec text in T001, don't assume symmetry), returning 0 or 1 `ApplicationRecordDataType` in the output array; empty/too-long URI -> `Bad_InvalidArgument`.
- [X] T012 [US1] Implement `handle_query_applications`: no role restriction; apply all provided filters as AND (`ApplicationName`/`ApplicationUri`/`ProductUri` via `like_match` when non-empty, `ApplicationType` bitmask `0x1`=Server/`0x2`=Client/`0`=all, `Capabilities` requiring the record's own `server_capabilities` to be a superset); never return a record whose `server_capabilities` includes `"NA"`; paginate by `record_id > StartingRecordId`, capped at `MaxRecordsToReturn` (0 = unbounded), returning `NextRecordId` (0 if exhausted) and the registry's `registry_created_at` as `LastCounterResetTime`; project matching records to `ApplicationDescription` per data-model.md's Table 13 mapping.
- [X] T013 [US1] Implement `handle_query_servers`: same filter/pagination logic as T012 minus the `ApplicationType` filter (Servers implied), reusing the SAME underlying filtered/paginated record set (don't duplicate the filter-and-paginate logic -- factor it into a shared private helper both T012 and T013 call, projecting the result differently per data-model.md's Table 15 mapping: one `ServerOnNetwork` row per `discovery_url` in each matching record).
- [X] T014 [US1] Wire all 7 new handlers into `register_pull_method_callbacks` (`pull_methods/mod.rs:550`) and `DirectoryInstanceNodeIds`' new fields, alongside the existing 6 Pull-model method registrations, following the exact same `add_method_callback_with_context` pattern.

### Tests for User Story 1

- [X] T015 [P] [US1] Unit tests for the registry-level logic (in `pull_methods/mod.rs`'s existing test module): RegisterApplication assigns a unique ID and rejects a duplicate URI; UpdateApplication rejects a URI change and rejects an unknown ID; UnregisterApplication removes the record and rejects an unknown ID; GetApplication/FindApplications return `Bad_NotFound`/empty appropriately; QueryApplications' AND-combined filtering (name/uri/type/product/capabilities), `NA`-capability exclusion, and pagination (StartingRecordId/MaxRecordsToReturn/NextRecordId) each get a dedicated case; QueryServers' one-row-per-discovery-url fan-out.
- [X] T016 [US1] Extend `async-opcua-server/tests/gds_pull_companion_integration.rs` with a real client/server end-to-end test: register the `GdsApplicationRecordTypeLoader` on both `ServerBuilder` (`with_type_loader`) and the connected `Session` (`add_type_loader`), then RegisterApplication -> QueryApplications finds it -> GetApplication returns it -> UpdateApplication changes a field -> QueryApplications reflects the change -> UnregisterApplication -> QueryApplications no longer finds it, all through real Call-service dispatch. Also test that a non-`SecurityAdmin` session gets `Bad_UserAccessDenied` from RegisterApplication but still succeeds against QueryApplications.
- [X] T017 [US1] Run T002/T004/T015/T016; all pass.

**Checkpoint**: A real client can manage the registered-application inventory end-to-end through
the standard OPC UA DirectoryType methods.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [X] T018 `cargo test -p async-opcua-server --all-features`: 0 failures (incl. unchanged GDS Push/Pull suites, features 101-105). `cargo build -p async-opcua-server --no-default-features --features gds` (companion-gds disabled): zero warnings.
- [X] T019 Update `TODO.md`: narrow the "GDS Directory / Authorization / KeyCredential services" entry to remove CUs 2232/3581 (closed) and keep only what remains (Authorization Service, KeyCredential Service, JWT/OAuth2 discovery, CU 2233's orthogonal LDS-ME connectivity). Add two new entries per research.md R6/R7: `UnregisterApplication` doesn't revoke certificates (tied to CU 3582's ledger/CRL gap) and no GDS method emits audit events yet (broader gap, not specific to this feature). Add a "Done" entry for feature 108.
- [X] T020 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CUs 2232 and 3581 (`Gap` -> `Implemented`).
- [X] T021 [P] Mirror T020 into `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T022 `cargo clippy --all-targets --all-features` and `cargo fmt --all -- --check` (workspace-wide) -- clean.
- [X] T023 Run the full local CI gate before opening the PR.

---

## Dependencies & Execution Order

Phase 2 (hand-authored type + TypeLoader + NodeIds + extended registry) blocks Phase 3 entirely --
none of the 7 handlers can be written before the type they traffic in exists and round-trips.
T012/T013 share a private filter/paginate helper (T013 depends on T012's helper existing, not on
T012's own task being "done" first -- implement together if that's more natural). Polish (T018-T023)
depends on Phase 3 being complete and green.

## Implementation Strategy

1. T001 (re-verify spec grounding) -> confirms exact semantics before any code.
2. T002-T006 (hand-authored type, TypeLoader, LIKE matcher, NodeIds, extended registry) ->
   validated compiles and round-trips in isolation.
3. T007-T014 (7 handlers + wiring) -> validated compiles.
4. T015-T017 (tests, incl. real end-to-end lifecycle) -> validated green.
5. T018-T023 (regression, docs, CI gate) -> PR.
