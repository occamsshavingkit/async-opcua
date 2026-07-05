# Implementation Plan: Spec Compliance Audit Fixes

**Branch**: `059-spec-compliance-audit-fixes` | **Date**: 2026-07-05 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/059-spec-compliance-audit-fixes/spec.md`

## Summary

Fix the 8 remaining open OPC UA specification compliance findings from the 2026-07-05 audit. An additional 13 of the original 23 findings have already been addressed in prior work. This plan covers the remaining gaps across session services, view services, discovery, secure channel lifecycle, and code hygiene.

## Technical Context

**Language/Version**: Rust 1.75+ (edition 2021)
**Primary Dependencies**: opcua-types, opcua-core, tokio (async runtime), rustls/openssl (TLS)
**Storage**: N/A (in-memory server state only)
**Testing**: cargo test (unit + integration)
**Target Platform**: Linux, Windows, macOS (cross-platform library)
**Project Type**: Library (OPC UA protocol stack with server, client, and pubsub crates)
**Performance Goals**: No regression — fixes are validation additions, code removal, or filter logic that adds negligible overhead
**Constraints**: Changes must not alter the wire protocol format; all fixes are server-side validation/filtering/lifecycle improvements
**Scale/Scope**: 6 files modified, 8 open findings across 4 crates

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Assessment |
|-----------|--------|------------|
| I. Correctness Over Completion | PASS | Each fix addresses a spec-mandated behavior gap. Fixes are validated against OPC UA spec sections. No known defects will remain after completion. |
| II. Do It Right Once | PASS | Each fix is a targeted, minimal change that addresses the root cause. No workarounds or shortcuts. |
| III. Individual Task Discipline | PASS | Each open finding is a separately addressable, independently verifiable task. |
| IV. Security Is Paramount | PASS | Three fixes (SESSION-04 runtime nonce validation, VIEW-02 input validation, VIEW-03 result mask enforcement) directly improve input validation and data leakage prevention. No fix weakens security. |
| V. Leave It Better Than You Found It | PASS | SC-04 removes dead code. SESSION-04 promotes debug_assert to runtime check. All changes improve code quality in their immediate area. |

**Gate Result**: PASS — all constitutional principles satisfied.

## Project Structure

### Documentation (this feature)

```text
specs/059-spec-compliance-audit-fixes/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
async-opcua-server/
├── src/
│   ├── session/
│   │   ├── manager.rs          # SESSION-04, SESSION-06
│   │   └── controller.rs       # SC-03, SC-04
│   ├── node_manager/
│   │   └── view.rs             # VIEW-02, VIEW-03
│   ├── info.rs                 # DISC-03, DISC-04
│   └── config/
│       └── server.rs           # SESSION-06 (min timeout config)
```

**Structure Decision**: Single monorepo workspace. All changes are in `async-opcua-server/src/`. No new crates or modules needed.

## Complexity Tracking

No constitutional violations. All changes are within existing modules and follow established patterns.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| None | N/A | N/A |
