# Research: OPC UA 2017 Profile Minimal Builds (054)

Resolved 2026-07-02 by: OPC Foundation profile-database API fetch (R1), direct repo
inspection with file:line grounding (R2–R9), and isolated baseline builds (R10).

## R1. Normative profile compositions (what is in/out per profile)

**Decision**: use the recursive profile-database resolution snapshotted in
[research-assets/PROFILES-2017.md](research-assets/PROFILES-2017.md) /
[profiles-2017.json](research-assets/profiles-2017.json) as the single source of truth.
Key consequences: username/password tokens mandatory at ALL rungs (incl. Nano); Write
optional at Nano; Call service required only from Embedded (GetMonitoredItems/ResendData);
real security policies + application certificate from Embedded; X509 user tokens + LDS
registration (Discovery Register/Register2) + Session Cancel from Standard; events/A&C,
history, aggregates, Query, NodeManagement, GDS, programs, diagnostics mandated at NO rung.

**Rationale**: Part 7 (1.05) no longer prints profile contents; the online database is the
maintained definition (OPC 10000-7 §1, §4.3/§4.5). 2017 family chosen per explicit user
direction.

**Alternatives considered**: 2022 profile family — rejected (user said 2017; 2022 adds
time-sync and currency CUs irrelevant to the size goal).

## R2. Gating mechanism: dispatch-arm removal onto the existing fallback

**Decision**: gate service *handling*, not decoding. The post-session dispatcher
(`async-opcua-server/src/session/message_handler.rs:244`) already has a catch-all that
answers `ServiceFault(BadServiceUnsupported)` (`message_handler.rs:408-421`). Each gated
service's match arm gets `#[cfg(feature = ...)]`; with the feature off, requests fall
through to the fallback. Pre-session/discovery services live in
`session/controller.rs:464-779` and are gated per-branch (per-branch faults exist there).

**Rationale**: fail-closed comes for free from an already-tested path; the types crate
keeps decoding every request type, so malformed-input surface is unchanged (constitution
IV). This is precisely how an unimplemented service already behaves.

## R3. Server-crate feature set (all default ON — compat for direct dependents)

**Decision**: add to `async-opcua-server`:

| Feature | Gates | Anchors (from coupling map) |
|---------|-------|------------------------------|
| `subscriptions` | whole `subscriptions/` module, publish/republish/create/modify/delete/transfer/set-publishing/set-triggering dispatch, `CreateMonitoredItem` trait surface, `RequestContext.subscriptions` | `message_handler.rs:31`, `session/{controller,manager,audit}.rs`, `server.rs:430`, `node_manager/mod.rs:44`, `memory/{core,simple,memory_mgr_impl}.rs` |
| `subscriptions-standard` (requires `subscriptions`) | deadband filter evaluation, triggering, larger tiers | split out of `subscriptions/monitored_item.rs` (deadband eval + `set_triggering` `:1043`; orchestration `session_subscriptions.rs:571`, `mod.rs:1425`) |
| `events` (requires `subscriptions`) | event monitored items, event-filter engine, notify paths | `services/subscription/{filter,where_clause,select}.rs`, `subscriptions/mod.rs:1205,1213`, `subscriptions/notify.rs` |
| `alarms` (requires `events`, `method-call`) | A&C types, condition/limit/discrete alarms, shelving, branching | `alarms/` module; refs `namespace/init.rs:5`, `memory/simple.rs:13` |
| `method-call` | Call service + method registration surface | `services/method.rs:16`, dispatch `message_handler.rs:365`; builtin `GetMonitoredItems`/`ResendData` in `memory/core.rs:1040/1058` additionally need `subscriptions-standard` |
| `history` | HistoryRead/HistoryUpdate services, backend trait, in-memory stores | dispatch `message_handler.rs:347-353`, `memory/simple.rs:163,435-780,821` |
| `history-aggregates` (requires `history`) | Part-13 aggregate engine | `aggregates/`; `memory/simple.rs:518-522`, `config/capabilities.rs` |
| `query` | QueryFirst/QueryNext | `services/query.rs`, dispatch `message_handler.rs:357-383`, `node_manager/query.rs` |
| `node-management` | AddNodes/AddReferences/DeleteNodes/DeleteReferences | `services/node_management.rs`, `node_manager/node_management.rs` |
| `diagnostics` | diagnostics node manager + ServerDiagnostics wiring | `builder.rs:61`, `server.rs:31`, `info.rs:221` |
| `rbac` | role resolver, role-management methods, well-known permissions | `rbac/`, `builder.rs:10,418`, `server.rs:340,466`, `info.rs:188` — hardest; see R6 |
| `gds` | GDS cert-management/push/pull methods | `gds/mod.rs:34` (self-contained — cleanest) |
| `fota` | firmware-update model | `fota/`, `info.rs:226`, `server.rs:422` |
| `programs` | Part-10 programs | `programs/mod.rs:12` (self-contained) |
| `lds` | ACTING as LDS: receiving RegisterServer/RegisterServer2, bounded registry | `controller.rs:715,743` (feature 024) — outside all server profiles |

