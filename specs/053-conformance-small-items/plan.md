# Implementation Plan: Conformance Small-Items Sprint

**Branch**: `053-conformance-small-items-sprint` | **Date**: 2026-07-02 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/053-conformance-small-items/spec.md`

## Summary

Close the entire remaining tail of the conformance-audit register in one sprint of 7 independent
user stories: P5-04 ServerDiagnostics mandatory children (the bulk of the work — 5 new mapped
diagnostics nodes fed from live session/subscription state), P4-ATTR-04 write EURange/enum
validation, P4-ATTR-03 LocalizedText write-locale gap closure (machinery mostly exists from
feature 049), P4-ATTR-02 maxAge freshness for refreshable sources, P8-02 event-driven EURange
re-resolution + one-shot SemanticsChanged bit, P3-09 AccessLevelEx attribute on Variables, and the
P5-03 verify-and-close (verified not-a-bug in Phase 0 — lock-in test + register update only).
Technical approach per story is settled in [research.md](research.md) with file:line anchors;
observable behavior is contracted in [contracts/service-behavior.md](contracts/service-behavior.md).

## Technical Context

**Language/Version**: Rust (workspace toolchain, edition 2021; MSRV per workspace `Cargo.toml`)
**Primary Dependencies**: workspace crates only — `async-opcua-server`, `async-opcua-nodes`,
`async-opcua-types` (generated diagnostics DataTypes already exist); no new external dependencies
**Storage**: N/A (in-memory server state; diagnostics computed on read)
**Testing**: `cargo test` — server-crate unit + test binaries, `async-opcua` integration suite
(in-process client↔server harness); red-first for behavior fixes; independent test authorship
**Target Platform**: Linux server (library crate, platform-neutral)
**Project Type**: library (OPC UA stack) — multi-crate Cargo workspace
**Performance Goals**: no hot-path regression: diagnostics arrays computed on read (not sampled
continuously); EURange re-resolution is event-driven O(changes), never per-sample; write-path
validation adds work only for Variables with modeled constraints
**Constraints**: no public-API breakage (additive only); network-reachable paths panic-free and
bounded (constitution §IV); `SessionSecurityDiagnosticsArray` is security-sensitive → admin-gated
**Scale/Scope**: 7 findings / 7 user stories; ~6 impl areas listed in quickstart.md; expected
net diff small-to-medium per story (US1 largest)

## Constitution Check

*GATE: evaluated against constitution v1.0.0 before Phase 0; re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Correctness over completion | PASS | verify-before-fix already applied (P5-03 verified not-a-bug in research; register citation §6.3.2→§6.3.3 corrected); red-first tests for each behavior change |
| II. Do it right once | PASS | US5 removes a documented deferral (ponytail note) via the event-driven design its own comment prescribes; US3 completes existing machinery instead of adding a parallel path |
| III. Individual task discipline | PASS | 7 independent stories, one task per codex dispatch, one commit per story (SC-007) |
| IV. Security is paramount | PASS | new write-path validation is reject-with-status (no panic); diagnostics exposure permission-gated, session-security admin-gated; EnabledFlag write privileged; no new deps |
| V. Leave it better | PASS | closes the register tail to zero open rows; stale ponytail comment removed with the fix |

**Post-Phase-1 re-check**: PASS — no violations introduced by the design; Complexity Tracking empty.

## Project Structure

### Documentation (this feature)

```text
specs/053-conformance-small-items/
├── plan.md              # This file
├── research.md          # Phase 0 — grounded decisions + code anchors (complete)
├── data-model.md        # Phase 1 — entities/state per story (complete)
├── quickstart.md        # Phase 1 — build/test/landing map (complete)
├── contracts/
│   └── service-behavior.md  # Phase 1 — client-observable contracts per story (complete)
├── checklists/requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks — NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
async-opcua-server/src/
├── diagnostics/server.rs            # US1: mapped-id coverage for the 5 new diagnostics nodes
├── diagnostics/node_manager.rs      # US7: verified correct (no change expected)
├── node_manager/memory/core.rs      # US1: read dispatch for new VariableIds
├── node_manager/memory/simple.rs    # US4: callback/sampler freshness decision
├── session/manager.rs               # US1: sessions iterator (new accessor)
├── session/instance.rs              # US1: session diagnostics row source
├── subscriptions/mod.rs             # US1: subscription enumeration; US5: range-change notice
├── subscriptions/subscription.rs    # US1: diagnostics getters (new)
├── subscriptions/monitored_item.rs  # US5: eu_range refresh + one-shot SemanticsChanged
└── address_space/utils.rs           # US2: EURange/enum write validation; US3: locale rules; US6 dispatch

async-opcua-nodes/src/variable.rs    # US6: access_level_ex field + read/set arms
async-opcua-types/                   # no changes expected (constants + structs already generated)

async-opcua/tests/integration/       # read.rs, write.rs, browse.rs, subscriptions.rs — test homes
specs/conformance-audit/FINDINGS.md  # register rows updated per story
```

**Structure Decision**: existing multi-crate workspace; all changes are additive within the
server/nodes crates at the file anchors mapped in research.md. No new crates, no new deps.

## Complexity Tracking

*No constitution violations — table intentionally empty.*
