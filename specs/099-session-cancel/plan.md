# Implementation Plan: Session Cancel Service Completion

**Branch**: `099-session-cancel` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/099-session-cancel/spec.md`

## Summary

Close CU 2190 (the one required-CU item explicitly flagged as needing real
architectural work) by making the Cancel service reach into this server's
one genuinely long-lived outstanding-request state — the per-session
Publish request queue — and abort matching requests with
`Bad_RequestCancelledByClient`, per OPC-10000-4 §5.7.5.

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`, stable toolchain)
**Primary Dependencies**: `async-opcua-server` (session message handler, subscriptions actor/cache), `async-opcua-client` (raw `Publish`/`Cancel` UARequest builders for the test), `tokio` (per-session actor + async test runtime)
**Storage**: N/A (in-memory per-session Publish request queue; no persistent storage)
**Testing**: `cargo test -p async-opcua --test integration_tests -- integration::core_tests::cancel` (new real-cancellation test, alongside the existing no-op-case test)
**Target Platform**: Cross-platform Rust library/server (Linux CI primary)
**Project Type**: Library (OPC UA server SDK) — single Cargo workspace
**Performance Goals**: N/A (correctness/conformance fix)
**Constraints**: Must not affect any other request type's handling; must not regress the existing "no match → cancelCount 0" behavior; must work whether or not the `subscriptions` feature is enabled (Cancel is a base Session service, not gated behind subscriptions)
**Scale/Scope**: 1 conformance unit; one new method on the per-session subscriptions cache/actor, one new actor command, one rewritten message-handler arm, one new integration test

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Grounded against OPC-10000-4 §5.7.5
  ("Successfully cancelled service requests shall respond with
  Bad_RequestCancelledByClient") before implementing. PASS.
- **II. Do It Right Once**: Reuses the existing per-session subscription
  actor's command-channel pattern (mirroring `SetPublishingModeRq`/
  `RepublishRq`) rather than inventing a new mechanism; reuses the same
  fault-and-requeue pattern already used by `remove_expired_publish_requests`.
  PASS.
- **III. Individual Task Discipline**: Single user story, single
  independent test. PASS.
- **IV. Security Is Paramount**: Cancel is scoped strictly to the calling
  session's own Publish queue (looked up by the session's internal
  `session_id`, never cross-session) — a client cannot cancel another
  session's requests. PASS.
- **V. Leave It Better Than You Found It**: Updates `AUDIT_TABLE` and
  `CU-COVERAGE.md`, and refreshes the now-stale doc comment on the
  existing `cancel_is_a_clean_noop` test. PASS.

No violations; Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/099-session-cancel/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/subscriptions/session_subscriptions.rs   # cancel_publish_requests(): scans/evicts publish_request_queue
async-opcua-server/src/subscriptions/actor.rs                    # SubscriptionCommand::CancelPublishRequests + handler + public async method
async-opcua-server/src/subscriptions/mod.rs                      # SubscriptionCache::cancel_publish_requests(): session_id -> actor dispatch
async-opcua-server/src/session/message_handler.rs                # Cancel arm rewritten as an async task calling the above
async-opcua/tests/integration/core_tests.rs                       # new cancel_aborts_a_queued_publish_request test
tools/cu-coverage-report/src/lib.rs                                # AUDIT_TABLE evidence for CU 2190
specs/conformance-tester/CU-COVERAGE.md                            # regenerated
TODO.md                                                             # backlog entry closed
```

**Structure Decision**: No new modules. Extends the existing per-session
subscription actor (`subscriptions/actor.rs`) with one new command,
mirroring the established `SetPublishingModeRq`/`RepublishRq` pattern
exactly, so the change is a small, idiomatic addition rather than a new
subsystem.

## Complexity Tracking

*No violations — section not needed.*