Existing features unchanged: `generated-address-space`, `discovery-server-registration`
(client-side Register — mandatory at Standard), `discovery-mdns`, `ecc`, `wss`, `json`,
`legacy-crypto`, crypto backends. Default set = ALL new features (+ current defaults), so
`cargo build -p async-opcua-server` is surface-identical before/after.

**Rationale**: feature-per-subsystem matches the profile in/out table exactly (R1) and
follows the crate's own precedent (R8). Default-ON preserves semver for direct dependents;
the facade already consumes the server crate with `default-features = false`
(`async-opcua/Cargo.toml` dependencies), so aliases can select arbitrary subsets.

**Alternatives considered**: one mega-feature per profile inside the server crate —
rejected (not composable, hides which subsystem costs what, and CU-level guards become
impossible); `cfg`-by-profile-name — rejected (profiles are compositions, not code units).

## R4. Facade alias compositions

**Decision** (`async-opcua/Cargo.toml`):

```toml
# NOTE (implementation finding): base-server forwards ALL gates, so nano cannot be
# built on it — nano enables the dependency raw via dep: syntax instead.
nano     = ["dep:async-opcua-server", "dep:async-opcua-nodes"]  # NO subsystem gates
micro    = ["nano", "async-opcua-server/subscriptions"]
embedded = ["micro", "async-opcua-server/subscriptions-standard",
            "async-opcua-server/method-call", "generated-address-space", "aws-lc-rs"]
standard = ["embedded", "discovery-server-registration"]
```

`base-server` and `server` are widened to forward ALL new server-crate features (so their
meaning — "full server, minus/plus the core nodeset" — is unchanged for existing users).

**Rationale**: mirrors R1 rung-by-rung. `embedded` includes `generated-address-space`
because Base Info Type System is mandatory there and the served type hierarchy is today
only available via the generated core namespace (pruning it is a further-savings item,
R11). `standard` adds only LDS-client registration as new compiled surface — X509 user
tokens and Session Cancel are already in the always-compiled core (their theoretical
gating is a further-savings note, not worth flags today).

## R5. Module splits required (the "separate functions into own files" work)

**Decision**:
1. `subscriptions/monitored_item.rs` — extract deadband-filter evaluation and triggering
   into `subscriptions/monitored_item/` submodules gated by `subscriptions-standard`;
   ungated builds reject filter-bearing CreateMonitoredItem/SetTriggering with
   `Bad_MonitoredItemFilterUnsupported` / `BadServiceUnsupported` respectively.
2. Event-filter engine already lives separately (`services/subscription/`) — pure cfg
   gating, no split needed.
3. `NodeManager` trait: monitored-item/subscription methods (`CreateMonitoredItem` flow)
   behind `#[cfg(feature = "subscriptions")]`; history methods behind `history`; query
   behind `query`; node-management behind `node-management`. Additive-only trait surface.
4. `RequestContext`/`SessionManager`/`ServerCore` fields holding `SubscriptionCache`
   cfg-gated using the `discovery-mdns` cfg-field precedent (`info.rs:214-219`).

**Rationale**: the coupling map shows subscriptions is the only subsystem whose types leak
into shared spines; everything else is decl+registration gating. Deadband/triggering are
interleaved in `monitored_item.rs` (eval + `:1043`), so a file split is unavoidable — as
the user anticipated.

## R6. RBAC gating depth

**Decision**: `rbac` gates the role-management method registration
(`server.rs:466`), the `rbac/` module, builder rules (`builder.rs:418`), and the
`RoleResolver` on `ServerInfo` (`info.rs:188`) — replaced under `cfg(not)` by a zero-size
always-empty-roles resolver so the auth spine keeps one code shape. Runtime RBAC
enforcement stays opt-in (feature 031) and simply cannot be enabled in a build without
`rbac` (config validation error, fail-closed).

**Rationale**: RoleResolver is threaded through RequestContext (auth spine), so full
type-level removal would distort shared signatures; a unit stub keeps signatures stable at
~zero size cost while the 108K `rbac/` module compiles out.

