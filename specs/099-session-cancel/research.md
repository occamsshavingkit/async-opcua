# Research: Session Cancel Service Completion

## CU 2190 — Session Cancel

**Prior AUDIT_TABLE entry** (Gap): "Cancel service is a deliberate no-op:
message_handler.rs:435-457 dispatches CancelRequest and audits it, but the
comment admits 'this server processes requests without a cancellable
queue, so there is nothing outstanding to cancel' — cancel_count is always
0, no in-flight request is ever actually aborted."

## Spec grounding (OPC-10000-4 §5.7.5, Cancel)

> This Service is used to cancel outstanding Service requests. Successfully
> cancelled service requests shall respond with Bad_RequestCancelledByClient.

Parameters (Table 21): request carries `requestHandle` ("The requestHandle
assigned to one or more requests that should be cancelled. All outstanding
requests with the matching requestHandle shall be cancelled."); response
carries `cancelCount` (UInt32, "Number of cancelled requests.").

The spec does not enumerate which service types a server must be able to
cancel — it is framed generically over "outstanding Service requests."

## What "outstanding" means in this server's architecture

This server processes almost every request synchronously or within one
quick round-trip through a node manager / per-session actor — there is no
meaningful window during which most requests are "in flight" long enough
for a client to usefully race a Cancel against them.

The one deliberate exception is **Publish**: a client sends a Publish
request, and the server holds it — queued in
`SessionSubscriptions::publish_request_queue`
(`async-opcua-server/src/subscriptions/session_subscriptions.rs`) — until
one of:
- Data becomes available to report,
- A keep-alive interval elapses, or
- The request's own deadline expires (`remove_expired_publish_requests`,
  which already resolves timed-out entries with `Bad_Timeout`).

This is precisely the "outstanding Service request" scenario Cancel exists
for, and this codebase already has all the machinery to resolve a queued
Publish request out-of-band (the `remove_expired_publish_requests` timeout
path is a direct precedent — it filters the queue and completes evicted
entries via their `oneshot::Sender<ResponseMessage>`).

**Decision**: Implement Cancel by adding a
`SessionSubscriptions::cancel_publish_requests(request_handle) -> u32`
method (mirroring `remove_expired_publish_requests`'s filter-and-resolve
shape) that removes every queued `PendingPublish` whose
`request.request_header.request_handle` matches, resolves each via its
`oneshot::Sender` with `ServiceFault::new(&req.request.request_header,
StatusCode::BadRequestCancelledByClient)`, and returns the count. Wire it
through the existing per-session actor command pattern
(`SubscriptionCommand`, `async-opcua-server/src/subscriptions/actor.rs`)
and the `SubscriptionCache` (`async-opcua-server/src/subscriptions/mod.rs`),
matching `set_publishing_mode`/`republish` exactly. Rewrite the
`RequestMessage::Cancel` arm in
`async-opcua-server/src/session/message_handler.rs` as an async spawned
task (like `SetPublishingMode`/`ModifySubscription`) that awaits this call
and reports the real count.

**Alternatives considered**:
- A global (cross-session) cancellable-request registry for every service
  type. Rejected: massive surface increase for zero real gain, since no
  other service type in this server holds a request outstanding for a
  duration a client could plausibly race against — the spec's own
  motivating scenario (long-running Publish) is already fully covered.
- Hard-aborting the tokio task processing a request via `AbortHandle`.
  Considered for genericity, but Publish requests are not backed by a
  standalone task while queued — they sit as data (`PendingPublish`) in
  `publish_request_queue`, resolved by a plain channel send rather than a
  task being polled. Filtering the queue and firing the channel directly
  (the same technique the existing timeout path already uses) is simpler,
  requires no new task-tracking machinery, and is fully sufficient per
  spec.

## Test approach

The high-level client `Session` API auto-manages its own Publish loop
internally and does not expose the request handles it allocates. To get a
controllable `requestHandle`, the test constructs its own `Publish` request
directly via `opcua_client::services::Publish::new(&session)` (a public
`UARequest` builder already used elsewhere in this test suite, e.g.
`subscriptions.rs`), reads the handle back via the builder's public
`.header()` accessor before sending, sends it on a background task via
`session.channel()` (also public), and then calls the existing public
`session.cancel(handle)` client method (already exercised by
`cancel_is_a_clean_noop`) to cancel it. This exercises the exact same
server-side Cancel code path a real client would hit.
