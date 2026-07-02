# Data Model: OPC UA 2017 Profile Minimal Builds (054)

No runtime data model. The entities are build-surface artifacts and their invariants.

## Subsystem gates (async-opcua-server features, all default ON)

| Gate | Requires | Excludes when off | Fail-closed behavior when off |
|------|----------|-------------------|-------------------------------|
| `subscriptions` | — | `subscriptions/` module, publish machinery, monitored items, subscription dispatch arms, `CreateMonitoredItem` trait surface | subscription services → `BadServiceUnsupported` |
| `subscriptions-standard` | `subscriptions` | deadband eval, triggering, standard tiers | deadband filter → `Bad_MonitoredItemFilterUnsupported`; SetTriggering → `BadServiceUnsupported` |
| `events` | `subscriptions` | event monitored items, event-filter engine, notify_events | event filter → `Bad_MonitoredItemFilterUnsupported` |
| `alarms` | `events`, `method-call` | A&C conditions/alarms/shelving/branching, condition methods | methods absent; nodes absent |
| `method-call` | — | Call service, method registration | Call → `BadServiceUnsupported` |
| `history` | — | HistoryRead/Update, backend trait, in-memory stores | history services → `BadServiceUnsupported` |
| `history-aggregates` | `history` | Part-13 aggregate engine | ReadProcessed → `Bad_AggregateNotSupported` |
| `query` | — | QueryFirst/QueryNext | → `BadServiceUnsupported` |
| `node-management` | — | AddNodes/AddReferences/DeleteNodes/DeleteReferences | → `BadServiceUnsupported` |
| `diagnostics` | — | diagnostics node manager, ServerDiagnostics wiring | nodes absent |
| `rbac` | — | rbac module, role methods, RoleResolver (stubbed) | enforcement config rejected at validation |
| `gds` | `method-call` | GDS cert-management/push/pull methods | methods absent |
| `fota` | `method-call` | firmware-update model | nodes/methods absent |
| `programs` | `method-call` | Part-10 programs | nodes/methods absent |
| `lds` | — | receiving RegisterServer/RegisterServer2 + registry | → `BadServiceUnsupported` |

Existing gates reused: `generated-address-space`, `discovery-server-registration`
(client-side; Standard rung), `discovery-mdns`, `ecc`, `wss`, `json`, `legacy-crypto`,
crypto backends.

Invariant: every gate is additive; any combination compiles; full set == today's surface.

## Profile compositions (facade aliases)

| Alias | = | New compiled surface vs previous rung |
|-------|---|----------------------------------------|
| `nano` | `base-server` w/ no subsystem gates | (floor) |
| `micro` | `nano` + `subscriptions` | basic data-change subscriptions |
| `embedded` | `micro` + `subscriptions-standard`, `method-call`, `generated-address-space`, `aws-lc-rs` | deadband/triggering, Call (GetMonitoredItems/ResendData), type system, real security |
| `standard` | `embedded` + `discovery-server-registration` | LDS self-registration (X509 tokens + Cancel already in core) |

`base-server` / `server` forward ALL gates (meaning unchanged).

## Benchmark samples (one consumer per composition)

| Sample crate | Alias | Capacity config (profile CU minimums) |
|--------------|-------|----------------------------------------|
| foundation-profile-nano-server | `nano` | ≥1 session |
| foundation-profile-micro-server | `micro` | ≥2 sessions, ≥1 subscription, ≥2 monitored items |
| foundation-profile-embedded-server | `embedded` | ≥2 subscriptions, ≥100 monitored items |
| foundation-profile-standard-server (NEW) | `standard` | ≥50 sessions, ≥5 subscriptions, ≥500 monitored items |

## Guard matrix (CI, per profile row)

1. Dependency guard: `cargo tree` deny-list per row (e.g. nano: no
   `async-opcua-core-namespace`, no aws-lc-rs).
2. Feature guard: resolved features of `async-opcua-server` must equal the composition.
3. Symbol spot-check: one sentinel symbol per big gated subsystem must be absent
   (subscriptions, alarms, history) in rows that exclude it.
4. Size: measured per isolated invocation; appended to `$GITHUB_STEP_SUMMARY`.

## Size matrix (documentation artifact)

Rows: nano, micro, embedded, standard, minimal-server, simple-server(full). Fields:
package, alias/features, bytes, MiB, delta-to-previous-rung. Provenance: arch, cargo
profile, rustc version, date, one-package-per-invocation caveat, CI pointer.
Baseline (pre-feature) snapshot lives in research.md R10.
