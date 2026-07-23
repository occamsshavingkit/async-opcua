# Tasks: Complete the 27 Partial Conformance Units

**Input**: Design documents from `specs/109-partial-cu-completion/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cu-spec-map.md, quickstart.md

**Tests**: This feature IS test-completion — every task produces or asserts a test. For the 8 Type-B CUs the task also makes a small code change; the paired test is what verifies it.

## Format: `[ID] [P?] [Story] Description`

- **[P]** = parallelizable (touches a different file, no dependency on an incomplete task).
- Every task is scoped to **one CU**. Execute one at a time (Constitution III).
- **Mandatory per task**: (1) `grep` the named symbol to find CURRENT code — the audit's line numbers are stale; (2) read the CU's OPC UA Part/§ in `contracts/cu-spec-map.md` via the OPC-UA reference MCP (or `~/opcua-specs/` PDFs) BEFORE writing anything; (3) assert the spec's observable behavior, not the code's shape.
- **Type-B anti-self-verification rule** (the 8 impl-gap tasks — T003, T004, T005, T008, T011, T012, T025, T026): after writing the fix + test, temporarily revert ONLY the fix, run the new test, and confirm it FAILS (red); then restore the fix and confirm it PASSES (green). A test that still passes with the fix reverted does not assert the behavior and does not close the CU. Record the red→green confirmation in the task's completion note.

---

## Phase 1: Setup (grounding)

- [ ] T001 Read `specs/109-partial-cu-completion/research.md`, `contracts/cu-spec-map.md`, and `quickstart.md`. Confirm the OPC-UA reference MCP responds (try a `cu` or `search_terms` lookup for CU 2275); if it does not, fall back to `pdftotext -layout` on `~/opcua-specs/` PDFs. Confirm the workspace builds: `cargo build -p async-opcua-server --all-features`.

**Checkpoint**: grounding tools verified — CU work can begin. There is no foundational (cross-story blocking) phase; all 27 CUs are independent, so user stories may proceed in any order.

---

## Phase 2: User Story 1 — Alarms & Conditions (Priority: P1)

**Goal**: close 2275, 2811, 2814, 2918, 4466. **Independent test**: run the alarms tests + browse the state-machine instances.

- [x] T002 [P] [US1] **CU 2275 (Trip)** — TEST-ONLY. Grep `DiscreteAlarmKind` + `Trip` in `async-opcua-server/src/alarms/discrete.rs`. Read Part 9 TripAlarmType (cu-spec-map row 2275). Add a test (in the alarms `#[cfg(test)]` module, alongside the existing OffNormal test) that drives a Trip-kind discrete alarm through activation and return-to-normal and asserts (a) it activates/clears and (b) its TypeDefinition resolves to `TripAlarmType`.
- [x] T003 [US1] **CU 2811 (GeneratesEvent)** — IMPL-GAP. Grep `ProgramStateMachine` (`async-opcua-server/src/programs/`) and `ShelvingStateMachine` (`async-opcua-server/src/alarms/state_machine.rs`), and `ReferenceTypeId::GeneratesEvent` (`address_space/mod.rs`). Read Part 10 / Part 16 / Part 3 §7 (row 2811). At each machine's instantiation add a `GeneratesEvent` reference from the owning object to the event type it emits. Add a test browsing the instance's GeneratesEvent references and asserting the expected target type is present.
- [x] T004 [US1] **CU 2814 (AvailableStates/AvailableTransitions)** — IMPL-GAP. Grep `AvailableStates` / `AvailableTransitions` (same state-machine instantiation sites as T003). Read Part 16 / Part 5 §B.4.5 (row 2814). Populate each Property's Value with the instance's real State / Transition `NodeId[]`. Add a test reading both Properties and asserting non-empty arrays matching the machine's declared states/transitions.
- [x] T005 [US1] **CU 2918 (HasEventSource)** — IMPL-GAP. Grep `has_event_source` (`async-opcua-nodes/src/object.rs`) and `write_has_condition_reference` (`async-opcua-server/src/alarms/deviation.rs` / `rate_of_change.rs` / `limit.rs`) for the pattern to mirror. Read Part 3 §7 HasEventSource/HasNotifier (row 2918). Add a `HasEventSource` reference from the notifier/area to the event-source object at the alarm-source binding site (mirroring the HasCondition wiring; do NOT restructure the notifier tree). Add a test asserting the HasEventSource reference exists and is browsable.
- [x] T006 [P] [US1] **CU 4466 (Respond2)** — TEST-ONLY. Grep `Respond2` / `respond2` in `async-opcua-server/src/alarms/`. Read Part 9 DialogConditionType Respond2 (row 4466). Add a test (mirroring the existing Respond test) that calls Respond2 on an active dialog and asserts the dialog transitions/validates.

