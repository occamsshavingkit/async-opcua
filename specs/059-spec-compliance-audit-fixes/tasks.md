# Tasks: Spec Compliance Audit Fixes

**Input**: Design documents from `specs/059-spec-compliance-audit-fixes/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/, quickstart.md

**Status**: 13 of 23 audit findings already resolved by prior work (feature 058). 8 findings remain open (7 full + 1 partial DISC-04).

**Note**: Tasks affecting OPC-UA wire-observable behavior include a spec reference (OPC-10000-X §Y.Z) per the AGENTS.md standing instructions.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions
- Include OPC UA spec reference for behavior-changing tasks

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: No new project structure needed — all crates and modules already exist. Skip.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Configuration change needed before SESSION-06 timeout fix.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T001 Add `min_session_timeout_ms: u64` field to `ServerConfig` in `async-opcua-server/src/config/server.rs`, defaulting to 1 (required by OPC-10000-4 §5.7.2.2 Table 15: "The Server shall provide a timeout greater than 0")

**Checkpoint**: Config field available — user story implementation can now begin

---

## Phase 3: User Story 1 - HIGH-Severity Gaps (Priority: P1)

**Goal**: All 6 HIGH-severity findings (SESSION-01, SESSION-02, SESSION-03, SESSION-07, VIEW-01, SC-01) already resolved in prior work. This phase is verification-only — confirm no regressions and that all P1 fixes are present.

**Independent Test**: `cargo test -p async-opcua-server -- --test-threads=1` passes with no regressions. Code review confirms each fix is present at the cited locations.

- [x] T002 [P] [US1] Verify by code inspection that SESSION-01 (sessionName default "UnnamedSession") is present at `async-opcua-server/src/session/manager.rs:386-393` — OPC-10000-4 §5.7.2.2
- [x] T003 [P] [US1] Verify by code inspection that SESSION-02 (oldest unactivated session eviction) is present at `async-opcua-server/src/session/manager.rs:736-769` — OPC-10000-4 §5.7.2.1
- [x] T004 [P] [US1] Verify by code inspection that SESSION-03 (clientNonce validated against [max(config,32),128]) is present at `async-opcua-server/src/session/manager.rs:492-506` — OPC-10000-4 §5.7.2.2
- [x] T005 [P] [US1] Verify by code inspection that SESSION-07 (X509 userTokenSignature rejection) is present at `async-opcua-server/src/session/manager.rs:1108-1114` — OPC-10000-4 §7.40.5, §5.7.3.2
- [x] T006 [P] [US1] Verify by code inspection that VIEW-01 (RESULT_MASK_IS_FORWARD applied) is present at `async-opcua-server/src/node_manager/view.rs:412-417` — OPC-10000-4 §5.9.2.2
- [x] T007 [P] [US1] Verify by code inspection that SC-01 (CloseSecureChannel audit dispatch) is present at `async-opcua-server/src/session/controller.rs:463-477` — OPC-10000-4 §6.5.5

**Checkpoint**: All P1 (HIGH) findings confirmed resolved, full test suite passes

---

## Phase 4: User Story 2 - MEDIUM-Severity Mismatches (Priority: P2)

**Goal**: Fix the 2 remaining MEDIUM-severity findings: serverNonce runtime validation (debug_assert → runtime check) and BrowseDirection::INVALID rejection. Verify the 3 already-resolved MEDIUM findings (SESSION-08, SC-02, DISC-01).

**Independent Test**: Server rejects BrowseDirection value 3 with `BadBrowseDirectionInvalid`. Server with nonce config outside [32,128] returns error in release builds.

### Implementation for User Story 2

- [x] T008 [US2] Replace `debug_assert!` with runtime error return `Err(StatusCode::BadConfigurationError)` for nonce length validation at `async-opcua-server/src/session/manager.rs:365-369` — OPC-10000-4 §5.7.2.2 Table 15: serverNonce "shall have a length between 32 and 128 bytes inclusive"
- [x] T009 [P] [US2] Add BrowseDirection value 3 (INVALID) rejection returning `BadBrowseDirectionInvalid` in `async-opcua-server/src/session/services/view.rs` before BrowseNode construction — OPC-10000-4 §5.9.2.4 Table 36, §7.5 Table 112
- [x] T010 [P] [US2] Verify by code inspection that SESSION-08 (localeIds preserved on null re-activation) is present at `async-opcua-server/src/session/manager.rs:1297-1306` — OPC-10000-4 §5.7.3.2 Table 17
- [x] T011 [P] [US2] Verify by code inspection that SC-02 (tokenCreatedAt updated on renewal) is present at `async-opcua-server/src/session/controller.rs:1175` — OPC-10000-6 §6.7.4 Table 64
- [x] T012 [P] [US2] Verify by code inspection that DISC-01 (startingRecordId uses `>` not `>=`) is present at `async-opcua-server/src/info.rs:527` — OPC-10000-4 §5.5.3.1

**Checkpoint**: All P2 (MEDIUM) findings resolved or verified

---

## Phase 5: User Story 3 - MINOR/LOW Deviations (Priority: P3)

**Goal**: Fix the 6 remaining LOW/MINOR-severity findings: min session timeout guard, external reference result mask, deferred cleanup documentation, redundant set_role removal, FindServers endpoint filtering, locale filtering. Verify the 5 already-resolved LOW findings.

**Independent Test**: Each fix independently verifiable. Server returns `revisedSessionTimeout > 0`. External browse references respect resultMask. Redundant call removed.

### Implementation for User Story 3

- [x] T013 [US3] Apply `max(min_session_timeout_ms, computed_timeout)` floor in `CreateSessionAllocation::new()` at `async-opcua-server/src/session/manager.rs:378-381` using the config field from T001 — OPC-10000-4 §5.7.2.2 Table 15: "The Server shall provide a timeout greater than 0"
- [x] T014 [P] [US3] Extract result mask field-stripping from `add()` method into private helper `strip_result_mask_fields()` in `async-opcua-server/src/node_manager/view.rs`, then call it from both `add()` and `add_unchecked()` — OPC-10000-4 §5.9.2.2 Table 34: resultMask applies to all ReferenceDescriptions
- [x] T015 [P] [US3] Remove redundant `self.channel.set_role(Role::Server)` at `async-opcua-server/src/session/controller.rs:1181` (role already set at line 200, never changed in between)
- [x] T016 [P] [US3] Add comment at `async-opcua-server/src/session/controller.rs:463-477` documenting that CloseSecureChannel resource release follows the async-drop pattern: keys/nonces are zeroized on drop and sessions time out independently — OPC-10000-6 §7.1.4
- [x] T017 [P] [US3] Add endpoint_url filtering to `registered_application_descriptions()` in `async-opcua-server/src/info.rs:651-669`, using the existing `matches_discovery_endpoint_url` helper as pattern — OPC-10000-12 §5.1: LDS filters by endpoint URL
- [x] T018 [P] [US3] Add locale-aware application_name selection to `find_servers_application_description()` in `async-opcua-server/src/info.rs:672-681`, using the existing `registered_server_application_name()` (info.rs:1395-1418) and `locale_id_matches` helper as pattern — OPC-10000-4 §7.2.4: application_name supports locale selection
- [x] T019 [P] [US3] Verify by code inspection that SESSION-05 (non-null authenticationToken rejected) is present at `async-opcua-server/src/session/manager.rs:486-489` — OPC-10000-4 §5.7.2.2
- [x] T020 [P] [US3] Verify by code inspection that DISC-02 (ECC security_level nonzero) is present at `async-opcua-server/src/config/endpoint.rs:104-105` — OPC-10000-7 §4.8
- [x] T021 [P] [US3] Verify by code inspection that SUB-01 (consistent Duration::from_micros in create/modify paths) is present at `async-opcua-server/src/subscriptions/session_subscriptions.rs:302,340` — OPC-10000-4 §5.14.1.2
- [x] T022 [P] [US3] Verify by code inspection that AS-01 (set_browse_name is pub(crate)) is present at `async-opcua-nodes/src/base.rs:267` — OPC-10000-3 §6.2.7

**Checkpoint**: All remaining P3 findings resolved or verified

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Build verification and final compliance check

- [x] T023 Run `cargo build --workspace` to confirm all changes compile
- [x] T024 Run `cargo test -p async-opcua-server` to confirm no regression
- [x] T025 Run full workspace test suite with `cargo test --workspace`
- [x] T026 Verify all 8 remaining open findings from audit are addressed per quickstart.md verification steps

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 2)**: T001 must complete before T013 (SESSION-06 needs the config field)
- **User Story 1 (Phase 3)**: No dependencies — verification-only, can run immediately. All [P] tasks run in parallel.
- **User Story 2 (Phase 4)**: No dependencies on Phase 2. T008, T009, T010, T011, T012 all [P] run in parallel.
- **User Story 3 (Phase 5)**: T013 depends on T001 (Phase 2). T014-T022 can all run in parallel (different files). T019 verification runs in parallel.
- **Polish (Phase 6)**: Depends on all implementation phases complete

### Within Each User Story

- US1 (Phase 3): All 6 tasks are verification-only [P] — run in parallel
- US2 (Phase 4): All 5 tasks [P] — run in parallel. T008 and T010 touch different line regions in manager.rs (365 vs 1297); execute sequentially if parallelizing on same file.
- US3 (Phase 5): T014-T022 all [P] — run in parallel (11 tasks). T013 runs first (depends on T001). T019-T022 verification runs in parallel with implementation.

### Parallel Opportunities

- Phase 3: All 6 verification tasks run in parallel
- Phase 4: All 5 tasks run in parallel (watch T008/T010 both touch manager.rs)
- Phase 5: T014-T022 (11 tasks) run in parallel after T013 completes
- Phase 6: Sequential build → test → full test → compliance check

---

## Parallel Example: User Story 3

```text
# First: T013 (depends on T001 config field)
Task: "Apply min session timeout floor in manager.rs:378-381"

