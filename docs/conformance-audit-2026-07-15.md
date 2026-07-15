# Conformance Catalogue — 2026-07-15

A ground-truth audit of OPC UA Foundation conformance-unit (CU) implementation
status, replacing prior narrower estimates. This document records *why* the
numbers changed and what the real backlog looks like; `specs/conformance-tester/CU-COVERAGE.md`
carries the full per-CU ledger this document summarizes.

## Why this exists

The CU-COVERAGE report previously tracked only the 4 canonical composite
server profiles (Nano/Micro/Embedded/Standard 2025), a 123-CU closure. The
OPC UA Foundation snapshot actually defines 76 server-side profiles/facets
referencing **492 distinct CUs** in total (of the 1182 CUs across the whole
snapshot — the other ~690 belong to Client/Gateway profile types this
Server-scoped snapshot doesn't link, not things this repo is silently
failing). The 72 facets outside the 4 canonical profiles were never
enumerated at all — CUs like the individual Alarms & Conditions subtypes,
Historical Access operations, GDS Directory services, and infrastructure
facets (File Access, Scheduler, Redundancy) had no evidence tracking
whatsoever.

`tools/cu-coverage-report` was widened to enumerate all 76 profiles via a
proper recursive transitive-closure computation (`included_conformance_units`
+ `included_profiles`, matching the tool's own pre-existing `transitive_cu_closure`
output exactly for the 4 canonical profiles it already covered).

That widening alone doesn't answer "is it implemented" — it just exposes the
true CU surface. Answering that required actually reading the code: **7
independent research passes**, one per subsystem cluster, each grounded in
file:line evidence rather than prior feature-completion claims (several of
which turned out to be narrower than remembered — see Alarms & Conditions
below).

## Headline numbers

| | Before this audit | After |
|---|---:|---:|
| CUs tracked | 123 (4 canonical profiles only) | 492 (all 76 server profiles/facets) |
| `implemented` | 27 | 245 |
| `partial` | 19 | 42 |
| `gap` (confirmed absent) | 0 (status didn't exist) | 197 |
| `needs-proof` (unreviewed) | 442 | 4 |
| `extensible` (feature 093) | 3 | 3 |
| `source-issue` | 1 | 1 |

The prior report wasn't wrong about what it covered — it was just covering a
small, self-selected slice and reporting "needs-proof" (a hedge) for
everything else. This audit converts almost all of that hedge into a real
verdict.

## Per-subsystem breakdown

| Subsystem | Implemented | Partial | Gap | Total audited |
|---|---:|---:|---:|---:|
| Alarms & Conditions (Part 9) | 21 | 7 | **98** | 126 |
| Node / Type / Method (Part 3/4/5) | 59 | 8 | **32** | 99 |
| GDS / Security (Part 12) | 35 | 15 | **21** | 71 |
| Historical Access (Part 11) | 55 | 2 | 12 | 69 |
| Subscriptions incl. Aggregates (Part 4/13) | 40 | 2 | 6 | 48 |
| User Token / Role / Auditing (Part 4/18) | 19 | 7 | 10 | 36 |
| Infra & Misc (File Access, Scheduler, Redundancy, ...) | 13 | 4 | **19** | 36 |
| **Total** | **242** | **45** | **198** | 485 |

(Cluster totals overlap slightly with the pre-existing 123-CU canonical-profile
review, hence 485 here vs. 492 in the full ledger — the delta is CUs already
reviewed in earlier features that fall outside these 76 facets.)

## Prioritized gap themes

Ranked by how large and how surprising the gap is relative to what was
believed implemented.

### 1. Alarms & Conditions: a 58-CU state-variable block, plus Enable/Suppress/Auditing (98 gaps)

Core A&C mechanics are genuinely solid: ConditionType/AlarmConditionType,
Acknowledge/Confirm/AddComment/Refresh/Refresh2, branching, shelving, dialog
respond, exclusive+non-exclusive limit alarms, and discrete OffNormal/Trip
alarms are all implemented and tested (`async-opcua-server/src/alarms/*.rs`,
2445 lines of integration tests in `async-opcua/tests/integration/alarms.rs`).
That's what "A&C complete" (feature `feature-ac-completion`) correctly
referred to.

What it didn't cover, confirmed as real gaps:
- **CU 5510–5567 (58 CUs)**: `TransitionTime`/`EffectiveTransitionTime`/`EffectiveDisplayName`
  state-variable properties on every limit sub-state (Low/LowLow/High/HighHigh)
  — zero hits anywhere in the codebase for these three terms.
- **Enable/Disable, Suppress/UnSuppress, OutOfService, Silence Methods** — the
  underlying state variables exist (`Suppressed`, `OutOfService`) but no
  client-facing Method is registered for any of them, and no test exercises
  them.
- **A&C Auditing** — an explicit `TODO` comment in the code admits this isn't
  done.
- **Alarm Groups, GetGroupMemberships, Latching, AlarmMetrics, COM A&E wrapper**.
- Most non-Limit/non-Discrete alarm subtypes (CertificateExpiration,
  SystemOffNormal, Discrepancy, Instrument/SystemDiagnostic, Exclusive/
  NonExclusive Level/Deviation/RateOfChange) exist only as nodeset type
  definitions, never instantiated by server code.

### 2. Node/Type/Method: DataAccess variable-type instances, semantic-change machinery (32 gaps)

Node Management and the Method Call framework are solid and tested. Most
"exposes type X in the AddressSpace" CUs are satisfied for free by the
full 1.05 nodeset import. Real gaps:
- **DataAccess variable types beyond `AnalogItemType`** — `TwoState`/`MultiState`/
  `MultiStateValueDiscrete`/`DiscreteItemType`/the `ArrayItemType` family are
  never instantiated anywhere in server/samples/tests.
- **SemanticChange status-bit machinery** doesn't exist.
- Several event types (`DeviceFailureEventType`, `SystemStatusChangeEventType`,
  `AuditUpdateStateEventType`) are defined but never emitted.
- `OrderedList`, Dictionary (IRDI/URI instances), `HasInterface` usage, and
  StateMachine-linked alarm triggering are all unimplemented beyond
  type-level nodeset presence.

### 3. GDS/Security: no Directory service, no Authorization Service, no KeyCredential Service (21 gaps)

Certificate push-model methods that exist (`StartSigningRequest`,
`CreateSigningRequest`, `GetRejectedList`, `UpdateCertificate`) are real and
tested. But:
- **GDS Directory** (`RegisterApplication`, `QueryServers`, `QueryApplications`)
  is explicitly mapped to `BadServiceUnsupported`
  (`async-opcua-server/src/session/services/method.rs:98-104,131-135`) — not
  missing by accident, actively rejected.
- **Authorization Service** (OAuth2 token issuance) doesn't exist: zero hits
  for `AuthorizationServiceConfigurationType`/`RequestAccessToken`.
- **KeyCredential Service** doesn't exist at all: zero non-generated hits for
  "KeyCredential" anywhere in the repo.
- JWT/OAuth2 **authority discovery** (issuer endpoint URL, Azure-style profile
  discovery) is absent even though local-certificate JWT signature validation
  itself genuinely works.

### 4. Historical Access: `ReadAtTimeDetails`, structured-data update paths (12 gaps)

All 35 standard Part-13 aggregates are genuinely implemented with per-aggregate
tests. Real gaps:
- **`ReadAtTimeDetails`** has zero server-side implementation for any backend
  — `SimpleNodeManagerImpl` overrides every other `history_read_*` method but
  conspicuously not this one, so it always falls through to
  `BadHistoryOperationUnsupported`.
- **"Structured Data" update/delete paths are a systematic gap** — both
  backends' `update_structure_data` only accept `Annotation`-typed values,
  exactly what the relevant CUs explicitly require to be more general than.
- **Start (2357) / End (2358) aggregates** are gaps distinct from their
  `*Bound` variants — only `StartBound`/`EndBound` are in the dispatch table.
- **Per-variable `AggregateConfigurationType`** isn't built (the aggregate
  engine only sources config from the request parameter, never an
  address-space node); the server-wide "Aggregate Master Configuration" is
  implemented for free via the imported nodeset.

