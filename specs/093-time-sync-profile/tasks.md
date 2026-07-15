---

description: "Task list for Time Sync profile decision (feature 093)"
---

# Tasks: Time Sync Profile Decision

**Input**: Design documents from `/specs/093-time-sync-profile/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/time-sync-contracts.md, quickstart.md

**Tests**: Included. This is a correctness-critical, network-facing library
(constitution Principle I "Correctness Over Completion"), and every CU's Success
Criterion is stated as an automated test in spec.md. Test tasks are therefore
first-class, not optional.

**Organization**: Grouped by user story (US1 P1, US2 P2, US3 P3) for independent
delivery. Each task cites the OPC UA section it implements so the `after_tasks`
analyze pass can verify grounding.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1 / US2 / US3 (setup, foundational, and polish tasks carry no story label)

## Path Conventions

Multi-crate Rust workspace. Server core: `async-opcua-server/src/`. Client:
`async-opcua-client/src/`. Integration tests: `async-opcua/tests/integration/`.
Report tool: `tools/cu-coverage-report/src/`. Docs: `docs/`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create the module skeleton the rest of the feature fills in.

- [ ] T001 Create `async-opcua-server/src/time_sync.rs` as an empty module (module doc comment only) and declare `pub mod time_sync;` in `async-opcua-server/src/lib.rs`.
- [ ] T002 Add `DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_MS: u64 = 5000` to the `constants` module in `async-opcua-server/src/lib.rs` (documented as the CU 3802 default acceptable clock skew; OPC UA leaves the tolerance application-defined).

**Checkpoint**: Module and constant exist; workspace still builds.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The `TimeSyncSource` seam and its runtime home. **Every user story
depends on these types existing.**

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [ ] T003 Define `TimeSyncMechanism` enum (`OsClock`, `Ntp`, `Ptp`, `Gptp`, `UaHeaderBased`, `Custom(String)`) with `Debug, Clone, PartialEq, Eq` in `async-opcua-server/src/time_sync.rs` per contract C2; map each variant to its CU in a doc comment (2478/2786/2479/2480/5505; OPC-10000-84 §6.6.3.6 for the PTP/gPTP/NTP-are-network-layer framing).
- [ ] T004 Define `TimeSyncStatus` struct (`mechanism`, `synchronized: bool`, `last_sync: Option<opcua_types::DateTime>`, `observed_skew: Option<std::time::Duration>`) with `Debug, Clone` in `async-opcua-server/src/time_sync.rs` per contract C2; document the no-stale-freshness invariant (spec Edge Cases).
- [ ] T005 Define the object-safe `TimeSyncSource: Send + Sync` trait with `fn status(&self) -> TimeSyncStatus` in `async-opcua-server/src/time_sync.rs` per contract C1; doc-comment that `status()` must be non-blocking and panic-free.
- [ ] T006 Add `pub time_sync_source: Arc<dyn TimeSyncSource>` to `ServerInfo` in `async-opcua-server/src/info.rs` (instance-scoped per feature 049; mirror the existing `authenticator: Arc<dyn AuthManager>` field), and re-export `TimeSyncSource`, `TimeSyncMechanism`, `TimeSyncStatus` from `async-opcua-server/src/lib.rs`.

**Checkpoint**: Trait + types + runtime field compile; `ServerInfo` construction sites updated to require the new field.

---

## Phase 3: User Story 1 - OS Clock Time Sync and Configurable Skew (Priority: P1) 🎯 MVP

**Goal**: Ship the always-available `OsClockSource` default (CU 2478) and the
`max_acceptable_clock_skew` config field (CU 3802), so a zero-config server
correctly claims OS-based time sync and operators can set the tolerance.

**Independent Test**: A default-built server reports `OsClock`/synchronized; a
config with `max_acceptable_clock_skew_ms` round-trips and `0` falls back to the
default.

### Tests for User Story 1

- [ ] T007 [P] [US1] Unit test in `async-opcua-server/src/time_sync.rs` (`#[cfg(test)]`): `OsClockSource::status()` returns `mechanism == OsClock`, `synchronized == true`, `last_sync.is_some()`, `observed_skew == None` (CU 2478; timestamps per OPC-10000-4 §7.33 use `DateTime`).
- [ ] T008 [P] [US1] Unit test in `async-opcua-server/src/config/server.rs` (`#[cfg(test)]`): `max_acceptable_clock_skew_ms` serde default applies when absent, a set value round-trips, and `0` → `max_acceptable_clock_skew()` returns the default `Duration` (CU 3802, FR-005).

