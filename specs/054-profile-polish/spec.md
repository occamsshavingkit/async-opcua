# Feature Specification: OPC UA 2017 Profile Minimal Builds

**Feature Branch**: `054-profile-polish`  
**Created**: 2026-07-02 (rescoped same day: from "alias + docs polish" to real compile-time
minimization after user direction)  
**Status**: Draft  
**Input**: User description: "The goal of the profiles is to make a binary as small as
possible using the current architecture we have (config flags remove items not in the
profile; might involve separating functions into their own files to be able to compile them
on their own) and then to have suggestions on what could be changed to save even more space.
Use the OPC-UA 2017 profiles."

**Profile grounding**: the OPC Foundation 2017 server profile family —
Nano Embedded Device 2017 / Micro Embedded Device 2017 / Embedded 2017 UA Server /
Standard 2017 UA Server — as
resolved from the OPC Foundation profile database on 2026-07-02
([research-assets/PROFILES-2017.md](research-assets/PROFILES-2017.md), raw dump
[research-assets/profiles-2017.json](research-assets/profiles-2017.json)). Profile model
semantics per OPC 10000-7 §4.3/§4.5.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Nano-Profile Minimal Binary (Priority: P1)

An integrator targeting chip-class hardware builds a server whose binary contains only the
surface the Nano Embedded Device 2017 Server Profile requires: sessions, read, browse,
discovery-self, username/password tokens, policy-None secure conversation. Library code for
everything outside the profile — subscriptions/monitored items, method call, events, alarms
& conditions, history, aggregates, query, node management, diagnostics, role
administration, GDS, FOTA, programs, discovery registration — is excluded at build time,
not merely disabled at runtime, and the binary shrinks accordingly.

**Why this priority**: Nano is the smallest profile and forces the gating architecture
(per-subsystem exclusion flags, module splits where a subsystem is entangled) that the
other stories reuse. It is the MVP: an embedded-class binary with a hard, measured floor.

**Independent Test**: Build the Nano benchmark sample; verify (a) it serves the
Nano-mandated operations to a real client, (b) requests for excluded services are rejected
with the proper service-level error and never crash the server, (c) the binary is
measurably smaller than today's equivalent minimal build, and (d) the excluded subsystems'
code is absent from the binary.

**Acceptance Scenarios**:

1. **Given** the Nano build, **When** a client connects with security None, creates a
   session, authenticates anonymously or with username/password, reads attributes, browses
   (including RegisterNodes and TranslateBrowsePaths), and queries endpoints, **Then** all
   of these succeed per the profile's mandatory conformance units.
2. **Given** the Nano build, **When** a client sends a request for an excluded service
   (e.g. CreateSubscription, Call, HistoryRead), **Then** the server answers with the
   standard "service not supported" fault and remains healthy — no panic, no hang, no
   connection-wide corruption beyond the rejected request.
3. **Given** the Nano build, **When** its binary is inspected, **Then** subscription,
   alarm/eventing, history, and method-call code is not present, and the measured size is
   recorded strictly below the pre-feature minimal baseline.
4. **Given** the full-featured build (default features), **When** the existing workspace
   test suite runs, **Then** behavior is unchanged — gating introduces no functional
   difference when everything is enabled.

---

### User Story 2 - Micro-Profile Binary (Priority: P2)

An integrator building for a Micro Embedded Device 2017 target gets Nano plus exactly the
profile's addition: basic data-change subscriptions (monitored items with value-change
reporting, publish machinery, queue-overflow handling) and at least two parallel sessions —
while deadband filters, triggering, methods, and events remain excluded from the binary.

**Why this priority**: Micro requires splitting the subscription subsystem into "basic
data-change" and "advanced (deadband/triggering/eventing)" compile units — the main
separating-functions-into-own-files work the user anticipated. It delivers the middle size
point of the matrix.

**Independent Test**: Build the Micro benchmark sample; a client creates a subscription
with monitored items and receives value-change notifications; a deadband-filtered
monitored-item request is rejected as unsupported; event/method/alarm code is absent from
the binary; measured size sits between Nano and Embedded.

**Acceptance Scenarios**:

