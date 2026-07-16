# Quickstart: Server Capabilities & Diagnostics Conformance Completion

## Verify US1 (ServerCapabilities Max* wiring)

```bash
cargo test -p async-opcua --test integration_tests -- server_capabilities
```

Connect any OPC UA client, read `ServerCapabilities.MaxSessions` and
`ServerCapabilities.OperationLimits` sub-nodes (or wherever
`MaxMonitoredItemsQueueSize`/`MaxMonitoredItemsPerSubscription`/
`MaxSubscriptionsPerSession` resolve in the address space) and confirm each
matches the server's configured `Limits`/`SubscriptionLimits` values rather
than null.

## Verify US2 (SamplingIntervalDiagnosticsArray non-exposure rationale)

No test to run — read the note in `docs/server-capacity-limits.md`
explaining why this array isn't exposed, and cross-check it against
`async-opcua-server/src/subscriptions/monitored_item.rs`'s
`sanitize_sampling_interval` (confirms continuously-variable, not fixed,
sampling intervals) and OPC-10000-5 §7.9/§12.8's conditional text.

## Verify US3 (Locations object)

```bash
cargo test -p async-opcua --test integration_tests -- locations
```

Browse from the server root to the standard `Locations` object path and
confirm it resolves.

## Verify US4 (capacity document)

Read `docs/server-capacity-limits.md` and cross-check each listed value
against `async-opcua-server/src/config/limits.rs`'s `Default` impls.

## Full gate

```bash
tools/ci-playbook.sh --ci    # launch detached per this repo's established gotcha
```
