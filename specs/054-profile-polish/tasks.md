# Tasks: OPC UA 2017 Profile Minimal Builds

**Input**: Design documents from `/specs/054-profile-polish/`
**Prerequisites**: plan.md, research.md (R1–R12), data-model.md,
contracts/feature-aliases.md, research-assets/PROFILES-2017.md (normative profile
grounding)

**Conventions** (from 053): red-first independent tests (Claude-authored, never by the
implementing codex dispatch); one task per codex dispatch; every task cites its OPC UA
Part/§ or profile Conformance Unit so the reference MCP can ground it; FINDINGS-style
doc rows ride with the story that closes them; one commit per user story.

**Status-code naming**: spec.md writes `Bad_ServiceUnsupported` (spec style); tasks and
code use the Rust `StatusCode` names (`BadServiceUnsupported`,
`BadMonitoredItemFilterUnsupported`, `BadAggregateNotSupported`). Same codes.

**Test-graph unification caveat**: the in-sample behavior tests run under `cargo test`,
where dev-dependencies (the in-tree client) unify features on shared crates. This does
NOT invalidate rejection tests — the client never enables `async-opcua-server` subsystem
gates — but binary-absence claims are ONLY valid against `cargo build` artifacts, which
ignore dev-deps. Keep behavior verification in tests and absence verification in
`tools/check-profile-absence.sh` against the built binary; never add a dev-dependency to
a profile sample that enables server-crate subsystem features.

**Tests**: verification is at three levels (behavior, rejection, absence) — test tasks
MUST be committed red (or red-equivalent: failing script/assertion) before their
implementation tasks.

## Phase 1: Setup (blocking all stories)

- [X] T001 Declare the 15 subsystem features in `async-opcua-server/Cargo.toml` per the
      research.md R3 table — all in `default`, with requires-relations
      (`subscriptions-standard = ["subscriptions"]`, `events = ["subscriptions"]`,
      `alarms = ["events", "method-call"]`, `history-aggregates = ["history"]`,
      `gds = ["method-call"]`, `fota = ["method-call"]`, `programs = ["method-call"]`).
      Declarations only — no cfg in code yet; full build must be feature-identical.
      [Cite: OPC 10000-7 §4.3/§4.5; data-model.md gate table]
- [X] T002 Widen `base-server`/`server` in `async-opcua/Cargo.toml` to forward ALL new
      server-crate gates (meaning unchanged) and add the four profile aliases per
      research.md R4: `nano`, `micro`, `embedded`, `standard`. [Cite: profile URIs
      `http://opcfoundation.org/UA-Profile/Server/{NanoEmbeddedDevice2017,
      MicroEmbeddedDevice2017,EmbeddedUA2017,StandardUA2017}`]

**Checkpoint**: `cargo check -p async-opcua --no-default-features --features nano|micro|embedded|standard`
all pass (gates exist, code not yet gated); full build unchanged.

## Phase 2: User Story 1 — Nano-Profile Minimal Binary (P1) 🎯 MVP

**Goal**: everything outside Nano 2017 compiles out; Nano sample serves the mandated core;
excluded services fail closed; binary strictly below 7,636,648 B (research.md R10).

**Independent test**: `cargo test -p async-opcua-foundation-profile-nano-server` (smoke +
rejection) green; symbol-absence script green; workspace suite (default features) green.

### Tests (red first)

- [X] T003 [P] [US1] Nano mandated-ops smoke test in
      `samples/foundation-profile-nano-server/tests/profile_smoke.rs`: None-policy
      connect, CreateSession/ActivateSession (anonymous AND username/password), Read,
      Browse + BrowseNext, RegisterNodes, TranslateBrowsePathsToNodeIds, GetEndpoints,
      FindServers-self. [Cite: Core 2017 Server Facet CUs — Session Base/Minimum 1,
      Attribute Read, View Basic/RegisterNodes/TranslateBrowsePath, Discovery Get
      Endpoints/Find Servers Self, Security User Name Password; OPC 10000-4 §5.5 Discovery, §5.7 Session, §5.9 View, §5.11 Attribute/Read]
