# Quickstart: Session Cancel Service Completion

## Verify CU 2190 (real Publish cancellation)

1. Connect a session and create a subscription with nothing to report
   (long publishing interval, huge keep-alive count, no monitored items).
2. Build a raw `Publish` request via
   `opcua_client::services::Publish::new(&session)`, read its
   `request_handle` from `.header().request_handle`, and send it on a
   background task via `session.channel()`.
3. Call `session.cancel(handle)` (retrying briefly until the request has
   reached the server's queue). Expect `cancelCount == 1`.
4. Expect the background Publish task to resolve promptly with
   `Bad_RequestCancelledByClient`, not the request's normal timeout.
5. Confirm the session is still fully usable afterward (a plain Read
   succeeds).

## Regression check

1. Call `session.cancel(some_unused_handle)` with no matching outstanding
   request. Expect `cancelCount == 0`, no error, session still usable
   (`cancel_is_a_clean_noop`).

## Full verification

```bash
cargo test -p async-opcua --test integration_tests -- integration::core_tests::cancel
cargo test -p async-opcua --test integration_tests -- integration::subscriptions::
cargo test -p async-opcua-server --lib --all-features
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
tools/ci-playbook.sh --ci
```