### 5. Infra/Misc: File Access, Scheduler, and Redundancy are essentially unbuilt (19 gaps)

- **File Access / Temporary File Access**: `FileType` node *metadata* exists
  (`fota/file_node.rs`) but no `add_method_cb` callback is wired anywhere —
  Open/Read/Write/Close Methods exist as inert nodes, not functional I/O.
- **Scheduler** is completely absent — the `programs/` module is the
  unrelated Part-10 `ProgramStateMachine`, not the Scheduler companion spec's
  `ScheduleType`/`CalendarType`.
- **Redundancy** (server clustering/failover) has zero implementation beyond
  a generated DataType stub.
- **Sessionless Invoke** exists only as generated request/response types,
  never wired into the session layer; `rbac/decision.rs:147` has a `TODO`
  admitting sessionless enforcement isn't done.
- **Multiple Languages / Troubleshooting Guide** documentation gaps
  (English-only; no FAQ/troubleshooting content anywhere).

### 6. User/Role/Auditing: RBAC write path, JWT authority discovery (10 gaps)

RBAC read-side enforcement is solid and tested. Gaps:
- **No live Write-service path sets `RolePermissions`/`DefaultRolePermissions`**
  — only settable at node-creation/config time.
- **`UserWriteMask`** is a static per-node field, never computed per-user
  (unlike `UserAccessLevel`, which genuinely is per-user).
- **`TrustedApplication` role and `UserManagement` AddressSpace object** are
  unimplemented.
- Several audit/system event types exist only as generated-nodeset structs,
  never instantiated (`AuditClientEventType`, `SystemStatusChangeEventType`,
  `DeviceFailureEventType`, `AuditHistoryUpdateEventType`).

### 7. Subscriptions: mostly solid, but Subscription Durability doesn't exist (6 gaps)

Aggregate subscriptions, DataChange/Event subscriptions, and Subscription
Transfer are all genuinely implemented and well-tested. The one substantial
gap: **Subscription Durable** (`SetSubscriptionDurable`) — zero "durable"
references anywhere in the server crate; the Method NodeId exists only as an
inert generated nodeset entry.

## What this doesn't cover

- This is a **Server**-scoped audit (the snapshot's own scoping). Client and
  Gateway/Aggregation profile CUs (the ~690 CUs never referenced by any of
  the 76 Server profiles here) are out of scope for this pass.
- Each verdict is one independent agent's reading of the code at a point in
  time; treat `gap` verdicts as strong (a targeted, documented search found
  nothing) but not infallible. Re-verify before committing to a specific
  implementation task, per this repo's usual grounding practice.
- `partial` (45 CUs) generally means "implementation exists, test coverage or
  full CU-described surface doesn't" — often the cheapest next win, since
  most of the work is already done.