- [X] T004 [P] [US1] Nano rejection test in
      `samples/foundation-profile-nano-server/tests/service_rejection.rs`:
      CreateSubscription, Publish, Call, HistoryRead, QueryFirst, AddNodes each answered
      `BadServiceUnsupported` via raw `UARequest` builders (error-mode convention, memory
      `errormode-selftest-campaign`), then a follow-up Read on the same session still
      succeeds (no corruption). RED today (these services currently succeed).
      [Cite: OPC 10000-4 §5.3 Service results, §7.34 ServiceFault; constitution IV]
- [X] T005 [P] [US1] Symbol/dependency absence script
      `tools/check-profile-absence.sh <package> <deny-features> <deny-symbols>`:
      `cargo tree -e features` deny-list + `nm -C` sentinel check (SubscriptionCache,
      alarm, history sentinels for nano) against the BUILT binary (see test-graph
      unification caveat above). RED today. [Cite: spec FR-009; research.md R12]

### Implementation (one gate per task; each leaves full build green)

- [X] T006 [US1] Gate the subscription spine behind `subscriptions`: cfg fields +
      constructors + dispatch arms in `async-opcua-server/src/session/message_handler.rs`
      (arms → fallback `:408-421`), `session/controller.rs:46`, `session/manager.rs:23`,
      `session/audit.rs:13`, `server.rs:430`; use the `discovery-mdns` cfg-field pattern
      (`info.rs:214-219`). [Cite: OPC 10000-4 §5.13/§5.14 MonitoredItem+Subscription service sets; research.md R2/R5]
- [X] T007 [US1] Gate the `NodeManager` monitored-item trait surface + all in-tree impls
      behind `subscriptions`: `node_manager/mod.rs:44`, `node_manager/memory/mod.rs:35`,
      `memory/core.rs:26`, `memory/memory_mgr_impl.rs:15`,
      `session/services/monitored_items.rs`. Additive-only trait change (full build
      unchanged). [Cite: OPC 10000-4 §5.13 MonitoredItem service set; research.md R5.3]
- [X] T008 [US1] Gate the `subscriptions/` module decl + public re-exports
      (`lib.rs:60`, `lib.rs:82-102`) and `session/services/mod.rs:31` re-export behind
      `subscriptions`; fix resulting cfg fallout so `--no-default-features
      --features base-server`-equivalent builds compile. [Cite: research.md R3]
- [X] T009 [US1] Gate Call service behind `method-call`: `session/services/method.rs`,
      dispatch arm `message_handler.rs:365`, method-registration surface (typed_method
      path stays compiled only with the feature). [Cite: OPC 10000-4 §5.12 Method service set]
- [X] T010 [US1] Gate history behind `history` and aggregates behind
      `history-aggregates`: dispatch arms `message_handler.rs:347-353`,
      `services/{history_read,history_update}.rs`, backend surface
      `node_manager/memory/simple.rs:163,435-780,821`, `aggregates/` +
      `memory/simple.rs:518-522`, `config/capabilities.rs` aggregate wiring. (Kept as ONE
      task deliberately: the aggregate hooks are inline in the same `simple.rs`
      history-read path — splitting would make two dispatches edit the same functions.)
      [Cite: OPC 10000-11 §6.1/§6.5; OPC 10000-4 §5.11 HistoryRead/HistoryUpdate; OPC 10000-13 §5]
- [X] T011 [US1] Gate QueryFirst/QueryNext behind `query`: dispatch arms
      (`message_handler.rs:357-383` range), `session/services/query.rs`,
      `node_manager/query.rs`. [Cite: OPC 10000-4 Annex B.2.3/B.2.4 (Query service set in 1.05.07)]