---

## Phase 3: User Story 2 — Historical Access (Priority: P1)

**Goal**: close 2289, 2950 on BOTH backends. **Independent test**: run the in-memory + sqlite history tests.

- [x] T007 [US2] **CU 2289 (UpdateEvent)** — TEST-ONLY, both backends. Grep `PerformUpdateType::Update` / `update_event` in `async-opcua-server/src/history/` and the sqlite backend. Read Part 11 §6.8 (row 2289). Add an Update-mode event-history test to `async-opcua-server/tests/history_events_inmemory.rs` AND the sqlite history test in `async-opcua-history-sqlite`, asserting upsert semantics (update-in-place if present, insert if absent).
- [x] T008 [US2] **CU 2950 (distinct timestamps)** — IMPL-GAP, both backends. Grep `history_read_raw_modified` + `timestamps_to_return` in `async-opcua-server/src/node_manager/memory/simple.rs` (note the `_timestamps_to_return` ignored param). Read Part 4 §7.7 DataValue + Part 11 read (row 2950). Fix the in-memory backend to honor `timestamps_to_return` (mask returned source/server timestamps to only those requested, matching sqlite). Add a test to `async-opcua-server/tests/history_data_inmemory.rs` AND the sqlite history test: store a value whose source timestamp differs from its server timestamp; read with `Both` → both distinct survive; read with `Source` → server timestamp absent.

---

## Phase 4: User Story 3 — Audit event coverage (Priority: P1)

**Goal**: close 2422, 3224, 3542, 3968. **Independent test**: run the audit/security tests.

