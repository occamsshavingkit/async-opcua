# Implementation Plan: Complete the 27 Partial Conformance Units

**Branch**: `109-partial-cu-completion` | **Date**: 2026-07-23 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `specs/109-partial-cu-completion/spec.md`

## Summary

Close all 27 Partial CUs (excluding the 3 Extensible time-sync CUs) to Implemented. 19 are **test-only** (code exists and is correct; add an independent test asserting the CU's spec behavior). 8 are **implementation-gap** (a value/reference/audit-event is never populated/wired/emitted; add the minimal fix + test): 2811, 2814, 2918, 2950, 3542, 3968, 3546, 3194. All cross-CU design judgment is resolved in [research.md](./research.md); the decisive one (CU 2823) is resolved TEST-ONLY against Part 2 §6.6 / CR 1.11, which make lockout explicitly optional. Each task targets one CU, is spec-grounded via [contracts/cu-spec-map.md](./contracts/cu-spec-map.md), and is sized for a small local coding model.

## Technical Context

**Language/Version**: Rust (workspace edition 2021)
**Primary Dependencies**: async-opcua workspace crates (`async-opcua-server`, `-types`, `-core`, `-nodes`, `-history-sqlite`, `tools/cu-coverage-report`); `chrono` (TimeZoneDataType); `tokio` (async tests)
**Storage**: history backends — in-memory + `async-opcua-history-sqlite` (SQLite); no new storage
**Testing**: `cargo test` (unit `#[cfg(test)]` modules + `async-opcua-server/tests/*.rs`); OPC-UA reference MCP for spec grounding
**Target Platform**: Linux (library / server crate)
**Project Type**: Rust library workspace (OPC UA protocol stack)
**Performance Goals**: none new — no hot-path change intended; the CU 2823 decision explicitly avoids adding per-request state to the auth path
**Constraints**: no new wire encoding; no new lock on the authentication hot path (FR-006); no-default-features workspace build stays green; the 3 Extensible + 141 Gap CUs untouched
**Scale/Scope**: 27 CUs across 8 subsystems; ~19 test-only + 8 small-fix tasks + 1 ledger/regeneration task

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

- **I. Correctness Over Completion (NON-NEGOTIABLE)** — PASS. The feature closes correctness/coverage gaps. CU 2950 fixes a real bug (ignored `timestamps_to_return`). Tests assert spec-mandated behavior on edge/error paths, not just happy paths.
- **II. Do It Right Once** — PASS. Type-B fixes address root causes (populate the value / wire the reference / emit the event), not symptoms. No `// TODO` on reachable paths; the single deliberate non-implementation (CU 2823 lockout) is explicitly recorded with spec justification (research.md R1) — the "recorded deliberate shortcut" the principle permits.
- **III. Individual Task Discipline** — PASS, and central here. tasks.md keeps one CU per task; the downstream model executes one at a time. No batching.
- **IV. Security Is Paramount** — PASS. CU 2422 (encrypted audit) and 2823 (invalid user token) are handled with care. The 2823 decision *rejects* an escalating-lockout map precisely because it would be an attacker-influenced unbounded allocation on a network-reachable path — the principle drove the choice.
- **V. Leave It Better Than You Found It** — PASS. Every touched CU ends fully tested with fresh ledger evidence; the coverage ledger becomes more accurate.

**Result: PASS, no violations. Complexity Tracking omitted (nothing to justify).**

## Project Structure

### Documentation (this feature)

```text
specs/109-partial-cu-completion/
├── plan.md              # This file
├── research.md          # Phase 0 — decisions (R1 CU 2823 resolved; R3-R10 Type-B insertion points)
├── data-model.md        # Phase 1 — changed data shapes (all additive/minimal)
├── quickstart.md        # Phase 1 — per-task loop + end-gate verification
├── contracts/
│   └── cu-spec-map.md    # Phase 1 — CU → OPC UA Part/§ grounding contract (authoritative)
├── checklists/
│   └── requirements.md   # Spec quality checklist (passed)
└── tasks.md             # Phase 2 — /speckit-tasks (NOT created here)
```

### Source Code (existing directories touched)

```text
async-opcua-server/src/
├── alarms/              # 2275, 2811(shelving), 2814(shelving), 2918, 4466
├── programs/            # 2811, 2814 (ProgramStateMachine)
├── rbac/                # 3539, 3540, 3541, 3542 (role_management.rs)
├── session/
│   ├── audit.rs         # 3224, 3542, 3968, 2422 (dispatch_*)
│   └── negotiate.rs     # 2823 (tarpit)
├── subscriptions/       # 2318, 2818, 3142, 5208, 3544
├── node_manager/memory/
│   ├── core.rs          # 2476, 3546, 3194 (Server_LocalTime, ServerCapabilities)
│   └── simple.rs        # 2950 (history_read_raw_modified timestamps fix)
├── history/             # 2289, 2950, 3968 (HistoryUpdate)
└── config/              # 3194 (new event-filter limit fields, if needed)

async-opcua-nodes/src/object.rs        # 2918 (has_event_source call site)
async-opcua-history-sqlite/            # 2289, 2950 (sqlite backend tests)
async-opcua-server/tests/*.rs          # integration tests per R12 map
samples/custom-codegen/                # 3201
tools/cu-coverage-report/src/lib.rs    # ledger flip (all 27)
specs/conformance-tester/CU-COVERAGE.md# regenerated artifact
```

**Structure Decision**: no new crates, modules, or directories. Every change lands in an existing file next to the code/tests for its subsystem. New tests live in the subsystem's existing `#[cfg(test)]` module or the mapped `tests/*.rs` integration file (R12).

## Complexity Tracking

No constitution violations — table intentionally omitted.