- [X] T012 [US1] Gate AddNodes/AddReferences/DeleteNodes/DeleteReferences behind
      `node-management`: dispatch arms, `session/services/node_management.rs`,
      `node_manager/node_management.rs` (+ write-validation cluster refs).
      [Cite: OPC 10000-4 §5.8]
- [X] T013 [US1] Gate diagnostics behind `diagnostics`: node-manager registration
      `builder.rs:61` (composes with the surrounding `generated-address-space` cfg
      block), `diagnostics/` module, `ServerDiagnostics` wiring `server.rs:31`,
      `info.rs:221`. [Cite: OPC 10000-5 §6.3 — Base Info Diagnostics is OPTIONAL at every
      2017 rung]
- [X] T014 [US1] Gate RBAC behind `rbac` with the R6 stub: `rbac/` module,
      `builder.rs:10,418`, `server.rs:340,466`, `RoleResolver` on `info.rs:188` replaced
      under `cfg(not)` by an always-empty-roles unit resolver;
      `enforce_role_based_access` config rejected at validation when feature off.
      [Cite: OPC 10000-18; Core 2017 "Security Role Server Authorization" is OPTIONAL]
- [X] T015 [US1] Gate GDS methods behind `gds`: `gds/mod.rs:34` registrations
      (cert-management, push, pull). Self-contained module — decl + registration gating
      only. [Cite: OPC 10000-12 §7]
- [X] T016 [US1] Gate FOTA behind `fota`: `fota/` module decl, `info.rs:226`
      `fota_cleanup` cfg-field (mdns-field pattern), `server.rs:422` init.
      [Cite: OPC 10000-21]
- [X] T017 [US1] Gate programs behind `programs`: `programs/mod.rs:12`
      (`register_program`, `ProgramMethodHandler`). Self-contained.
      [Cite: OPC 10000-10]
- [X] T018 [US1] Gate the LDS receive-side behind `lds`: `controller.rs:715,743`
      RegisterServer/RegisterServer2 arms + the bounded registry from feature 024;
      off ⇒ `BadServiceUnsupported` per-branch fault. [Cite: OPC 10000-12 §4.2.2/§5.1; OPC 10000-4 §5.5 RegisterServer/RegisterServer2]
- [X] T019 [US1] Gate alarms behind `alarms`: `alarms/` module decl, refs in
      `namespace/init.rs:5` and `node_manager/memory/simple.rs:13`, event dispatch
      touchpoints `alarms/dispatch.rs:110-114`, `alarms/methods.rs`. [Cite: OPC 10000-9]
- [X] T020 [US1] Gate the event engine behind `events`:
      `session/services/subscription/{filter,where_clause,select}.rs`,
      `subscriptions/mod.rs:1205,1213` notify paths, `subscriptions/notify.rs`; event
      monitored items rejected `BadMonitoredItemFilterUnsupported` when off.
      [Cite: OPC 10000-4 §7.22.3 EventFilter]
- [ ] T021 [US1] Switch `samples/foundation-profile-nano-server` to
      `features = ["nano"]` + Nano capacity config (≥1 session), minimal hand-rolled
      NODE MANAGER + address space satisfying Address Space Base / Base Info Core
      Structure. (T003 finding: the 041 sample binary never actually ran — base-server
      with no node manager exits at startup with "No node managers defined"; the 041
      benchmark only measured build size. The minimal node manager is a hard
      requirement, and the R10 baseline sizes are lower bounds of a non-functional
      server.) Verify
      T003–T005 green and record the measured size in `specs/054-profile-polish/research-assets/size-accounting.md`. [Cite: Nano profile URI; Core 2017
      CUs Address Space Base / Base Info Core Structure]

**Checkpoint (US1 = MVP)**: nano < 7,636,648 B; full workspace suite green; commit.

## Phase 3: User Story 2 — Micro-Profile Binary (P2)