1. **Given** the Micro build, **When** a client creates a subscription, adds monitored
   items for value changes, and publishes, **Then** notifications flow per the Embedded
   DataChange Subscription facet, and two clients can hold sessions in parallel.
2. **Given** the Micro build, **When** a client requests a deadband filter, triggering, an
   event monitored item, or a method call, **Then** the request is rejected with the
   appropriate unsupported-feature status and the server stays healthy.
3. **Given** the Micro build binary, **Then** eventing/alarm, history, method-call, and
   deadband/triggering code is absent, and the measured size is strictly between the Nano
   and Embedded builds.

---

### User Story 3 - Embedded-Profile Binary (Priority: P3)

An integrator building for an Embedded 2017 UA Server target gets Micro plus exactly the
profile's additions: real message security (application-instance certificate, at least one
real security policy), the standard data-change subscription tier (deadband filters,
triggering, larger item/subscription minimums), the GetMonitoredItems/ResendData methods
(and therefore the method-call service), and a served type system — while events, alarms,
history, query, and the other out-of-profile subsystems remain excluded.

**Why this priority**: Completes the 2017 ladder and yields the largest still-embedded
configuration; depends on the gates created by US1/US2.

**Independent Test**: Build the Embedded benchmark sample; a client connects over an
encrypted channel using the server certificate, uses deadband-filtered monitored items and
triggering, calls GetMonitoredItems/ResendData; event/alarm/history code is absent from the
binary; measured size sits between Micro and the full server.

**Acceptance Scenarios**:

1. **Given** the Embedded build, **When** a client connects with a real security policy
   (sign & encrypt) against the server's application-instance certificate, **Then** the
   secure channel and session work end to end.
2. **Given** the Embedded build, **When** a client uses deadband filters, triggering, and
   calls the GetMonitoredItems and ResendData methods, **Then** they behave per the
   Standard DataChange Subscription 2017 facet, and the served address space exposes the
   type system (Base Info Type System CU).
3. **Given** the Embedded build binary, **Then** eventing/alarm, history/aggregate, query,
   node-management, GDS/FOTA/programs code is absent, and the measured size is strictly
   between the Micro build and the full server build.

---

### User Story 4 - Standard-Profile Binary (Priority: P4)

An integrator building a plant-floor-class server gets Embedded plus exactly what the
Standard 2017 UA Server Profile adds as code surface: X509-certificate user identity
tokens, registration with a Local Discovery Server (RegisterServer/RegisterServer2), and
session Cancel — while eventing/alarms, history, aggregates, query, node management, GDS,
FOTA, and programs remain excluded, because even Standard 2017 does not mandate them. The
capacity additions (≥50 parallel sessions, ≥500 monitored items, enhanced
subscription/publish minimums) are delivered as configuration defaults, not code.

**Why this priority**: The full async-opcua server exceeds Standard 2017 by a wide margin;
this rung shows how much binary the surplus costs and completes the 2017 ladder at the top.
Depends on gates from US1–US3.

**Independent Test**: Build the Standard benchmark sample; a client authenticates with an
X509 user token, the server registers with an LDS, Cancel works; alarm/history/query code
is absent from the binary; measured size sits between Embedded and the full server.

**Acceptance Scenarios**:

1. **Given** the Standard build, **When** a client authenticates with an X509 user
   identity token over a secure channel, registers-then-discovers the server via an LDS,
   and cancels an outstanding request, **Then** these behave per the profile's mandatory
   conformance units.
2. **Given** the Standard build binary, **Then** eventing/alarm, history/aggregate, query,
   node-management, GDS/FOTA/programs code is absent, and the measured size is strictly
   between the Embedded build and the full server build.
3. **Given** the Standard build's default configuration, **Then** its session and
   monitored-item limits satisfy the profile's capacity minimums (≥50 parallel sessions,
   ≥500 monitored items, ≥5 subscriptions).

---

### User Story 5 - Measured Size Matrix and CI Guard (Priority: P5)

A maintainer and a prospective embedded user can read a measured, dated binary-size matrix
(Nano / Micro / Embedded / Standard / full server) in the documentation, and reviewers see the same
measurements as a table in every CI run summary, with CI failing a profile row if an
excluded subsystem leaks back into that profile's dependency tree or binary.

