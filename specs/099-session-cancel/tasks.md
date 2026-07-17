---

description: "Task list for feature 099: Session Cancel Service Completion"
---

# Tasks: Session Cancel Service Completion

**Input**: Design documents from `/specs/099-session-cancel/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Included, per this repo's constitution (Principle I).

**Organization**: Single user story (P1), independently testable.

## Format: `[ID] [P?] [Story] Description`

- **[Story]**: US1 (only story)
- Every task cites the OPC-10000 Part/§ it implements against.

## Path Conventions

`async-opcua-server/src/subscriptions/{session_subscriptions,actor,mod}.rs`
(implementation); `async-opcua-server/src/session/message_handler.rs`
(Cancel dispatch); test in
`async-opcua/tests/integration/core_tests.rs`; evidence ledger in
`tools/cu-coverage-report/src/lib.rs`.

---

## Phase 1: Setup

No setup tasks — extends existing files only, no new modules.

---

## Phase 2: Foundational

No blocking cross-story prerequisites.

---

## Phase 3: User Story 1 - Cancel a queued Publish request (Priority: P1) 🎯 MVP

**Goal**: Cancel reaches into the per-session Publish request queue and
aborts matching outstanding requests with `Bad_RequestCancelledByClient`,
per OPC-10000-4 §5.7.5. Closes CU 2190.

**Independent Test**: See spec.md.

### Implementation for User Story 1

- [X] T001 [US1] In `session_subscriptions.rs`, add `cancel_publish_requests(&mut self, request_handle: u32) -> u32`, filtering `publish_request_queue` and resolving matches via their `oneshot::Sender` with `ServiceFault(Bad_RequestCancelledByClient)`, mirroring `remove_expired_publish_requests` (OPC-10000-4 §5.7.5).
- [X] T002 [US1] In `subscriptions/actor.rs`, add `SubscriptionCommand::CancelPublishRequests { request_handle, response }`, its command-loop handler, and a public `cancel_publish_requests(&self, request_handle: u32) -> Result<u32, ()>` method, mirroring `RepublishRq`/`republish`.
- [X] T003 [US1] In `subscriptions/mod.rs`, add `SubscriptionCache::cancel_publish_requests(&self, session_id: u32, request_handle: u32) -> Result<u32, StatusCode>`, mirroring `republish`/`set_publishing_mode` (returns `Ok(0)`, not an error, when the session has no active cache).
- [X] T004 [US1] In `session/message_handler.rs`, rewrite the `RequestMessage::Cancel` arm as an `AsyncMessage` (spawned task) that awaits the above and reports the real `cancel_count`, falling back to 0 when the `subscriptions` feature is disabled.

### Tests for User Story 1

- [X] T005 [US1] Add `cancel_aborts_a_queued_publish_request` in `core_tests.rs`: create a subscription with nothing to report, send a raw `Publish` request via `opcua_client::services::Publish`, capture its handle, call `session.cancel(handle)`, assert `cancelCount == 1` and the Publish task resolves with `Bad_RequestCancelledByClient`; confirm the session stays usable afterward.
- [X] T006 [US1] Refresh the doc comment on the existing `cancel_is_a_clean_noop` test (no longer describes Cancel as a universal no-op); confirm it still passes unmodified (its assertion — cancelling an unused handle returns 0 — is still correct, since nothing matches).
- [X] T007 [US1] Run T005 and T006; confirm both pass.

**Checkpoint**: Closes CU 2190.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [X] T008 Run the full `core_tests.rs`/`subscriptions.rs` suites plus the full integration suite and `async-opcua-server` lib suite; confirm zero regressions.
- [X] T009 [P] Update `tools/cu-coverage-report/src/lib.rs`'s `AUDIT_TABLE` for CU 2190 from `Gap` to `Implemented` with file:line/test evidence.
- [X] T010 [P] Regenerate `specs/conformance-tester/CU-COVERAGE.md`.
- [X] T011 Update `TODO.md`'s conformance backlog to reflect CU 2190 closing.
- [X] T012 Run `cargo clippy --all-targets --all-features`, `cargo fmt --all`, then the full CI gate (`tools/ci-playbook.sh --ci`, launched detached).

---

## Dependencies & Execution Order

Single user story; tasks are sequential within it (T001→T004 are additive
layers of the same call chain, T005-T007 verify, T008-T012 polish).

## Implementation Strategy

1. T001-T004 (implementation) → T005-T007 (test) → validate → commit.
2. Polish → PR.
