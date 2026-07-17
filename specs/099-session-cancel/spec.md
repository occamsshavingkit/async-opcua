# Feature Specification: Session Cancel Service Completion

**Feature Branch**: `099-session-cancel`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "Session Cancel service completion: close CU 2190 by making the Cancel service actually reach into the Publish request queue and abort outstanding requests with Bad_RequestCancelledByClient."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Cancel a queued Publish request (Priority: P1)

A client has an outstanding Publish request sitting on the server (waiting
for data, a keep-alive, or a timeout). The client decides it no longer
needs that specific request answered — for example, it is shutting down a
subscription path or replacing the request with a fresh one — and calls
Cancel with the request's `requestHandle`. The server must actually abort
that specific outstanding request, resolving it immediately with
`Bad_RequestCancelledByClient`, and report it in `cancelCount`.

**Why this priority**: This is the entire scope of the feature — Cancel
against real outstanding state, closing the one previously-admitted no-op
gap in the CU backlog.

**Independent Test**: Create a subscription with nothing to report. Send a
Publish request and capture its request handle. Call Cancel with that
handle. Confirm Cancel reports `cancelCount = 1`, and the Publish request
resolves promptly with `Bad_RequestCancelledByClient` instead of waiting
for its normal timeout/keep-alive.

**Acceptance Scenarios**:

1. **Given** a Publish request queued on the server with no data available
   to return, **When** the client calls Cancel with that request's
   `requestHandle`, **Then** the Cancel response reports `cancelCount = 1`
   and the Publish request resolves with `Bad_RequestCancelledByClient`.
2. **Given** no outstanding request matches the given `requestHandle`,
   **When** the client calls Cancel, **Then** the Cancel response reports
   `cancelCount = 0` and the session remains fully usable (no regression to
   existing no-match behavior).
3. **Given** a session with an active Publish request queued, **When** the
   client calls Cancel with an unrelated `requestHandle`, **Then** the
   queued Publish request is NOT cancelled (it stays queued, waiting for
   data/keep-alive/its own timeout).

---

### Edge Cases

- Requests other than Publish (Read, Write, Browse, etc.) complete
  synchronously or within one quick internal round-trip on this server —
  there is no meaningful "outstanding" window for Cancel to interrupt for
  those, so this feature does not attempt to cancel them.
- A `requestHandle` that matches more than one queued Publish request (a
  client re-using a handle, which well-behaved clients should not do) —
  the server cancels every match, per the spec text ("All outstanding
  requests with the matching requestHandle shall be cancelled").

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST track outstanding Publish requests per
  session such that they can be located by `requestHandle`.
- **FR-002**: On receiving a Cancel request, the server MUST cancel every
  queued Publish request for the calling session whose `requestHandle`
  matches the one supplied, resolving each with
  `Bad_RequestCancelledByClient`.
- **FR-003**: The Cancel response's `cancelCount` MUST equal the number of
  requests actually cancelled.
- **FR-004**: Cancelling a `requestHandle` with no match MUST return
  `cancelCount = 0` and MUST NOT be treated as an error.
- **FR-005**: A session MUST remain fully usable for subsequent requests
  after a Cancel call, whether or not anything was cancelled.

### Key Entities

- **Publish request queue**: the server's per-session record of Publish
  requests that have not yet been resolved with data, a keep-alive, or a
  timeout.
- **Cancel request/response**: the requestHandle to match and the count of
  requests actually cancelled.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A queued Publish request with a known `requestHandle` is
  cancelled in 100% of test runs when Cancel is called with that handle.
- **SC-002**: A Cancel call with no matching outstanding request returns
  `cancelCount = 0` in 100% of test runs, with no error and no effect on
  the session.
- **SC-003**: CU 2190 is marked `Implemented` in the project's conformance
  evidence ledger with file:line and test-name citations.

## Assumptions

- Publish is the only request type this server holds outstanding for any
  meaningful duration; other services are synchronous or resolve within
  one quick internal round-trip, so Cancel's real-world scope on this
  server is limited to the Publish queue. This is grounded in
  OPC-10000-4 §5.7.5 ("This Service is used to cancel outstanding Service
  requests") without further restricting which service types a
  conformant server must support cancelling — the CU is satisfied by a
  server correctly cancelling whatever it genuinely holds outstanding.
