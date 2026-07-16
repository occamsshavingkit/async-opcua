# Phase 0 Research: Server Capabilities & Diagnostics Conformance Completion

## US1 — ServerCapabilities Max* node inventory

**Decision**: Cross-referenced every generated `Server_ServerCapabilities_*`
`VariableId` (39 total) against what `node_manager/memory/core.rs`'s
`get_attribute` match block currently wires (line ~828 onward). 19 are
unwired; of those, most are non-scalar structural/method nodes already
handled elsewhere (`RoleSet`/`RoleSet_AddRole*`/`RemoveRole*` via
`role_management.rs`; `AggregateFunctions` via `add_aggregates`;
`ModellingRules`/`SoftwareCertificates`/`ConformanceUnits`/
`OperationLimits` itself is the parent Object, not a scalar). The
genuinely-unwired scalar limits, and their config source:

| Node | Config field | Status |
|---|---|---|
| `MaxSessions` | `Limits.max_sessions: usize` (config/limits.rs:53) | wire directly |
| `MaxMonitoredItemsQueueSize` | `SubscriptionLimits.max_monitored_item_queue_size: usize` (config/limits.rs:127) — already enforced at `monitored_item.rs:314` | wire directly |
| `MaxMonitoredItemsPerSubscription` | `SubscriptionLimits.max_monitored_items_per_sub: usize` (config/limits.rs:124) | wire directly |
| `MaxSubscriptionsPerSession` | `SubscriptionLimits.max_subscriptions_per_session: usize` (config/limits.rs:102) | wire directly |
| `MaxSubscriptions` | *(no field — no server-wide cap enforced)* | report `0` (Part 5 §6.3.2: 0 = no limit) |
| `MaxMonitoredItems` | *(no field — no server-wide cap enforced)* | report `0` (same) |
| `MaxSelectClauseParameters`/`MaxWhereClauseParameters` | tracked by separate CU 3194, out of this feature's scope | leave as-is |

**Rationale**: `MaxSubscriptions`/`MaxMonitoredItems` (the un-suffixed,
server-wide totals) have no corresponding enforcement anywhere in the
codebase — only the per-session/per-subscription variants are tracked and
enforced. Per OPC-10000-5 §6.3.2, `0` is the spec-defined value meaning "no
limit," which is the *honest* value here — inventing a non-zero number
without matching enforcement would violate Constitution Principle I
(reporting a value the server doesn't actually honor). Adding real
server-wide caps is a separate, larger feature (not requested, not part of
any of the 6 target CUs) and is out of scope.

**Alternatives considered**: Adding new `Limits` fields for
server-wide `max_subscriptions`/`max_monitored_items` and enforcing them —
rejected as scope creep beyond what CU 3911/3912 actually require (report
the *capability*, not invent new enforcement); the CU's own description
in the audit register is about exposing already-true information, not
adding new limits.

## US2 — SamplingIntervalDiagnosticsArray (major correction from the
original task framing — read this before implementing)

**Decision**: Per OPC-10000-5 §7.9/§12.8, `SamplingIntervalDiagnosticsArray`
is explicitly conditional: "The sampling interval diagnostics are only
collected by Servers which use a fixed set of sampling intervals ... the
NodeId assigned to a given sampling interval diagnostics variable shall
not change as long as the Server configuration does not change. **A Server
may not expose the SamplingIntervalDiagnosticsArray if it does not use
fixed sampling rates.**" CU 3196's own description confirms this
precondition verbatim: "when the Server is handling subscriptions with
fixed sampling intervals."

Checked `async-opcua-server/src/subscriptions/monitored_item.rs:299-311`
(`sanitize_sampling_interval`): this server accepts *any* client-requested
sampling interval as a continuous `f64`, clamped only to a configured
minimum floor (`min_sampling_interval_ms`) — never snapped to a fixed,
pre-configured set of allowed rates. This server's sampling-interval model
is the opposite of what the CU requires as a precondition: it is
continuously variable, not fixed.

**Revised scope for US2**: There is no new diagnostics array to build. The
correct, spec-conformant resolution is to *not* expose
`SamplingIntervalDiagnosticsArray` (the spec explicitly permits this) and
close CU 3196 as satisfied-by-inapplicability, with evidence citing both
the spec's own conditional text and `sanitize_sampling_interval`'s
continuous-interval behavior as proof the precondition doesn't hold. This
replaces the original plan.md/spec.md framing (build a new live-computed
array) with a documentation-only task: add a short doc note (in the same
capacity document as US4, or a comment at the relevant EnabledFlag/
diagnostics site) recording *why* this array is absent, so a future reader
doesn't mistake the absence for an unnoticed gap.

**Why this wasn't caught before planning**: The original task framing
(and the prior audit's own evidence text, "no SamplingIntervalDiagnosticsArray,
no EnabledFlag gating, no test") treated the missing array as pure gap
without checking the CU's own conditional precondition against this
server's actual sampling-interval negotiation code — the same category of
mistake feature 095 corrected repeatedly (assuming a CU's literal name
implies unconditional applicability without reading its full description).

**Alternatives considered**: Building the array anyway (grouping live
monitored items by their currently-in-use interval value, dynamically) —
rejected: with continuously-variable intervals, the NodeId-per-entry
churns as new interval values appear, directly violating the spec's own
stability requirement ("NodeId... shall not change as long as the Server
configuration does not change"); implementing it would produce a
technically-present but spec-*non-conformant* array, which is worse than
correctly omitting it.

## US3 — Locations object

**Decision**: Test-only. The `Locations` Object (nodeset_16.rs:918-943) is
already loaded via the default `CoreNamespace` import (core.rs:147); no
code change identified as necessary. A `browse.rs` integration test proving
the standard hierarchical path resolves closes this CU per the audit's own
prior note ("no test browses to it").

**Rationale**: The audit's own evidence for this CU already states the
node and its wiring exist; re-verifying via `grep` during Phase 0 for this
plan turned up no additional gap. If task-time investigation finds the
path doesn't actually resolve (audit evidence proven wrong, as happened in
feature 095), this becomes a real fix — but the starting assumption is
test-only.

## US4 — Core capacity document

**Decision**: A new `docs/server-capacity-limits.md` enumerating each
`Limits`/`SubscriptionLimits`/`OperationalLimits` field from
`config/limits.rs`, its default value (from that struct's `Default` impl),
and how it's configured (`ServerConfig`/builder method). At minimum: max
secure channels (`max_unactivated_sessions_per_channel`/channel-level
limits), `max_sessions`, `max_subscriptions_per_session`,
`max_monitored_items_per_sub`, `max_monitored_item_queue_size`, plus the
already-wired `OperationalLimits` fields for completeness.

**Rationale**: CU 3808 requires a documentation artifact, not a runtime
API — matches this repo's existing precedent of `docs/*.md` files for
similar completeness requirements (e.g. `docs/time-synchronization.md` for
feature 093). Generating it from the actual struct definitions and their
`Default` impls (rather than hand-copying numbers) keeps it from drifting
stale, per SC-004's "verified to match" requirement.

**Alternatives considered**: A doc-test or build-time check asserting the
doc's numbers match the `Default` impl — considered for extra rigor but
deferred; SC-004 only requires the values to currently match, verifiable
by a reviewer/test at implementation time without new tooling.
