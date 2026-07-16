# Server Capacity Limits (OPC UA "Documentation Server Facet")

This document enumerates this server's core capacity limits, their
built-in defaults, and how each is configured. It resolves `TODO.md`'s
conformance backlog for CU 3808 (Documentation - Core Capacities), part of
the required-CU backlog for the Micro/Embedded/Standard 2025 server
profiles (feature 096).

All limits below are fields of `async-opcua-server`'s `Limits` struct
(`async-opcua-server/src/config/limits.rs`), set via the `limits:` section
of a server's config file (see `samples/server.conf`) or programmatically
via `ServerBuilder::limits`/`ServerHandle::limits_mut`. Defaults shown are
this crate's built-in `Limits::default()` values (also asserted against
`samples/server.conf` by `limits.rs`'s own
`server_conf_limits_match_struct_field_names` test).

## Connection & session limits

| Limit | Default | Field |
|---|---:|---|
| Max unactivated sessions per secure channel | 5 | `max_unactivated_sessions_per_channel` |
| Unactivated session timeout | 10,000 ms | `unactivated_session_timeout_ms` |
| Max registered sessions (total) | 20 | `max_sessions` |
| Max in-flight requests per connection | 512 | `max_inflight_requests_per_connection` |
| Max registered servers (local discovery/`RegisterServer` registry) | 1,000 | `max_registered_servers` |

This server does not impose a separate "max secure channels" cap
independent of the above; channel-level admission is governed by
`max_unactivated_sessions_per_channel`/`unactivated_session_timeout_ms`
plus transport-level connection handling.

## Message & encoding limits

| Limit | Default | Field |
|---|---:|---|
| Max array length (elements) | 100,000 | `max_array_length` |
| Max string length (characters) | 65,535 | `max_string_length` |
| Max byte string length (bytes) | 65,535 | `max_byte_string_length` |
| Max message size (bytes) | 327,675 | `max_message_size` |
| Max chunk count | 5 | `max_chunk_count` |
| Send buffer size (bytes) | 65,535 | `send_buffer_size` |
| Receive buffer size (bytes) | 65,535 | `receive_buffer_size` |

## Continuation points

| Limit | Default | Field |
|---|---:|---|
| Max browse continuation points | 5,000 | `max_browse_continuation_points` |
| Max history continuation points | 500 | `max_history_continuation_points` |
| Max query continuation points | 500 | `max_query_continuation_points` |

## Subscription & monitored-item limits (`Limits.subscriptions`)

| Limit | Default | Field |
|---|---:|---|
| Max subscriptions per session | 100 | `subscriptions.max_subscriptions_per_session` |
| Max monitored items per subscription | 10,000 | `subscriptions.max_monitored_items_per_sub` |
| Max monitored-item queue size | 10 | `subscriptions.max_monitored_item_queue_size` |
| Max pending publish requests per session | 20 | `subscriptions.max_pending_publish_requests` |
| Max publish requests per session, per subscription | 4 | `subscriptions.max_publish_requests_per_subscription` |
| Max `KeepAliveCount` | 30,000 | `subscriptions.max_keep_alive_count` |
| Default `KeepAliveCount` (client sets 0) | 10 | `subscriptions.default_keep_alive_count` |
| Max lifetime count | 90,000 | `subscriptions.max_lifetime_count` |
| Max notifications per publish | 1,000 | `subscriptions.max_notifications_per_publish` |
| Max queued notifications per subscription | 20 | `subscriptions.max_queued_notifications` |
| Min sampling interval | 100.0 ms | `subscriptions.min_sampling_interval_ms` |
| Min publishing interval | 100.0 ms | `subscriptions.min_publishing_interval_ms` |

This server has no server-wide (across all sessions) cap on total
subscriptions or total monitored items — only the per-session
(`MaxSubscriptionsPerSession`) and per-subscription
(`MaxMonitoredItemsPerSubscription`) limits above are enforced. Per
OPC-10000-5 §6.3.2, `ServerCapabilities.MaxSubscriptions` and
`.MaxMonitoredItems` therefore correctly report `0` ("no limit") rather
than a fabricated total (feature 096, CUs 3911/3912).

### Why `SamplingIntervalDiagnosticsArray` is not exposed

OPC-10000-5 §7.9/§12.8 make `ServerDiagnostics.SamplingIntervalDiagnosticsArray`
conditional: *"The sampling interval diagnostics are only collected by
Servers which use a fixed set of sampling intervals ... A Server may not
expose the SamplingIntervalDiagnosticsArray if it does not use fixed
sampling rates."* This server negotiates a continuously-variable,
client-requested sampling interval per monitored item — see
`sanitize_sampling_interval` in
`async-opcua-server/src/subscriptions/monitored_item.rs`, which clamps
only to `min_sampling_interval_ms` and otherwise accepts any client-
requested value — so the array's precondition never holds. Not exposing
it is the spec-conformant choice, not a gap (feature 096, CU 3196).

## Service-call operation limits (`Limits.operational`)

| Limit | Default | Field |
|---|---:|---|
| Max nodes per TranslateBrowsePathsToNodeIds call | 100 | `operational.max_nodes_per_translate_browse_paths_to_node_ids` |
| Max nodes per Read call | 10,000 | `operational.max_nodes_per_read` |
| Max nodes per Write call | 10,000 | `operational.max_nodes_per_write` |
| Max nodes per Call (method) call | 100 | `operational.max_nodes_per_method_call` |
| Max nodes per Browse call | 1,000 | `operational.max_nodes_per_browse` |
| Max nodes per RegisterNodes call | 1,000 | `operational.max_nodes_per_register_nodes` |
| Max nodes per create/modify/delete MonitoredItems call | 1,000 | `operational.max_monitored_items_per_call` |
| Max nodes per HistoryRead (data) call | 100 | `operational.max_nodes_per_history_read_data` |
| Max nodes per HistoryRead (events) call | 100 | `operational.max_nodes_per_history_read_events` |
| Max nodes per HistoryUpdate call | 100 | `operational.max_nodes_per_history_update` |
| Max references per node during Browse | 1,000 | `operational.max_references_per_browse_node` |
| Max node descriptions per Query call | 100 | `operational.max_node_descs_per_query` |
| Max data sets returned per node (Query) | 1,000 | `operational.max_data_sets_query_return` |
| Max references per data set (Query) | 100 | `operational.max_references_query_return` |
| Max nodes per AddNodes/DeleteNodes call | 1,000 | `operational.max_nodes_per_node_management` |
| Max references per AddReferences/DeleteReferences call | 1,000 | `operational.max_references_per_references_management` |
| Max subscriptions per create/modify/delete Subscriptions call | 10 | `operational.max_subscriptions_per_call` |

## Configuring these limits

Set any of the above under the `limits:` key of a server's YAML config
file (see `samples/server.conf` and `samples/profiles/*.conf` for the
per-profile presets this repository ships), or programmatically:

```rust
let mut server = ServerBuilder::new()
    // ...
    .limits(Limits { max_sessions: 100, ..Default::default() })
    .build()?;
```

or at runtime via `ServerHandle::limits_mut()`.