**Why this priority**: The size wins from US1–US4 decay silently without measurement and
regression guarding; the matrix is also the user-facing deliverable that proves the work.

**Independent Test**: Read the docs matrix (six measured rows with architecture, build
profile, toolchain, and date); open a CI run summary and see per-build sizes; introduce an
artificial leak (e.g. enable an excluded feature in a profile sample) and watch that CI row
fail.

**Acceptance Scenarios**:

1. **Given** the documentation, **Then** it contains measured sizes for the four profile
   builds plus the existing minimal-server and full-server contrast rows, with measurement
   provenance and a pointer to CI for fresh numbers.
2. **Given** a footprint CI run, **Then** the run summary shows each build's measured size
   as a table without opening logs, and each profile row fails if profile-excluded
   subsystems appear in its build.

---

### User Story 6 - Further-Savings Report (Priority: P6)

A maintainer reads a written report of concrete opportunities to shrink the profile
binaries further than the current architecture allows — each with the architectural change
required, an estimated size impact, and its risk/effort class — so the next size-reduction
effort can be chosen deliberately.

**Why this priority**: Explicitly requested ("suggestions on what could be changed to save
even more space"); it converts everything learned while gating into a ranked backlog.

**Independent Test**: Read the report; verify it contains at least five distinct,
non-overlapping suggestions, each naming the blocking architectural constraint, an
evidence-based size estimate (from symbol/section measurement, not guesswork), and
risk/effort classification.

**Acceptance Scenarios**:

1. **Given** the report, **Then** at least five suggestions are listed, ranked by
   estimated size impact, each with: what to change, why the current architecture prevents
   it today, measured evidence for the estimate, and risk/effort class.
2. **Given** the suggestions, **Then** each is actionable as a future feature (clear scope
   boundary), and none is already achievable with the flags delivered by US1–US4.

---

### Edge Cases

- **Additive feature model**: build flags can only add, so profile builds work by the
  final binary selecting few flags, and any flag combination — including ones matching no
  named profile — must still compile and behave coherently (no compile-time combination
  explosions, no missing-symbol holes between gates).
- **Feature unification**: when a profile build coexists in a dependency graph with a
  full-featured consumer, the graph unifies to the larger surface. Profile minimization is
  a property of the final binary's own dependency declaration; documentation must say so.
- **Excluded ≠ ignorable**: the wire protocol still lets clients send requests for excluded
  services; every excluded service must fail closed with the standard unsupported-service
  fault (network-facing input must never panic — constitution IV).
- **Advertised capability honesty**: what the server advertises (endpoints, server
  capabilities, operation limits) must reflect what is compiled in, so compliant clients
  are not invited to call excluded services.
- **Full build regression risk**: gating touches shared code paths; the default
  (everything-on) build must remain behaviorally identical, verified by the unchanged
  workspace test suite.
- **Nano/Micro crypto floor**: username/password tokens are mandatory even at Nano; with
  security None endpoints the token encryption path must still work (or be an explicit,
  documented plaintext-over-None posture consistent with the profile).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server library MUST provide compile-time flags that exclude, at minimum,
  each of these subsystems independently: subscriptions/monitored items (basic tier),
  advanced monitoring (deadband/triggering), method call, eventing + alarms & conditions,
  history + aggregates, query, node management, diagnostics, role administration (RBAC),
  GDS, FOTA, programs, discovery registration. Where a subsystem is entangled with core
  code, the code MUST be reorganized (split into separately compilable units) rather than
  left compiled-in.
- **FR-002**: The facade crate MUST provide `nano`, `micro`, `embedded`, and `standard` feature
  selections whose compositions match the 2017 profile definitions as grounded in
  [research-assets/PROFILES-2017.md](research-assets/PROFILES-2017.md): Nano = core server
  surface only; Micro = Nano + basic data-change subscriptions + ≥2 sessions; Embedded =
  Micro + standard data-change tier (deadband, triggering, GetMonitoredItems/ResendData
  methods) + certificate-based security + served type system.
- **FR-003**: A request for any compiled-out service MUST be answered with the standard
  unsupported-service fault (Bad_ServiceUnsupported) and MUST NOT panic, hang, or corrupt
  unrelated sessions (constitution IV: network-facing paths fail closed).
- **FR-004**: Server-advertised metadata (endpoint descriptions, server capabilities,
  operation limits) MUST NOT advertise capabilities that are compiled out.
- **FR-005**: With all features enabled (default build), behavior MUST be unchanged: the
  full workspace test suite passes without modification of expected behavior.
- **FR-006**: Every flag combination implied by the profile lattice MUST compile: each
  profile alias standalone, each individual subsystem flag toggled off the full set, and
  the no-default-features baseline (CI-checkable).
- **FR-007**: The foundation-profile benchmark samples — the three existing ones plus a new Standard sample — MUST build through their
  profile alias and demonstrate the profile's mandated behavior (smoke-testable against a
  real client).
