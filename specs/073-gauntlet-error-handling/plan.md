# Implementation Plan: Gauntlet Error-Handling Fixes

**Branch**: `072-gauntlet-error-handling` | **Date**: 2026-07-12 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/073-gauntlet-error-handling/spec.md`

## Summary

Fix 20 remaining OPC UA Gauntlet error-handling failures from issue #282. The server currently
rejects NodeManagement operations with `BadServiceUnsupported` per-item, and has incorrect
status codes for SetTriggering, QueryFirst, and HistoryUpdate edge cases. The fix adds
service-layer input validation that returns OPC UA Part 4 specified operation-level error codes
for bad inputs, without implementing full service semantics.

## Technical Context

**Language/Version**: Rust (stable)
**Primary Dependencies**: tokio (async runtime), async-opcua-types (OPC UA types), async-opcua-core (messages/types)
**Storage**: In-memory address space (Nodes, References) via `AddressSpace`
**Testing**: `cargo test` with existing integration test harness in `async-opcua/tests/integration/`
**Target Platform**: Linux server (and cross-platform via Rust)
**Project Type**: Library + demo server binary
**Performance Goals**: No regression — validation overhead at service boundary should be O(1) per operation
**Constraints**: Must not panic on untrusted input; must fail closed; no new dependencies
**Scale/Scope**: 20 Gauntlet test failures across 4 service areas

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|-----------|-------|--------|
| I. Correctness Over Completion | Each operation-level status code is grounded in a spec table | PASS |
| II. Do It Right Once | Validation is added at the single correct layer (service boundary before dispatch) | PASS |
| III. Individual Task Discipline | Plan decomposes into independently verifiable tasks per operation type | PASS |
| IV. Security Is Paramount | Input validation bounds untrusted input; no panics on decode path; fail-closed defaults | PASS |
| V. Leave It Better Than You Found It | Fixes existing wrong status codes; adds regression tests | PASS |

**Gate result**: All principles pass. No violations to justify.

## Project Structure

### Documentation (this feature)

```text
specs/073-gauntlet-error-handling/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/
├── session/
│   ├── message_handler.rs           # Service dispatch (unchanged)
│   └── services/
│       ├── node_management.rs       # PRINCIPAL: Add input validation before NM dispatch
│       ├── query.rs                 # QueryFirst status fix
│       ├── attribute.rs             # HistoryUpdate status fix
│       └── mod.rs                   # Service infrastructure (unchanged)
├── subscriptions/
│   └── actor.rs                     # SetTriggering validation
└── node_manager/
    ├── mod.rs                       # NodeManager/NodeMutator trait definitions
    └── memory/
        └── memory_mgr_impl.rs       # Memory manager — existing validation from 048

async-opcua/tests/integration/
├── read.rs                          # Existing (updated for NodeManagement validation tests)
├── write.rs                         # Existing
├── methods.rs                       # Existing
├── subscriptions.rs                 # Add SetTriggering validation test
└── history.rs                       # Add HistoryUpdate tests
```

**Structure Decision**: Single Rust workspace (existing layout). Changes touch server service handlers
for validation and integration tests for regression coverage.

## Complexity Tracking

No violations to justify.