## R7. Identity-token crypto floor at Nano/Micro (policy None)

**Decision**: keep the current behavior: plaintext user-name tokens are accepted on None
endpoints (`negotiate.rs:94-98,174-176` — no crypto needed), and tokens encrypted with the
server cert still decrypt via the RSA path if a cert exists. Nano/Micro samples run None +
plaintext-or-anonymous; password verification via argon2 stays (authenticator.rs:380).
Document this posture (spec FR-008). No new crypto gating in this feature; shrinking the
crypto crate for None-only builds goes to the further-savings report (R11).

**Rationale**: matches the profile (SecurityPolicy Support with None is conformant at
Nano/Micro; User Name Password CU is satisfied by the None-policy token variant); avoids
touching the crypto crate's surface in the same feature that reshapes the server crate.

## R8. cfg precedent to follow (style)

`generated-address-space` for subsystem registration blocks with `#[cfg(not)]` fallbacks
(`builder.rs:53-64`); `discovery-server-registration` for background tasks
(`future::pending()` fallback, `server.rs:611-615`); `discovery-mdns` for cfg'd struct
fields (`info.rs:214-219`); `ecc` for fine-grained expression-level gating
(`negotiate.rs:145-172`). New gates copy these shapes — no new idioms.

## R9. Advertised-capability honesty (FR-004)

**Decision**: the core node manager's capability/limit nodes
(`node_manager/memory/core.rs:712-806`) and `HistoryServerCapabilities` (`:798+`) are
cfg-adjusted so gated-out services advertise `false`/absent; `OperationalLimits` fields for
gated services are cfg-gated with the mdns-field pattern. Nano/Micro (no
generated-address-space) advertise via `GetEndpoints` only — nothing to adjust beyond the
endpoint's token policies.

## R10. Baseline measurements (pre-feature, 2026-07-02, rustc 1.96.0, x86-64 Linux, `--profile embedded`, stripped)

| Package | Bytes | Note |
|---------|-------|------|
| foundation-profile-nano-server | 7,636,648 | base-server, no crypto backend |
| foundation-profile-micro-server | 7,636,664 | identical surface to nano today |
| foundation-profile-embedded-server | 9,906,256 | + aws-lc-rs (~2.27 MB) |
| minimal-server | 7,631,864 | base-server |
| simple-server | 15,862,224 | `server` (full nodeset), pure-Rust crypto |

**SC-001 target**: post-gating Nano must land strictly below 7,636,648 B; expected way
below (subscriptions 332K src + alarms 176K + rbac 108K + history/aggregates 152K + gds
60K + services/diagnostics compile out, plus their transitive monomorphization).

**Methodology caveat (normative for docs + CI)**: one package per cargo invocation.
Building multiple packages together unifies features across the shared graph and poisons
the numbers (measured: combined build inflated nano from 7.64 MB to 17.37 MB and collapsed
the nano↔embedded delta to 448 bytes).

## R11. Further-savings report (US6) — methodology

**Decision**: evidence = `cargo bloat`-style symbol/section accounting (via `nm --size-sort`
/ `cargo bloat` if available in-sandbox) on the four profile binaries. Report lives at
`docs/profile-size-report.md`. Seeded candidate list (to be measured, not asserted):
pruned/tiered core nodeset for Base Info Type System (vs full 4.7 MB generated namespace);
None-only crypto build (drop RSA/x509 parsing at Nano/Micro); tokio feature slimming /
current-thread runtime; `chrono`→`time`-lite or custom DateTime; regex-free config path;
serde/config gating (config file parsing vs programmatic); panic-message stripping
(`panic_immediate_abort`-class, conflicts with unwind posture — document trade-off);
monomorphization hotspots in generic node-manager plumbing.

## R12. CI + measurement integration

**Decision**: extend `ci_footprint.yml` to a 4-profile matrix (add standard), each job:
(1) `cargo tree` guard — profile-excluded features/crates absent (per-profile deny-list,
e.g. nano row fails if `async-opcua-server` resolves `subscriptions`); (2) build
`--locked --profile embedded`, one package per invocation; (3) `stat` size + append
markdown row to `$GITHUB_STEP_SUMMARY`; (4) symbol spot-check guard (e.g. `nm | grep -c`
for a subscription symbol must be 0 in nano/micro). A shared script
(`tools/footprint.sh` or workflow-inline) is used by docs measurement and CI so the
matrix and the summary can't diverge.