### Implementation for User Story 1

- [ ] T009 [US1] Implement `OsClockSource` (unit struct, `Debug, Default, Clone`) and `impl TimeSyncSource for OsClockSource` in `async-opcua-server/src/time_sync.rs` per contract C3 (CU 2478); `last_sync = DateTime::now()`.
- [ ] T010 [US1] Add `max_acceptable_clock_skew_ms: u64` (`#[serde(default = ...)]` → `DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_MS`) and a `max_acceptable_clock_skew(&self) -> Duration` accessor (with `0 → default`) to `ServerConfig` in `async-opcua-server/src/config/server.rs` per contract C5 (CU 3802).
- [ ] T011 [US1] Add `ServerBuilder::with_time_sync_source(Arc<dyn TimeSyncSource>)` storing `Option<Arc<dyn TimeSyncSource>>`, and in `ServerBuilder::build()` install `Arc::new(OsClockSource)` when unset before constructing `ServerInfo`, in `async-opcua-server/src/builder.rs` per contract C4 (mirror `with_authenticator`; FR-002/FR-003).

**Checkpoint**: Default server claims CU 2478; CU 3802 config works; US1 tests pass. **MVP complete.**

---

## Phase 4: User Story 2 - UA-Based Periodic Time Sync (Priority: P2)

**Goal**: Ship the feature-gated `UaHeaderTimeSyncSource` that periodically reads a
well-known endpoint's response-header timestamp to observe skew (CU 5505), plus
the client method that surfaces that timestamp.

**Independent Test**: Two local instances; the poller detects and reports offset
within one interval; an unreachable endpoint yields `synchronized = false`.

### Tests for User Story 2

- [ ] T012 [P] [US2] Integration test in `async-opcua/tests/integration/time_sync.rs`: stand up a local server, point a `UaHeaderTimeSyncSource` (short interval) at its discovery endpoint, and assert that within one interval `status().synchronized == true` and `observed_skew.is_some()` (CU 5505; GetEndpoints OPC-10000-4 §5.5.4, ResponseHeader timestamp §7.33). Gate the test with `#[cfg(feature = "time-sync-ua")]`.
- [ ] T013 [US2] Integration test in `async-opcua/tests/integration/time_sync.rs` (same file as T012 → sequential): point a `UaHeaderTimeSyncSource` at an unreachable endpoint and assert `status().synchronized == false` with no panic (FR-009). Gate with `#[cfg(feature = "time-sync-ua")]`.
- [ ] T013a [US2] Integration test in `async-opcua/tests/integration/time_sync.rs` (same file → sequential): after the poller has observed a skew, assert that both the configured tolerance (`ServerInfo.config.max_acceptable_clock_skew()`) and the source's `status().observed_skew` are retrievable together from `ServerInfo`, and that a caller can determine whether observed skew exceeds the configured tolerance — the excess is visible (FR-006, US2 Acceptance Scenario 3; ties CU 5505 to CU 3802). Gate with `#[cfg(feature = "time-sync-ua")]`.

### Implementation for User Story 2

