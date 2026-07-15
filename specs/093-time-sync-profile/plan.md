# Implementation Plan: Time Sync Profile Decision

**Branch**: `093-time-sync-profile` | **Date**: 2026-07-15 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `specs/093-time-sync-profile/spec.md`

## Summary

Resolve all 7 Time Sync conformance units (currently `gap` in CU-COVERAGE.md) by
introducing a `TimeSyncSource` extensibility trait on the server, shipping two
built-in implementations, and documenting the rest as extension points:

- **`OsClockSource`** (always available, default) — satisfies CU 2478 (OS-based).
- **`max_acceptable_clock_skew`** server config field — satisfies CU 3802.
- **`UaHeaderTimeSyncSource`** (feature-gated, reuses the client crate) — polls a
  well-known OPC UA endpoint's `ResponseHeader.timestamp` to observe skew;
  satisfies CU 5505.
- **CU 5793** (support ≥1 mechanism) closes as a consequence of 2478/5505.
- **CU 2479 (PTP), 2480 (gPTP), 2786 (NTP)** — documented as satisfiable only via a
  user-supplied `TimeSyncSource`, grounded in OPC-10000-84 §6.6.3.6, which treats
  gPTP time sync as a network-stack dependency rather than an OPC UA
  application-layer concern.

Then regenerate `specs/conformance-tester/CU-COVERAGE.md` from an extended
`tools/cu-coverage-report` classification, and add per-profile time-sync
documentation.

## Technical Context

**Language/Version**: Rust (workspace edition, matches repo)
**Primary Dependencies**: none new. `UaHeaderTimeSyncSource` reuses the already-optional
`async-opcua-client` dependency of `async-opcua-server` (present since feature 024's
`discovery-server-registration`). `tokio` (already core) for the periodic poll loop.
**Storage**: N/A (runtime state only; config via existing serde `ServerConfig`)
**Testing**: `cargo test` / `cargo nextest`; unit tests in-crate, integration tests in
`async-opcua/tests/integration/`, doc/example under `samples/` or `async-opcua-server/examples/`
**Target Platform**: all supported (Linux/Windows/macOS); `OsClockSource` and config are
`no_std`-profile-safe (system clock only); `UaHeaderTimeSyncSource` is feature-gated off by
default so nano/micro footprint profiles do not pay for it.
**Project Type**: Rust protocol-stack library (multi-crate workspace)
**Performance Goals**: N/A — a periodic (default order-of-minutes) background poll; no hot-path impact.
**Constraints**: no new always-on dependency; nano/micro profiles must not grow; poll failure must
fail-safe (report `synchronized = false`), never panic; no secret/timestamp-source leakage in logs.
**Scale/Scope**: ~1 trait + 2 built-in impls + 1 config field + 1 small client method + report-tool
classification extension + docs + tests. Bounded, single-feature scope.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment |
|---|---|
| **I. Correctness Over Completion** | The poller consumes a `ResponseHeader.timestamp` from a remote endpoint. Even though that endpoint is operator-configured/trusted over the existing secure channel, the parse path MUST NOT panic and MUST treat unreachable/malformed/absent timestamps as `synchronized = false` (FR-009). Edge cases (never-polled, stale) are enumerated in the spec and will be tested. **PASS** with the fail-safe requirement carried into tasks. |
| **II. Do It Right Once** | No `// TODO` on reachable paths; the client `ResponseHeader.timestamp` gap is fixed properly by surfacing it through a small dedicated method, not by copy-pasting the discovery internals. **PASS** |
| **III. Individual Task Discipline** | Work decomposes cleanly into per-story, per-CU tasks (trait, OsClockSource, config, client method, UA source, report tool, docs, each with its own test). `/speckit-tasks` will keep one task per line. **PASS** |
| **IV. Security Is Paramount** | Reachable-from-network consideration: the timestamp is attacker-influenceable only if the operator points the poller at a hostile endpoint; regardless, it is only used to *report* observed skew, never to set the local clock or make a trust decision — so a lying source cannot escalate. Fail-closed: a failed/absent poll reports NOT synchronized rather than asserting sync. No secrets logged. No new advisory surface (no new deps). **PASS** |
| **V. Leave It Better Than You Found It** | Closes 7 `gap` rows, adds an evidence category the report tool was missing, and adds previously-absent time-sync docs. No debris. **PASS** |

**Result**: PASS. No violations; Complexity Tracking table not required.

## Project Structure

### Documentation (this feature)

```text
specs/093-time-sync-profile/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output (trait + config + client-method contracts)
├── checklists/
│   └── requirements.md  # from /speckit-specify
└── tasks.md             # Phase 2 output (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
async-opcua-server/
├── src/
│   ├── time_sync.rs            # NEW: TimeSyncSource trait, TimeSyncMechanism, TimeSyncStatus,
│   │                           #      OsClockSource (always available)
│   ├── time_sync_ua.rs         # NEW (cfg feature): UaHeaderTimeSyncSource + poll loop
│   │                           #      (mirrors discovery.rs structure)
│   ├── lib.rs                  # re-export time_sync items; gate time_sync_ua
│   ├── builder.rs              # with_time_sync_source(...); install OsClockSource default in build()
│   ├── info.rs                 # ServerInfo gains time_sync_source: Arc<dyn TimeSyncSource>
│   ├── server.rs               # spawn UaHeaderTimeSyncSource poll loop on startup (cfg-gated)
│   └── config/
│       ├── server.rs           # max_acceptable_clock_skew_ns field (serde) + accessor
│       └── ...                 # constants.rs: DEFAULT_MAX_ACCEPTABLE_CLOCK_SKEW_NS
├── examples/
│   └── custom_time_sync.rs     # NEW: US3 minimal custom TimeSyncSource (NTP/PTP/gPTP stand-in)
└── Cargo.toml                  # NEW feature `time-sync-ua = ["async-opcua-client"]`

async-opcua-client/
└── src/session/client.rs       # NEW: method returning server ResponseHeader.timestamp from a
                                #      discovery (GetEndpoints) call (surfaces what get_endpoints drops)

async-opcua/
└── tests/integration/
    └── time_sync.rs            # NEW: OsClockSource default + UaHeaderTimeSyncSource two-instance test

tools/cu-coverage-report/
└── src/lib.rs                  # extend EvidenceStatus with Extensible; reclassify the 7 CUs; update tests

specs/conformance-tester/
└── CU-COVERAGE.md              # REGENERATED (not hand-edited) after lib.rs change

docs/
├── time-synchronization.md     # NEW: per-profile claims + extension guide (FR-010/011)
└── opcua-foundation-profile-roadmap.md  # update Time Sync section
```

**Structure Decision**: Core trait + `OsClockSource` + config live in `async-opcua-server`
unconditionally (no optional deps, footprint-safe). `UaHeaderTimeSyncSource` and its poll loop
live behind a new off-by-default `time-sync-ua` Cargo feature that enables the already-optional
`async-opcua-client` dependency — the same gating pattern feature 024 used for
`discovery-server-registration`, so nano/micro profiles are unaffected. Report-tool and docs
changes are self-contained.

## Complexity Tracking

> No constitution violations — table intentionally omitted.
