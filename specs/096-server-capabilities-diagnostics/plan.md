# Implementation Plan: Server Capabilities & Diagnostics Conformance Completion

**Branch**: `096-server-capabilities-diagnostics` | **Date**: 2026-07-16 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/096-server-capabilities-diagnostics/spec.md`

## Summary

Close six required CUs (3911, 3912, 4053, 4055, 3196, 3808) for the Micro/
Embedded/Standard 2025 server profiles. Four independently-testable
increments, in priority order: (1) wire the remaining unwired
`ServerCapabilities`/`OperationLimits` Max* nodes to their existing config
fields — `MaxSessions` (already tracked as `Limits.max_sessions`),
`MaxMonitoredItemsQueueSize` (already tracked as
`SubscriptionLimits.max_monitored_item_queue_size`),
`MaxMonitoredItemsPerSubscription` and `MaxSubscriptionsPerSession` (both
already tracked), plus reporting the spec-valid `0` ("no limit") for
`MaxMonitoredItems`/`MaxSubscriptions`, which have no server-wide cap and
never will without adding new enforcement machinery out of this feature's
scope; (2) **[Phase 0 correction — no new code]** document why
`SamplingIntervalDiagnosticsArray` is correctly *not* exposed:
OPC-10000-5 §7.9/§12.8 makes this array conditional on the server using a
fixed set of sampling intervals, and this server's
`sanitize_sampling_interval` (monitored_item.rs:299-311) negotiates a
continuously-variable, client-requested interval per monitored item — the
precondition never holds, so non-exposure is the spec-conformant choice,
not a gap; (3) a test proving the already-wired `Locations` object
resolves via Browse; (4) a new `docs/server-capacity-limits.md`
enumerating the core capacity constants from `config/limits.rs` (which
also documents US2's non-exposure rationale).

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`)
**Primary Dependencies**: `async-opcua-server` (`node_manager/memory/core.rs`, `config/limits.rs`), `async-opcua-types` (generated `VariableId`/`node_ids.rs`)
**Storage**: N/A (in-memory `AddressSpace`, no persistence layer touched)
**Testing**: `cargo test`, integration tests in `async-opcua/tests/integration/` (`read.rs` for capability reads, `browse.rs` for the Locations object)
**Target Platform**: Cross-platform Rust library (server crate), `diagnostics` cargo feature (for EnabledFlag context in the doc note only)
**Project Type**: Rust workspace library — single crate area (`async-opcua-server/src/node_manager/memory/core.rs`)
**Performance Goals**: No new performance targets; this feature adds zero new steady-state overhead (US2 is documentation-only, not a new live computation)
**Constraints**: MUST NOT panic on malformed read requests (Constitution Principle IV); the `ServerCapabilities` Max* values MUST reflect what the server actually enforces or a spec-valid `0` (never a fabricated non-zero placeholder) — Constitution Principle I forbids reporting a number the server doesn't actually honor
**Scale/Scope**: 4 `ServerCapabilities` node wirings (US1) + 1 documentation note (US2, corrected from a full diagnostics array during Phase 0 research) + 1 test (US3) + 1 doc (US4), within `async-opcua-server/src/node_manager/memory/core.rs` and a new `docs/server-capacity-limits.md`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Each user story ships with its own
  test proving the reported value matches the real configured/enforced
  limit (SC-001/SC-002/SC-003), not just that the node returns non-null.
  `MaxMonitoredItems`/`MaxSubscriptions` report `0` honestly (no
  server-wide enforcement exists) rather than a fabricated number, per the
  Assumptions section. PASS.
- **II. Do It Right Once**: US1 wires to the *existing* config fields
  already used for enforcement elsewhere (no new parallel config surface);
  US2 was caught during Phase 0 research to be a non-gap once the CU's own
  conditional precondition is checked against the server's actual
  sampling-interval code — building the array anyway would have produced a
  technically-present but spec-*non-conformant* structure (NodeIds that
  churn as intervals vary), which Principle II's "root causes, not
  symptoms" argues against. PASS.
- **III. Individual Task Discipline**: Enforced at `/speckit-tasks` — each
  task is one file-scoped, independently verifiable change; the four user
  stories are themselves independently testable per spec.md. PASS (verified
  at task generation).
- **IV. Security Is Paramount**: `ServerCapabilities` reads are already
  behind the existing attribute-read authorization path (no new attack
  surface); US2 introduces no new code at all. No new network-reachable
  parsing of untrusted input is introduced. PASS.
- **V. Leave It Better Than You Found It**: `tools/cu-coverage-report`'s
  `AUDIT_TABLE` updated for all 6 CUs with file:line/test evidence on
  completion, mirroring feature 095's closing convention. PASS (verified at
  completion).

## Project Structure

### Documentation (this feature)

```text
specs/096-server-capabilities-diagnostics/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md         # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks, not this command)
```

### Source Code (repository root)

Single Rust workspace, no new crates or top-level directories. Touches:

```text
async-opcua-server/src/
├── node_manager/memory/core.rs   # ServerCapabilities Max* wiring (US1)
└── config/limits.rs              # No new fields expected; existing fields are the source of truth for US1

async-opcua/tests/integration/
├── read.rs                       # ServerCapabilities Max* read tests (US1)
└── browse.rs                     # Locations object Browse test (US3)

docs/
└── server-capacity-limits.md     # New capacity document (US4); also documents US2's non-exposure rationale

tools/cu-coverage-report/src/lib.rs        # AUDIT_TABLE updates for all 6 CUs
specs/conformance-tester/CU-COVERAGE.md    # Regenerated
```

**Structure Decision**: Extends the existing `node_manager/memory/core.rs`
match-arm dispatch pattern already used for the 15 currently-wired
`ServerCapabilities` nodes, and the existing `DiagnosticsArrayKind` pattern
already used for `SubscriptionDiagnosticsArray`/session diagnostics arrays
— no new architectural surface, no new crate, no new top-level module.

## Complexity Tracking

*No constitution violations — this section is not applicable.*
