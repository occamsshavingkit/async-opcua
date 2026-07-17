# Data Model: Session Cancel Service Completion

No new persistent entities. One new command variant and one new method on
existing types:

## `SubscriptionCommand::CancelPublishRequests` (new, `subscriptions/actor.rs`)

```rust
CancelPublishRequests {
    request_handle: u32,
    response: oneshot::Sender<u32>,
}
```

Sent by `SubscriptionCache::cancel_publish_requests(session_id,
request_handle)` to the target session's actor; the actor answers with the
number of Publish requests it cancelled.

## `SessionSubscriptions::cancel_publish_requests` (new, `session_subscriptions.rs`)

```rust
pub(crate) fn cancel_publish_requests(&mut self, request_handle: u32) -> u32
```

Filters `publish_request_queue: VecDeque<PendingPublish>`, removing and
resolving (via each entry's `oneshot::Sender<ResponseMessage>`, with
`ServiceFault(Bad_RequestCancelledByClient)`) every entry whose
`request.request_header.request_handle` matches. Returns the count
removed. Structurally mirrors the existing
`remove_expired_publish_requests` timeout-eviction method.

## `SubscriptionCache::cancel_publish_requests` (new, `subscriptions/mod.rs`)

```rust
pub(crate) async fn cancel_publish_requests(
    &self,
    session_id: u32,
    request_handle: u32,
) -> Result<u32, StatusCode>
```

Looks up the session's actor handle (returning `Ok(0)`, not an error, if
the session has no active subscription cache — nothing to cancel is not a
failure) and dispatches the command, mirroring `republish`/
`set_publishing_mode`.

## `RequestMessage::Cancel` handling (rewritten, `session/message_handler.rs`)

Was a synchronous no-op returning `cancel_count: 0` unconditionally. Now an
`AsyncMessage` (spawned task) that awaits
`subscriptions.cancel_publish_requests(session_id, request.request_handle)`
and reports the real count, falling back to `0` when the `subscriptions`
feature is compiled out (Cancel remains a valid base Session service
either way).