# Then launch all remaining in parallel:
Task: "Extract result mask stripping helper in view.rs"                    # T014
Task: "Remove redundant set_role in controller.rs:1181"                    # T015
Task: "Document deferred cleanup in controller.rs:463-477"                 # T016
Task: "Add endpoint_url filtering in info.rs:651-669"                      # T017
Task: "Add locale-aware name selection in info.rs:672-681"                 # T018
Task: "Verify SESSION-05 in manager.rs:486-489"                            # T019
Task: "Verify DISC-02 in endpoint.rs:104-105"                              # T020
Task: "Verify SUB-01 in session_subscriptions.rs:302,340"                  # T021
Task: "Verify AS-01 in base.rs:267"                                        # T022
```

---

## Implementation Strategy

### MVP First (User Stories 1 + 2 Only)

1. Complete Phase 2: T001 (config field)
2. Complete Phase 3: Verify all P1 fixes (T002-T007) — already resolved
3. Complete Phase 4: Fix remaining P2 items + verify resolved P2 items (T008-T012)
4. **STOP and VALIDATE**: Run `cargo test --workspace`, verify all HIGH+MEDIUM findings resolved
5. Deploy/demo if ready

### Incremental Delivery

1. Foundation → T001 config field ready
2. Add US1 verification → All P1 confirmed resolved → Foundation confirmed
3. Add US2 fixes + verifications → SESSION-04 + VIEW-02 fixed, all MEDIUM verified → All HIGH+MEDIUM resolved
4. Add US3 fixes → 6 remaining LOW items → All 23 findings resolved
5. Polish → Full build + test suite pass

### Parallel Team Strategy

With multiple agents:

1. Agent A: T001 (config field), then T013 (SESSION-06)
2. Agent B: T008 (SESSION-04) + T009 (VIEW-02) — Phase 4
3. Agent C: T014-T018 (VIEW-03, SC-03, SC-04, DISC-03, DISC-04) — Phase 5
4. Agent D: T002-T007, T010-T012, T019-T022 (all verifications) — Phases 3 + 4 + 5
5. After all complete: T023-T026 (build + test) — Phase 6

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each task is atomic: one file, one concern, independently completable
- Tasks affecting OPC-UA behavior include spec reference: OPC-10000-X §Y.Z
- Verification tasks specify method: "Verify by code inspection that [finding] is present at [file:line]"
- Implementation tasks specify concrete error codes and helper patterns to follow
- Commit after each task or logical group
- Stop at any checkpoint to validate independently
