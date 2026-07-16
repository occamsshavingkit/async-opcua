---

description: "Task list for feature 096: Server Capabilities & Diagnostics Conformance Completion"
---

# Tasks: Server Capabilities & Diagnostics Conformance Completion

**Input**: Design documents from `/specs/096-server-capabilities-diagnostics/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included — this repo's constitution (Principle I, Correctness Over
Completion) requires demonstrable correctness including edge cases, and every
prior speckit feature in this repo has shipped with test coverage per user
story.

**Organization**: Tasks are grouped by user story (P1→P4, matching spec.md's
priority order) so each is independently implementable and testable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US4)
- Every task cites the OPC-10000 Part/§ and/or the generated `VariableId`
  constant it implements against, per this repo's `speckit-tasks-cite-spec-
  sections` convention.

## Path Conventions

Single Rust workspace crate area: `async-opcua-server/src/node_manager/
memory/core.rs`, `async-opcua-server/src/config/limits.rs`, tests in
`async-opcua/tests/integration/read.rs` and `browse.rs`, evidence ledger in
`tools/cu-coverage-report/src/lib.rs`.

---

## Phase 1: Setup

No project-initialization tasks required — this feature extends existing,
already-registered code paths (`node_manager/memory/core.rs`'s attribute
dispatch, no new cargo features).

---

## Phase 2: Foundational

No blocking cross-story prerequisites. Each user story below is
independently implementable.

---

## Phase 3: User Story 1 - ServerCapabilities Max* Node Wiring (Priority: P1) 🎯 MVP

**Goal**: Report the server's real configured/effective value for every
currently-unwired scalar `ServerCapabilities` Max* node, closing CUs 3911
and 3912 (OPC-10000-5 §6.3.2).

**Independent Test**: Configure a server with explicit
`max_sessions`/`max_monitored_item_queue_size`/`max_monitored_items_per_sub`/
`max_subscriptions_per_session` values, read each corresponding
`ServerCapabilities` node over the wire, and confirm each returns the
configured value; confirm `MaxSubscriptions`/`MaxMonitoredItems` return `0`.

### Tests for User Story 1

- [X] T001 [P] [US1] Add integration test in `async-opcua/tests/integration/read.rs` asserting `ServerCapabilities.MaxSessions` (VariableId `Server_ServerCapabilities_MaxSessions` = 24095) reads back the server's configured `Limits.max_sessions` value (OPC-10000-5 §6.3.2). Write to FAIL against current code first (current behavior: static null).
- [X] T002 [P] [US1] Add integration test asserting `ServerCapabilities.OperationLimits`'s `MaxMonitoredItemsQueueSize` node (VariableId `Server_ServerCapabilities_MaxMonitoredItemsQueueSize` = 31916) reads back `SubscriptionLimits.max_monitored_item_queue_size` (already enforced at `monitored_item.rs:314`).
- [X] T003 [P] [US1] Add integration test asserting `MaxMonitoredItemsPerSubscription` reads back `SubscriptionLimits.max_monitored_items_per_sub`, and `MaxSubscriptionsPerSession` reads back `SubscriptionLimits.max_subscriptions_per_session`.
- [X] T004 [P] [US1] Add integration test asserting `MaxSubscriptions` and `MaxMonitoredItems` (the server-wide, non-per-session/per-subscription variants) both read back literal `0`, per OPC-10000-5 §6.3.2's "0 = no limit" and this server having no server-wide cap for either.

### Implementation for User Story 1

- [X] T005 [US1] In `async-opcua-server/src/node_manager/memory/core.rs`'s `get_attribute` match block (the same block wiring `Server_ServerCapabilities_MaxArrayLength` etc., ~line 829), add an arm for `VariableId::Server_ServerCapabilities_MaxSessions` returning `(context.info.config.limits.max_sessions as u32).into()`.
- [X] T006 [US1] In the same match block, add an arm for `VariableId::Server_ServerCapabilities_MaxMonitoredItemsQueueSize` returning `(context.info.config.limits.subscriptions.max_monitored_item_queue_size as u32).into()`.
- [X] T007 [US1] In the same match block, add arms for `VariableId::Server_ServerCapabilities_MaxMonitoredItemsPerSubscription` (→ `limits.subscriptions.max_monitored_items_per_sub`) and `VariableId::Server_ServerCapabilities_MaxSubscriptionsPerSession` (→ `limits.subscriptions.max_subscriptions_per_session`).
- [X] T008 [US1] In the same match block, add arms for `VariableId::Server_ServerCapabilities_MaxSubscriptions` and `VariableId::Server_ServerCapabilities_MaxMonitoredItems`, both returning literal `0u32.into()` with a comment citing OPC-10000-5 §6.3.2 and noting no server-wide cap is enforced (see research.md).
- [X] T009 [US1] Run T001-T004; confirm they now pass. Fix any `VariableId`/field-path mismatches found (confirm the exact struct path for `limits.subscriptions.*` vs top-level `limits.*` against `config/limits.rs`).

**Checkpoint**: US1 fully functional and testable independently. Closes CUs
3911 (Server Capabilities Subscriptions) and 3912 (Server Capabilities 2).

---

## Phase 4: User Story 2 - SamplingIntervalDiagnosticsArray Non-Exposure Rationale (Priority: P2)

**Goal**: Document why `SamplingIntervalDiagnosticsArray` is correctly not
exposed by this server, closing CU 3196 (OPC-10000-5 §7.9/§12.8). **No new
runtime code** — this was corrected from a full diagnostics-array build
during Phase 0 planning; see research.md and plan.md's Summary before
starting.

**Independent Test**: A reviewer reads the documentation note (added as
part of US4's capacity document) and confirms it accurately cites both the
spec's conditional clause and `sanitize_sampling_interval`'s actual
behavior.

### Implementation for User Story 2

- [X] T010 [US2] Re-confirm `async-opcua-server/src/subscriptions/monitored_item.rs`'s `sanitize_sampling_interval` (~line 299-311) still accepts arbitrary continuous `f64` sampling intervals (clamped only to a minimum floor, never snapped to a fixed set) — this is the evidence the non-exposure rationale depends on. If a later change made intervals fixed, this task becomes "implement the array" instead; flag to the user if so.
- [X] T011 [US2] Add a note to `docs/server-capacity-limits.md` (created in US4/T014) explaining `SamplingIntervalDiagnosticsArray` is not exposed because this server negotiates continuously-variable per-monitored-item sampling intervals rather than a fixed set, citing OPC-10000-5 §7.9/§12.8's conditional text and `monitored_item.rs`'s `sanitize_sampling_interval`.

**Checkpoint**: US1 and US2 both independently closed. Closes CU 3196.

---

## Phase 5: User Story 3 - Locations Object Browse Test (Priority: P3)

**Goal**: Prove the standard `Locations` object (already loaded via the
default `CoreNamespace` import) is reachable via Browse, closing CU 4053.

**Independent Test**: Browse from the server object to the `Locations`
object using the standard hierarchical path.

### Tests for User Story 3

- [X] T012 [US3] Add integration test in `async-opcua/tests/integration/browse.rs` browsing from the Server object to the `Locations` object (nodeset_16.rs:918-943) and asserting the browse resolves. If it does NOT resolve, this reveals a real wiring gap in `CoreNamespace` (core.rs:147) rather than the test-only fix assumed in plan.md — investigate and fix the actual wiring in that case.

**Checkpoint**: US1, US2, US3 all independently closed. Closes CU 4053.

---

## Phase 6: User Story 4 - Core Capacity Documentation (Priority: P4)

**Goal**: Publish `docs/server-capacity-limits.md` enumerating the server's
core capacity limits and their defaults, closing CU 3808.

**Independent Test**: A reviewer cross-checks each listed value against
`config/limits.rs`'s `Default` impls.

### Implementation for User Story 4

- [X] T013 [US4] Read `async-opcua-server/src/config/limits.rs`'s `Limits`, `SubscriptionLimits`, and `OperationalLimits` struct fields and their `Default` impls in full, to source accurate current default values.
- [X] T014 [US4] Create `docs/server-capacity-limits.md` enumerating each field from T013 with its default value and how it's configured (config file field / `ServerConfig` builder method), at minimum covering `max_unactivated_sessions_per_channel` (per-channel), `max_sessions`, `max_subscriptions_per_session`, `max_monitored_items_per_sub`, `max_monitored_item_queue_size`, plus the already-wired `OperationalLimits` fields for completeness. Include the US2 non-exposure note (T011) in this same document.
- [X] T015 [US4] Cross-check every value in T014 against the actual `Default` impls; fix any drift found.

**Checkpoint**: All four user stories independently functional. Closes CU
3808.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [X] T016 Run the full existing `async-opcua/tests/integration/read.rs` and `browse.rs` suites; confirm zero regressions.
- [X] T017 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CUs 3911, 3912, 4053, 4055, 3196, 3808 from `Gap`/`Partial` to `Implemented` with file:line/test evidence citations for each.
- [X] T018 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md` via `cargo run -p async-opcua-cu-coverage-report -- <snapshot> <output>` reflecting T017's updates.
- [X] T019 Update `TODO.md`'s conformance backlog entry to reflect these 6 CUs closing.
- [X] T020 Run `cargo clippy --all-targets --all-features` and `cargo fmt --all`, then the project's standard CI gate (`tools/ci-playbook.sh --ci`) before opening a PR — launch detached per this repo's established gotcha (the Bash tool's own backgrounding caps at 10 minutes; the gate takes ~15-20 min).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup / Foundational**: No tasks — proceed directly to User Story 1.
- **US1 (P1)**: No dependencies on other stories. Recommended first (MVP,
  highest CU-count leverage, only story with genuinely new runtime code).
- **US2 (P2)**: Independent of US1, but its single doc-note task (T011)
  writes into the same file US4 creates (T014) — sequence after US4/T014
  creates the file, or write both in the same pass.
- **US3 (P3)**: Fully independent — a standalone test.
- **US4 (P4)**: Independent of US1/US3's own logic; T014 is the file US2's
  T011 appends to.
- **Polish**: After all desired user stories are complete.

### Within Each User Story

- Tests are written first (and should fail against pre-change code) before
  implementation tasks land, for US1/US3.
- US2/US4 are documentation-only; T013 (read defaults) precedes T014
  (write doc) precedes T015 (cross-check).

### Parallel Opportunities

- T001-T004 (US1 tests) can be drafted in parallel (different assertions),
  though they may land in the same test file.
- T005-T008 (US1 `core.rs` match arms) touch the same match block
  sequentially in practice to avoid merge conflicts, though logically
  independent.
- US1, US3 can be worked in parallel by different contributors; US2 and
  US4 share one file so are best done together.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 3 (US1): T001-T009.
2. **STOP and VALIDATE**: run the US1 Independent Test from spec.md /
   quickstart.md.
3. This alone closes the two largest-weighted CUs (3911, 3912) and is the
   only story with genuinely new runtime behavior.

### Incremental Delivery

1. US1 → validate → commit (per this repo's "one commit per user story"
   convention).
2. US4 → validate → commit (creates the doc file US2 needs).
3. US2 → validate → commit (appends to US4's doc).
4. US3 → validate → commit.
5. Polish → PR.

Each story adds value without breaking previously-completed stories.