**Goal**: `subscriptions` ON / `subscriptions-standard`+`events` OFF is a real build:
basic data-change monitoring works; deadband/triggering/events/methods compiled out.

### Tests (red first)

- [ ] T022 [P] [US2] Micro smoke test in
      `samples/foundation-profile-micro-server/tests/profile_smoke.rs`: CreateSubscription
      → CreateMonitoredItems (value change, queue size 1) → Publish notification flow,
      including publish-queue overflow handling; two parallel sessions. [Cite: Embedded
      DataChange Subscription facet CUs — Monitor Basic/Items 2/QueueSize_1/Value Change,
      Subscription Basic/Publish Min 02, PublishRequest Queue Overflow; Session Minimum 2
      Parallel; OPC 10000-4 §5.13/§5.14]
- [ ] T023 [P] [US2] Micro rejection test in
      `samples/foundation-profile-micro-server/tests/service_rejection.rs`: deadband
      DataChangeFilter → `BadMonitoredItemFilterUnsupported`; EventFilter item →
      `BadMonitoredItemFilterUnsupported`; SetTriggering → `BadServiceUnsupported`;
      Call → `BadServiceUnsupported`; sessions stay healthy. RED until T024/T025.
      [Cite: OPC 10000-4 §7.22.2 DataChangeFilter, §5.13.5 SetTriggering]

### Implementation

- [ ] T024 [US2] Split deadband out of `subscriptions/monitored_item.rs` into
      `subscriptions/monitored_item/filters.rs` gated by `subscriptions-standard`; the
      ungated path rejects filter-bearing create/modify with
      `BadMonitoredItemFilterUnsupported` (fail-closed, never silently ignore a filter).
      Pure move + gate; deadband tests (`monitored_item.rs:1293,1337`) ride along.
      [Cite: OPC 10000-4 §7.22.2; OPC 10000-8 §7.2 PercentDeadband]
- [ ] T025 [US2] Split triggering into `subscriptions/monitored_item/triggering.rs` +
      orchestration gates (`session_subscriptions.rs:571`, `subscriptions/mod.rs:1425`,
      SetTriggering dispatch arm) behind `subscriptions-standard`.
      [Cite: OPC 10000-4 §5.13.5 SetTriggering]
- [ ] T026 [US2] Switch `samples/foundation-profile-micro-server` to
      `features = ["micro"]` + capacity config (≥2 sessions, ≥1 subscription, ≥2
      monitored items); verify T022/T023 + absence script (no deadband/trigger/event
      sentinels) green; record measured size in research-assets/size-accounting.md (must
      sit strictly between nano and embedded). [Cite: Micro profile URI]

**Checkpoint**: nano < micro (monotonic); full suite green; commit.

## Phase 4: User Story 3 — Embedded-Profile Binary (P3)

**Goal**: `embedded` alias = micro + `subscriptions-standard` + `method-call` +
`generated-address-space` + `aws-lc-rs`; secure channel + standard-tier monitoring +
GetMonitoredItems/ResendData work; advertised capabilities honest.

### Tests (red first)

- [ ] T027 [P] [US3] Embedded smoke test in
      `samples/foundation-profile-embedded-server/tests/profile_smoke.rs`: Basic256Sha256
      Sign&Encrypt channel against the server's application-instance certificate;
      deadband-filtered monitored item; SetTriggering; Call GetMonitoredItems + ResendData;
      browse confirms the type system is exposed (Base Info Type System). [Cite: Embedded
      2017 CUs — Security Default ApplicationInstance Certificate, Security Policy
      Required, Base Info Type System, Standard DataChange facet CUs; OPC 10000-4 §5.11]

### Implementation

- [ ] T028 [US3] Gate the builtin Server methods GetMonitoredItems/ResendData
      (`node_manager/memory/core.rs:1030-1068`) under `method-call` +
      `subscriptions-standard`; when gated out they are absent (not faulting stubs).
      [Cite: OPC 10000-5 §9.1 GetMonitoredItems / §9.2 ResendData]