- [x] T009 [P] [US3] **CU 2422 (encrypted audit)** — TEST-ONLY. Grep `dispatch_audit_event` (`async-opcua-server/src/session/audit.rs`) and how audit events reach subscriptions. Read Part 4 Auditing + Part 2 Security (row 2422). Add a test (in `tests/security_tests.rs`) that: opens a `SignAndEncrypt` secure channel + session, subscribes to the Server object's audit-event notifier (event monitored item), triggers an audit-generating action, and asserts the audit event is RECEIVED on that encrypted session — receipt over the SignAndEncrypt session is the proof of encrypted delivery (the transport layer, not application code, does the encryption; the test proves audit events flow through it end-to-end).
- [x] T010 [US3] **CU 3224 (NodeManagement audit)** — TEST-ONLY. Grep the `dispatch_*` add/delete-nodes functions in `session/audit.rs` and their call sites in `node_manager/memory/memory_mgr_impl.rs`. Read Part 4 §5.7 + the Audit*Nodes/*References event types (row 3224). Add tests asserting audit emission for DeleteNodes, AddReferences, and DeleteReferences (AddNodes is already covered).
- [x] T011 [US3] **CU 3542 (RoleMappingRuleChanged)** — IMPL-GAP. Grep `add_identity` / `remove_identity` in `async-opcua-server/src/rbac/role_management.rs` and `RoleMappingRuleChanged`. Read Part 18 §4 + the audit event type (row 3542). Emit `RoleMappingRuleChangedAuditEventType` after a successful mapping mutation, via the same dispatch path the other audit events use. Add a test calling AddIdentity/RemoveIdentity and asserting the audit event is emitted.
- [x] T012 [US3] **CU 3968 (HistoryUpdate audit)** — IMPL-GAP. Grep `dispatch_write_audit` in `session/audit.rs` (confirm zero `history` refs there) and the HistoryUpdate service handler (grep `HistoryUpdate` in `session/services/attribute.rs` / node-manager history path). Read Part 11 audit + AuditHistory*UpdateEventType (row 3968). Add `dispatch_history_update_audit` mirroring `dispatch_write_audit`, emitting the correct `AuditHistory*UpdateEventType` subtype, and wire the call at the HistoryUpdate handler. Add a test performing a HistoryUpdate and asserting emission.

---

## Phase 5: User Story 4 — RBAC well-known role permissions (Priority: P2)

**Goal**: close 3539, 3540, 3541. **Independent test**: run the rbac tests.

- [x] T013 [P] [US4] **CU 3539 (ConfigureAdmin)** — TEST-ONLY. Grep `ConfigureAdmin` + `preset` in `async-opcua-server/src/rbac/`. Read Part 3 well-known roles + Part 18 (row 3539). Add a test asserting ConfigureAdmin's permission bitset matches the Part 3 definition (mirror the existing SecurityAdmin perm test).
- [x] T014 [P] [US4] **CU 3540 (AuthenticatedUser)** — TEST-ONLY. Grep `AuthenticatedUser` + `preset` in `async-opcua-server/src/rbac/`. Read Part 3 well-known roles (row 3540). Add a test asserting AuthenticatedUser's permission bitset (mirror the existing Anonymous perm test).
- [x] T015 [P] [US4] **CU 3541 (Observer/Engineer/Supervisor)** — TEST-ONLY. Grep `Observer` / `Engineer` / `Supervisor` + `preset` in `async-opcua-server/src/rbac/`. Read Part 3 well-known roles (row 3541). Add tests asserting each of the three roles' permission bitsets (mirror the existing Operator perm test).

---

## Phase 6: User Story 5 — Subscriptions & MonitoredItems (Priority: P2)

**Goal**: close 2318, 2818, 3142, 5208, 3544. **Independent test**: run the subscription/monitored-item tests.

- [x] T016 [P] [US5] **CU 2318 (queueSize clamp)** — TEST-ONLY. Grep `sanitize_queue_size` in `async-opcua-server/src/subscriptions/`. Read Part 4 §5.12.2 / §7.16 (row 2318). Add a test creating a monitored item with a queueSize above the server maximum and asserting the revised queueSize is clamped to the max. Do NOT expand event-monitored-item queueing.
- [x] T017 [P] [US5] **CU 2818 (structured value)** — TEST-ONLY. Grep the sampling pipeline (`sample`) in `async-opcua-server/src/subscriptions/`. Read Part 4 §5.12 (row 2818). Add a test that monitors a node holding a Structure (ExtensionObject) value and asserts the notification carries the structured value.
- [x] T018 [P] [US5] **CU 3142 (dataEncoding XML/JSON)** — TEST-ONLY. Grep `data_encoding` in the subscriptions sampling path. Read Part 4 §5.12 + Part 6 encoding (row 3142). Add a monitored-item test specifying an XML (and/or JSON) dataEncoding and asserting the notification value is encoded as requested.
- [x] T019 [P] [US5] **CU 5208 (IndexRange)** — TEST-ONLY. Grep `range_of` / `index_range` in the subscriptions sampling path. Read Part 4 §7.22 NumericRange + §5.12 (row 5208). Add a test creating a monitored item with an IndexRange over an array node and asserting only the ranged sub-value is delivered.
- [x] T020 [US5] **CU 3544 (ResendData)** — TEST-ONLY. Grep `Server_ResendData` (`node_manager/memory/core.rs`) and `resend_data` (`subscriptions/subscription.rs`). Read Part 4 §5.13.6 (row 3544). Add a test calling ResendData and asserting the next publish resends current values for the subscription's monitored items.

---

## Phase 7: User Story 6 — Read/Write/Call value handling (Priority: P3)

**Goal**: close 2203, 2454, 3605. **Independent test**: run the read/write/call service tests.

- [x] T021 [P] [US6] **CU 2203 (Write structure)** — TEST-ONLY. Grep `write_node_value` in `async-opcua-server/src/address_space/`. Read Part 4 §5.10 (row 2203). Add a test writing an ExtensionObject (structured) value to a writable node and reading it back, asserting the value round-trips.
- [x] T022 [P] [US6] **CU 2454 (Call structure arg)** — TEST-ONLY. Grep the method-call argument handling in `async-opcua-server/src/node_manager/method.rs`. Read Part 4 §5.11 (row 2454). Add a test in `tests/method_call_tests.rs` whose Call input argument is a Structure/ExtensionObject and assert it round-trips into the method body.
- [x] T023 [P] [US6] **CU 3605 (MaxNodesPerMethodCall)** — TEST-ONLY. Grep `MaxNodesPerMethodCall` / `max_nodes_per_method_call` in `node_manager/memory/core.rs` and config. Read Part 4 §5.11 + Part 5 OperationLimits (row 3605). Add a test in `tests/method_call_tests.rs` issuing a Call exceeding MaxNodesPerMethodCall and asserting the server enforces the limit with the spec status.

---

## Phase 8: User Story 7 — Server metadata, event fields, filter limits, custom types (Priority: P3)

**Goal**: close 2476, 3546, 3194, 3201. **Independent test**: run the core/event/custom-codegen tests.

- [x] T024 [P] [US7] **CU 2476 (Server_LocalTime)** — TEST-ONLY. Grep `Server_LocalTime` + `TimeZoneDataType` in `async-opcua-server/src/node_manager/memory/core.rs`. Read Part 5 §6.3.3 + Part 3 TimeZoneDataType (row 2476). Add a test reading the Server_LocalTime attribute and asserting a plausible `TimeZoneDataType`.
- [x] T025 [US7] **CU 3546 (event localTime)** — IMPL-GAP. Grep `local_time` in `async-opcua-core/src/events.rs` / the event-construction path, and `Server_LocalTime` in `core.rs`. Read Part 5 §6.4.2 BaseEventType (row 3546). Populate an emitted event's `local_time` from the SAME `TimeZoneDataType` source `Server_LocalTime` uses (extract a shared helper if needed so the two agree). Add a test emitting an event and asserting its localTime is non-null/plausible. (Depends on understanding T024's source; may be done after T024.)
- [x] T026 [US7] **CU 3194 (Max Select/Where ClauseParameters)** — IMPL-GAP. Grep `MaxSelectClauseParameters` / `MaxWhereClauseParameters` (generated nodeset + `core.rs`) and the event-filter evaluator for its effective cap. Read Part 4 event filter + Part 5 ServerCapabilities (row 3194). (research R10 confirmed NO config field for these exists yet.) Add two config fields — `max_select_clause_parameters` and `max_where_clause_parameters` on the server limits struct — defaulting to the evaluator's current effective caps, and populate the two ServerCapabilities node Values from them at address-space construction. Add a test in `tests/event_filter_tests.rs` reading both nodes and asserting non-null values equal to the configured limits.
- [x] T027 [P] [US7] **CU 3201 (custom EventTypes + encoding objects)** — TEST-ONLY. Grep `samples/custom-codegen` generated types (`encoding_ids`, `gen`). Read Part 3 §5.8 + Part 6 encoding (row 3201). First ENUMERATE the sample's custom EventTypes from its generated `types/` module (the codegen output lists them); then add an e2e test in the custom-codegen sample's test suite that browses the custom namespace and, for each enumerated custom EventType, asserts the browse result contains a `HasEncoding` reference to its Encoding object(s) (Default Binary, and Default XML/JSON where generated). Distinct from CU 5801.

---

## Phase 9: User Story 8 — Security hardening (Priority: P3)

**Goal**: close 2823. **Independent test**: run the security tests.

- [x] T028 [US8] **CU 2823 (invalid user token)** — TEST-ONLY (decision fixed in research.md R1: NO escalating lockout — Part 2 §6.6 / CR 1.11 make it optional). Grep `tarpit_authentication_failure` / `IDENTITY_TOKEN_VALIDATION_TARPIT` in `async-opcua-server/src/session/negotiate.rs`. Read Part 2 §6.6 + CR 1.11 (row 2823). Add a test in `tests/security_tests.rs` (near the existing tarpit test) asserting (a) an invalid user token is rejected with the correct status and (b) the failure path is delayed by the tarpit. Do NOT add escalation or any per-source state.

---

## Phase 10: Polish — ledger, regeneration, gates

- [x] T029 Flip all 27 rows in `tools/cu-coverage-report/src/lib.rs` `AUDIT_TABLE` from `EvidenceStatus::Partial` to `EvidenceStatus::Implemented`, each with a RE-VERIFIED evidence string (current file:line of the code + the exact new test name). Update any `cu-coverage-report` self-tests that assert these ids as `partial`. CUs: 2275, 2811, 2814, 2918, 4466, 2289, 2950, 2422, 3224, 3542, 3968, 3539, 3540, 3541, 2318, 2818, 3142, 5208, 3544, 2203, 2454, 3605, 2476, 3546, 3194, 3201, 2823.
- [x] T030 Regenerate `specs/conformance-tester/CU-COVERAGE.md`: `cargo run -p async-opcua-cu-coverage-report --quiet -- /home/quackdcs/micro-opcua/profiles/opcua-profile-normalized-snapshot.json specs/conformance-tester/CU-COVERAGE.md`. Verify all 27 rows now read `implemented` (see quickstart.md end-gate grep, expect 27). **Regression guard (FR-009/SC-005)**: also confirm the 3 Extensible rows are untouched — `grep -E "\| (2479|2480|2786) \|" specs/conformance-tester/CU-COVERAGE.md | grep -c extensible` must equal 3 — and that the total `gap` count did not change from its pre-feature value (141). If either guard fails, a CU was flipped that should not have been.
- [ ] T031 Full regression: `cargo test -p async-opcua-server --all-features`, `cargo test -p async-opcua-history-sqlite`, `cargo test -p async-opcua-cu-coverage-report`; and `cargo build -p async-opcua-server --no-default-features` (must stay green). 0 failures.
- [ ] T032 `cargo clippy --workspace --all-features --all-targets` and `cargo fmt --all -- --check` — both clean.
- [ ] T033 Full local CI gate before opening any PR.

---

## Dependencies & ordering

- **T001** (setup) before everything.
- Within each story, tasks are independent except: **T025 (3546)** benefits from doing **T024 (2476)** first (shared TimeZoneDataType source).
- **Phase 10 (T029-T033)** runs LAST — the ledger flip requires every CU's test to exist and pass first.
- Stories US1-US8 are mutually independent and may be worked in any order (P1 first is recommended: US1, US2, US3).

## Parallel execution notes

- `[P]` tasks within a phase touch different files and may run concurrently.
- Non-`[P]` tasks (T003, T004, T005, T008, T011, T012, T020, T025, T026, T028) either share a file with a sibling or make a code change whose test must follow the change — do them sequentially within their story.

## Task count

33 tasks: 1 setup + 27 CU tasks (one per CU) + 5 polish. 19 CU tasks are test-only; 8 are impl-gap (fix + test): T003, T004, T005, T008, T011, T012, T025, T026.