- [ ] T014 [US2] Add `Client::get_server_time_via_endpoints(&self, server: impl ConnectorBuilder) -> Result<opcua_types::DateTime, Error>` to `async-opcua-client/src/session/client.rs` per contract C6: one session-less `GetEndpoints` round-trip returning `response_header.timestamp` (OPC-10000-4 §5.5.4, §7.33); never panics on error. Reuse the existing `get_server_endpoints_inner` path rather than duplicating it.
- [ ] T015 [US2] Declare Cargo feature `time-sync-ua = ["async-opcua-client"]` in `async-opcua-server/Cargo.toml` per contract C8 (off by default; same optional-client-dep mechanism as `discovery-server-registration`).
- [ ] T016 [US2] Implement `UaHeaderTimeSyncSource` (`endpoint_url`, `poll_interval`, `state: Arc<RwLock<TimeSyncStatus>>`), `new(...)`, and `impl TimeSyncSource` (reads shared state) in new `async-opcua-server/src/time_sync_ua.rs` behind `#[cfg(feature = "time-sync-ua")]`, declared in `lib.rs`; per contract C7 (mechanism `UaHeaderBased`; CU 5505). `poll_interval` is caller-supplied; clamp it up to a documented `MIN_UA_POLL_INTERVAL` floor (e.g. 1s) in `new(...)` so a pathologically small value cannot busy-loop the discovery endpoint (constitution Principle IV: bound self-inflicted resource use), and document that the caller owns the value otherwise (FR-008).
- [ ] T017 [US2] Implement the poll loop (mirror `async-opcua-server/src/discovery.rs`: `tokio::time::interval`, `MissedTickBehavior::Skip`, `CancellationToken` shutdown) that each tick calls T014, computes `observed_skew = |server_ts − local_now|` and sets `synchronized`/`last_sync` on success or `synchronized = false` on failure (FR-009, no stale freshness), in `async-opcua-server/src/time_sync_ua.rs`.
- [ ] T018 [US2] Spawn the poll loop at server startup when the active `time_sync_source` is a `UaHeaderTimeSyncSource`, in `async-opcua-server/src/server.rs` behind `#[cfg(feature = "time-sync-ua")]` (mirror the discovery-registration spawn site; wire shutdown to the server `CancellationToken`).

**Checkpoint**: With `--features time-sync-ua`, CU 5505 is a live, tested capability; US1 still passes with the feature off.

---

## Phase 5: User Story 3 - Documented Extensibility for NTP / PTP / gPTP (Priority: P3)

**Goal**: Turn the US1 trait into a usable, documented integration point for the
three network-layer mechanisms the library deliberately does not implement, and
state per-profile claims.

**Independent Test**: The example `TimeSyncSource` impl compiles against the trait
and is accepted by the builder; the docs state the per-profile claims and the
NTP/PTP/gPTP extension-point position.

### Tests for User Story 3

- [ ] T019 [P] [US3] Add `async-opcua-server/examples/custom_time_sync.rs`: a minimal `TimeSyncSource` reporting `mechanism: Ntp` with a fixed skew, registered via `with_time_sync_source`; it must compile and be exercised as a compile-check (CU 2479/2480/2786 as user-supplied; OPC-10000-84 §6.6.3.6). Ensure it is picked up by `cargo build --examples`.

### Implementation for User Story 3

- [ ] T020 [P] [US3] Write `docs/time-synchronization.md` per contract C10: a per-canonical-profile table (Nano 2266 / Micro 2267 / Embedded 2268 / Standard 2269) stating which Time Sync CUs are claimed and by which mechanism (built-in `OsClock`, built-in `UaHeaderBased`, or user-supplied) (FR-010), and an explicit statement that PTP/gPTP/NTP are not implemented in-library and require a user-supplied `TimeSyncSource`, citing OPC-10000-84 §6.6.3.6 (FR-011).

**Checkpoint**: All three built-in/documented paths exist; an integrator can wire NTP/PTP/gPTP.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Reflect all 7 resolved CUs in the coverage report and roadmap, and run
the full gate. Depends on US1–US3 being complete.