- [ ] T029 [US3] Advertised-capability honesty (FR-004): cfg-adjust
      `node_manager/memory/core.rs:712-806` capability/limit nodes,
      `HistoryServerCapabilities` (`:798+`, `config/capabilities.rs`), and
      `OperationalLimits` fields so gated-out services advertise false/absent; verify
      against the full build (all-on ⇒ unchanged values). [Cite: OPC 10000-5 §6.3.2
      ServerCapabilitiesType; spec FR-004]
- [ ] T030 [US3] Switch `samples/foundation-profile-embedded-server` to
      `features = ["embedded"]` + capacity config (≥2 subscriptions, ≥100 monitored
      items) + demo certificate provisioning for the smoke test; verify T027 + absence
      script (no alarm/history/query sentinels) green; record measured size in
      research-assets/size-accounting.md.
      [Cite: Embedded profile URI]

**Checkpoint**: micro < embedded; full suite green; commit.

## Phase 5: User Story 4 — Standard-Profile Binary (P4)

**Goal**: `standard` alias = embedded + `discovery-server-registration`; X509 user
tokens, LDS self-registration, Cancel proven; still no events/history/etc.

### Implementation (crate skeleton FIRST — tests need a crate to live in)

- [ ] T031 [US4] Create `samples/foundation-profile-standard-server` (workspace member,
      `publish = false`, `features = ["standard"]`, default-features off) with capacity
      config per data-model.md (≥50 sessions, ≥5 subscriptions, ≥500 monitored items);
      wire into the pre-push no-default check set. [Cite: Standard profile URI; Standard
      2017 CUs Session Minimum 50 Parallel, Enhanced DataChange facet]

### Tests (red against unfinished composition)

- [ ] T032 [P] [US4] Standard smoke test in
      `samples/foundation-profile-standard-server/tests/profile_smoke.rs`: X509
      user-token activation over Sign&Encrypt; RegisterServer2 flow against an in-process
      `lds`-featured peer server (valid under the test-graph unification caveat — the
      binary absence check is separate); Cancel of an outstanding request per Part 4
      §5.7.5. [Cite: Standard 2017 CUs — Security User X509, Discovery
      Register/Register2, Session Cancel; OPC 10000-4 §5.7.5; OPC 10000-12 §4.2.2]

### Verification

- [ ] T033 [US4] Verify the standard composition end-to-end: T032 green, absence script
      (no alarm/history/query/gds sentinels) green, measured size strictly between
      embedded and simple-server; record size in research-assets/size-accounting.md. [Cite: spec SC-001]

**Checkpoint**: embedded < standard < full; full suite green; commit.

## Phase 6: User Story 5 — Measured Size Matrix and CI Guard (P5)

- [ ] T034 [P] [US5] Measurement script `tools/footprint.sh`: builds each of the six
      matrix packages in an ISOLATED cargo invocation (`--locked --profile embedded`),
      emits bytes + MiB + markdown rows; used identically by docs and CI (research.md
      R10 caveat is a hard rule in the script). [Cite: research.md R10/R12]
- [ ] T035 [P] [US5] Lattice-check script `tools/check-feature-lattice.sh` (FR-006):
      each alias standalone (`cargo check -p async-opcua --no-default-features
      --features <alias>`), each of the 15 gates individually disabled from the full
      server-crate surface (enumerated via `--no-default-features` + all-but-one),
      no-default baseline; script header documents the sampling rationale (not 2^15).
      Script only — no workflow edits (those are T036).
      [Cite: spec FR-006; plan.md Complexity Tracking]
- [ ] T036 [US5] Rework `.github/workflows/ci_footprint.yml` (single owner of this
      file): 4-profile matrix (add standard row), per-row guards — `cargo tree -e
      features` deny-list + symbol spot-check via `tools/check-profile-absence.sh`,
      `$GITHUB_STEP_SUMMARY` table rows from `tools/footprint.sh`, and a lattice job
      running `tools/check-feature-lattice.sh`. [Cite: spec FR-009; spec FR-006]