- **FR-008**: Documentation MUST present a measured size matrix — Nano, Micro, Embedded,
  minimal-server, full server — with architecture, build profile, toolchain, and date, and
  MUST explain the additive-feature/unification caveat and the Nano/Micro no-certificate
  (policy None) security posture.
- **FR-009**: The footprint CI workflow MUST build all four profile variants, publish
  every measured size to the run summary as a table, and fail a profile row when a
  profile-excluded subsystem appears in that build (dependency-tree and/or symbol-level
  guard).
- **FR-010**: A further-savings report MUST be delivered with ≥5 ranked suggestions, each
  with the blocking architectural constraint, measured evidence for its size estimate, and
  a risk/effort classification.

### Key Entities

- **Subsystem gate**: a compile-time flag excluding one server subsystem; the unit of size
  reduction and the unit of regression guarding.
- **Profile composition (`nano`/`micro`/`embedded`/`standard`)**: a named facade feature selection
  mapping one 2017 profile's mandatory conformance surface onto a set of subsystem gates.
- **Profile benchmark sample**: the buildable consumer of one composition; the artifact CI
  measures and guards.
- **Size matrix**: dated measured sizes for the three compositions plus contrast rows;
  mirrored live by the CI run-summary table.
- **Further-savings report**: ranked post-gating opportunities requiring architecture
  changes, with evidence-based estimates.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The Nano build's measured binary is strictly smaller than the pre-feature
  minimal baseline (current `base-server` benchmark), and sizes are strictly monotonic:
  Nano < Micro < Embedded < Standard < full server.
- **SC-002**: For each profile build, every mandated operation of its 2017 profile works
  against a real client, and every excluded service request is answered with the standard
  unsupported fault — zero panics or hangs under those requests.
- **SC-003**: The default full-featured build passes the entire existing workspace test
  suite unchanged.
- **SC-004**: The documentation matrix contains six measured rows with provenance;
  reviewers can read every build's size from the CI run summary; a profile-excluded
  subsystem leaking into a profile build turns its CI row red.
- **SC-005**: The further-savings report contains at least five ranked, evidence-backed,
  non-overlapping suggestions, each scoped as a future feature.

## Assumptions

- The 2017 profile family (not 2022+) is the target, per explicit user direction; the
  resolved 2026-07-02 database snapshot in research-assets/ is the normative grounding for
  what is in/out of each profile.
- Optional conformance units are excluded from profile builds unless they come for free
  (e.g. Attribute Write stays available where its code is part of the core read/write
  path); the point of the builds is the minimal mandatory surface.
- "Binary size" means the stripped, size-profile-built benchmark executable on x86-64
  Linux, as measured by the existing footprint machinery; cross-target absolute numbers
  differ but the ordering and roughly the ratios carry.
- Runtime configuration (session limits, monitored-item limits) continues to handle
  profile constraints that are quantities rather than code surface (e.g. "Session Minimum
  2 Parallel" is capacity, not code to add).
- The Embedded profile's "Base Info Type System" is satisfiable by serving the type
  hierarchy the server actually uses; shrinking the generated core nodeset below "all of
  it" is expected to land in the further-savings report if it cannot be done safely within
  this feature.
- The existing `[profile.embedded]` cargo build profile and CI dependency-boundary guard
  are reused and extended, not replaced.