- [ ] T021 Extend `EvidenceStatus` in `tools/cu-coverage-report/src/lib.rs` with an `Extensible` variant (label `extensible`; note "Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library") and add its `evidence_note` arm, per contract C9.
- [ ] T022 Reclassify the 7 Time Sync CUs in `tools/cu-coverage-report/src/lib.rs`: empty/remove `time_sync_gaps()`, add 2478/3802/5505/5793 to `implemented_cus()`, add new `extensible_cus()` = {2479, 2480, 2786} routed to `Extensible` in `classify_cu`; update the unit-test assertions (currently asserting `| 2478 | ... | gap |`) to the new statuses (FR-012).
- [ ] T023 Regenerate `specs/conformance-tester/CU-COVERAGE.md` by running `cargo run -p async-opcua-cu-coverage-report -- "$ASYNC_OPCUA_PROFILE_SNAPSHOT_DIR/opcua-profile-normalized-snapshot.json" specs/conformance-tester/CU-COVERAGE.md` (do NOT hand-edit); confirm no Time Sync row reads `gap` (FR-012, SC-001).
- [ ] T024 [P] Update the Time Sync section of `docs/opcua-foundation-profile-roadmap.md` to reference `docs/time-synchronization.md` and reflect the resolved statuses (2478/3802/5505/5793 implemented, 2479/2480/2786 extensible).
- [ ] T025 [P] Update `TODO.md`: move the "Time Sync profile decision" item from Remaining to Done with a one-line summary referencing feature 093.
- [ ] T026 Run the pre-PR gate: `tools/ci-playbook.sh --ci`, plus the feature legs default CI misses — `cargo clippy -p async-opcua-server --features time-sync-ua --all-targets -- -D warnings` and `cargo test -p async-opcua-server --features time-sync-ua` and `cargo test -p async-opcua-cu-coverage-report`; fix any failure before considering the feature done (constitution: green before done).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies.
- **Foundational (Phase 2)**: depends on Setup; **blocks all user stories** (they all use `TimeSyncSource`/`TimeSyncStatus`).
- **US1 (Phase 3)**: depends on Foundational. MVP.
- **US2 (Phase 4)**: depends on Foundational; independent of US1 at runtime (its own feature) but builds on the same trait. The builder default (T011) makes US1 the natural first delivery.
- **US3 (Phase 5)**: depends on Foundational (needs the trait for the example) and on US1's builder wiring (T011) for registration.
- **Polish (Phase 6)**: depends on US1–US3 (T023's regenerated report and T024 docs assert the resolved state of all stories).

### Within Each User Story

- Tests are written to fail first, then implementation makes them pass (constitution: tests accompany fixes).
- US1: T007/T008 (tests) → T009/T010/T011 (impl).
- US2: T012/T013/T013a (tests) → T014 (client) → T015 (feature) → T016/T017/T018 (source + loop + spawn). T013a (FR-006 skew-vs-observed) needs no new production code beyond T006's `ServerInfo.time_sync_source` and T010's config accessor — it asserts the two are jointly retrievable.
- US3: T019 (example/test) alongside T020 (docs).

### Parallel Opportunities

- T001 ∥ T002 (Setup, different files).
- Within Foundational, T003/T004/T005 are the same file (`time_sync.rs`) → sequential; T006 follows.
- US1 tests T007 ∥ T008 (different files). US2 tests T012/T013/T013a share `time_sync.rs` integration file → sequential; all before impl.
- US3 T019 ∥ T020 (example vs docs). Polish T024 ∥ T025 (docs vs TODO).

---

## Implementation Strategy

### MVP First (User Story 1)

1. Phase 1 Setup → Phase 2 Foundational → Phase 3 US1.
2. **STOP and VALIDATE**: default server claims CU 2478; CU 3802 config round-trips. This alone moves 2478/3802/5793 off `gap` and is a shippable increment.

### Incremental Delivery

1. Foundation + US1 → CU 2478/3802 (+5793) live.
2. US2 → CU 5505 live (feature-gated).
3. US3 → CU 2479/2480/2786 documented extension point.
4. Polish → CU-COVERAGE regenerated, roadmap + TODO updated, full gate green.

### Notes

- One task at a time, committed individually (constitution Principle III; memory `commit-at-end-of-user-story` → one commit per user story is acceptable, but keep each task self-contained).
- Do not hand-edit `specs/conformance-tester/CU-COVERAGE.md` — it is generated (T023).
- Keep the `time-sync-ua` feature off by default so nano/micro footprints are unchanged; run its clippy/test legs locally (default CI misses them).