- [ ] T037 [US5] `docs/setup.md`: replace the 041 benchmark section with the six-row
      measured matrix (bytes, MiB, delta per rung) + provenance (arch/profile/rustc/date)
      + one-package-per-invocation caveat + feature-unification note + Nano/Micro
      security-posture note (policy None, plaintext-or-cert-encrypted tokens, pure-Rust
      crypto Marvin caveat) + "benchmarks, not certification" scope language.
      [Cite: spec FR-008; research.md R7]

**Checkpoint**: CI green with visible size tables; leak test (temporarily enabling an
excluded feature) turns the row red; commit.

## Phase 7: User Story 6 — Further-Savings Report (P6)

- [ ] T038 [US6] Symbol/section accounting for the four profile binaries (`cargo bloat`
      if available, else `nm --size-sort`/`size -A`), extending the per-story size log in
      `specs/054-profile-polish/research-assets/size-accounting.md` with evidence tables.
      [Cite: research.md R11]
- [ ] T039 [US6] Write `docs/profile-size-report.md`: ≥5 ranked, non-overlapping
      suggestions (seed list research.md R11 — pruned type-only nodeset, None-only crypto
      build, tokio slimming, DateTime/chrono replacement, config-parsing gate,
      panic-machinery trade-off, monomorphization hotspots), each with blocking
      architectural constraint, measured evidence, risk/effort class, and future-feature
      scope boundary. [Cite: spec FR-010/SC-005]

**Checkpoint**: report reviewed against SC-005; commit.

## Phase 8: Polish & Final Verification

- [ ] T040 Full pre-push gate: fmt, clippy `--workspace --all-targets --all-features`,
      `RUSTFLAGS="-D warnings"` no-default checks (incl. all four foundation-profile
      crates), lattice script (T036), `cargo deny check advisories`, full workspace
      tests, integration suite. [Cite: SESSION-HANDOFF pre-push gate]
- [ ] T041 Cross-doc consistency: contracts/feature-aliases.md, data-model.md, README
      feature list, `docs/compatibility.md` (if it enumerates features) all match the
      shipped gate/alias set; walk spec.md Success Criteria SC-001–SC-005 and check each
      off with evidence links in this file. [Cite: spec.md Success Criteria]

## Dependencies & Execution Order

- Setup (T001–T002) blocks everything.
- US1 (T003–T021) is the MVP and blocks US2 (needs gates + spine), US2 blocks US3
  (subscriptions-standard), US3 blocks US4 (embedded base). US5 needs US1–US4 sizes;
  US6 needs US5's binaries/accounting.
- Within each story: test tasks [P] first (committed red), then gates in listed order
  (spine → trait → module for US1), sample switch last. Exception US4: crate skeleton
  (T031) precedes its tests (T032) — a test file needs a crate to live in.
- Parallel opportunities: T003/T004/T005; T022/T023; gates T009–T020 touch mostly
  disjoint modules and may proceed as parallel dispatches ONLY if same-file collisions
  are excluded — tasks touching `session/message_handler.rs` (T006, T009, T010, T011,
  T012, T018) MUST be serialized (memory `feature-034` parallel-PR same-file hazard);
  `lib.rs` module-decl edits (T008, T015–T020) likewise serialize on that file.

## Implementation Strategy

MVP = Phase 1 + Phase 2 (US1): after T021 the Nano build exists, measured, guarded, and
the full build is regression-proven — ship-worthy alone. Each later story is one rung and
one commit. If any single gate proves architecturally infeasible within its task (e.g.
trait-surface fallout too broad), STOP, record the constraint, and move that cut to the
US6 report rather than shipping a half-gate (constitution I/II).
