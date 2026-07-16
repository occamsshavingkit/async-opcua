# Data Model: Server Capabilities & Diagnostics Conformance Completion

No new persistent entities or storage schema. This feature exposes
existing in-memory configuration and subscription state through the
standard OPC UA information model; the "entities" below are informational
model concepts, not new Rust types beyond what's noted.

## ServerCapabilities Max* Node → Config Field Mapping

| Standard node | Type | Source | New code |
|---|---|---|---|
| `ServerCapabilities.MaxSessions` | UInt32 | `Limits.max_sessions` | `core.rs` match arm |
| `ServerCapabilities.OperationLimits`/... `MaxMonitoredItemsQueueSize` | UInt32 | `SubscriptionLimits.max_monitored_item_queue_size` | `core.rs` match arm |
| `ServerCapabilities.MaxMonitoredItemsPerSubscription` | UInt32 | `SubscriptionLimits.max_monitored_items_per_sub` | `core.rs` match arm |
| `ServerCapabilities.MaxSubscriptionsPerSession` | UInt32 | `SubscriptionLimits.max_subscriptions_per_session` | `core.rs` match arm |
| `ServerCapabilities.MaxSubscriptions` | UInt32 | *(none — no server-wide cap)* | `core.rs` match arm returning literal `0` |
| `ServerCapabilities.MaxMonitoredItems` | UInt32 | *(none — no server-wide cap)* | `core.rs` match arm returning literal `0` |

No validation rules beyond "value must equal what the server actually
enforces" (or the spec-valid `0` sentinel for uncapped totals) — these are
read-only, server-computed values, not client-writable.

## SamplingIntervalDiagnosticsArray Non-Exposure

No new entity: per research.md's Phase 0 correction, this array is not
built. `SamplingIntervalDiagnosticsDataType` (OPC-10000-5 §12.8) would have
had 4 fields (`SamplingInterval`, `SampledMonitoredItemsCount`,
`MaxSampledMonitoredItemsCount`, `DisabledMonitoredItemsSamplingCount`) had
it applied — noted here only so a future revisit (if this server ever
gains a fixed-sampling-interval mode) knows the full shape required.

## Core Capacity Document Entries

Each row of `docs/server-capacity-limits.md` maps directly to one
`config/limits.rs` struct field: name, current default (from that struct's
`Default` impl), and how it's set (config file field / `ServerConfig`
builder method). No new struct — purely a documentation artifact over the
existing `Limits`/`SubscriptionLimits`/`OperationalLimits` types.
