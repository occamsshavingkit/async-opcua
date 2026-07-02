# Implementation Plan: OPC UA 2017 Profile Minimal Builds

**Branch**: `054-profile-polish` | **Date**: 2026-07-02 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/054-profile-polish/spec.md`

## Summary

Make each OPC UA 2017 server profile (Nano / Micro / Embedded / Standard) buildable as a
minimal binary by introducing per-subsystem compile-time features in `async-opcua-server`
(default ON — no change for direct dependents), composing them into `nano` / `micro` /
`embedded` / `standard` facade aliases, and letting requests for compiled-out services fall
through to the existing `BadServiceUnsupported` dispatch fallback. Where a subsystem is
entangled (deadband/triggering inside `monitored_item.rs`, `CreateMonitoredItem` in the
`NodeManager` trait), split modules / cfg-gate trait surface. Measure everything into a
docs size matrix + CI run-summary tables with per-profile leak guards, and deliver a
ranked further-savings report for cuts the current architecture cannot reach.

Profile compositions are normative per
[research-assets/PROFILES-2017.md](research-assets/PROFILES-2017.md) (OPC Foundation
profile DB snapshot 2026-07-02). Subsystem coupling was mapped by direct inspection —
see research.md R2–R8 for file:line grounding.

## Technical Context

**Language/Version**: Rust (workspace edition 2021), stable `rustc 1.96.0`  
**Primary Dependencies**: cargo additive features (existing precedent:
`generated-address-space`, `discovery-server-registration`, `discovery-mdns`, `ecc`,
`wss`); GitHub Actions `$GITHUB_STEP_SUMMARY`; `cargo tree` + `nm`/symbol inspection for
leak guards  
**Storage**: N/A  
**Testing**: full workspace suite (default features) must stay green — FR-005;
per-profile smoke tests using the in-tree client against each benchmark sample
(profile-mandated ops + excluded-service rejection); compile matrix over the feature
lattice (each alias standalone, each gate toggled off full, no-default baseline);
CI footprint jobs (one cargo invocation per package — feature unification poisons
combined builds, research.md R10)  
**Target Platform**: any; measured reference numbers on x86-64 Linux,
`--profile embedded`  
**Project Type**: library feature architecture + module splits + samples + CI + docs  
**Performance Goals**: no runtime regression in the full build (gating is compile-time)  
**Constraints**: additive-only features; every lattice combination compiles (FR-006);
network-reachable paths in gated builds fail closed (`BadServiceUnsupported` /
`Bad_MonitoredItemFilterUnsupported`), never panic (constitution IV); advertised
capabilities must match compiled surface (FR-004)  
**Scale/Scope**: ~13 new server-crate features; splits in `subscriptions/monitored_item.rs`
and the `NodeManager` trait surface; 4 facade aliases; 1 new sample crate
(`foundation-profile-standard-server`); CI workflow rework; docs; report

## Constitution Check

*GATE: evaluated against constitution v1.0.0 — PASS (re-checked post-design).*

- **I. Correctness Over Completion**: each profile build is verified three ways — behavior
  (client smoke against mandated CUs), rejection (excluded services fail closed, tested),
  and absence (dependency/symbol guards). The full build is verified by the unchanged
  workspace suite. No story ships with a known gap; if a gate turns out to be infeasible
  within the architecture, it moves *explicitly* to the further-savings report instead of
  shipping half-gated.
- **II. Do It Right Once**: gating follows the crate's existing cfg conventions
  (research.md R8) rather than inventing new mechanisms; entangled code is split into
  properly owned modules (not `#[allow(dead_code)]`-suppressed); the measurement script is
  shared between docs and CI so numbers can't drift.
- **III. Individual Task Discipline**: tasks.md keeps one gate / one split / one sample /
  one workflow change per task; codex dispatches get one task each with Part/§ or
  profile-CU citations.
- **IV. Security Is Paramount**: the wire still accepts requests for gated-out services —
  every such path must return the standard fault; fuzz-adjacent risk (decode of request
  types whose handler is compiled out) is covered by keeping *decode* surface intact
  (types crate untouched) and gating only *handling*. Identity-token posture per profile
  is explicit: Nano/Micro = policy None endpoints with plaintext-or-cert-encrypted
  password tokens (research.md R7); Embedded/Standard = real policies via aws-lc-rs.
  Excluded subsystems reduce attack surface — gating must never weaken an included path.
- **V. Leave It Better**: retires the "folklore" profile builds (041 samples that were
  full binaries with small address spaces) in favor of measured minimal builds; the
  further-savings report becomes the next size backlog. (The 041 samples were honest
  benchmarks of what existed; this feature makes the profile names mean what they say.)

## Project Structure

### Documentation (this feature)

```text
specs/054-profile-polish/
├── plan.md               # This file
├── research.md           # Phase 0: profile grounding, coupling map, decisions R1–R12
├── research-assets/      # PROFILES-2017.md + profiles-2017.json (normative grounding)
├── data-model.md         # Gate table, alias compositions, guard matrix
├── quickstart.md         # Build/measure/verify commands
├── contracts/feature-aliases.md   # Public feature-name contract (4 aliases + gates)
├── checklists/requirements.md
└── tasks.md              # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/Cargo.toml         # + subsystem features (default ON)
async-opcua-server/src/
├── lib.rs                            # cfg-gated module decls (alarms, history, gds, ...)
├── session/message_handler.rs        # cfg-gated dispatch arms → BadServiceUnsupported fallback
├── session/controller.rs             # cfg-gated LDS-registry arms (RegisterServer/2 receive)
├── subscriptions/                    # gate whole module (feature "subscriptions");
│   └── monitored_item.rs             # SPLIT: deadband+triggering → monitored_item/filters.rs,
│                                     #   triggering.rs (feature "subscriptions-standard")
├── session/services/subscription/    # event filter engine → feature "events"
├── node_manager/...                  # cfg-gated trait methods (CreateMonitoredItem etc.)
├── node_manager/memory/core.rs       # builtin methods GetMonitoredItems/ResendData gating;
│                                     #   advertised-capability adjustments (FR-004)
└── (alarms/, history/, aggregates/, gds/, fota/, programs/, rbac/, diagnostics/)
                                      # per-subsystem cfg gates at decl + registration points
async-opcua/Cargo.toml                # facade: nano/micro/embedded/standard aliases
samples/foundation-profile-nano-server/      # consume alias `nano`
samples/foundation-profile-micro-server/     # consume alias `micro`
samples/foundation-profile-embedded-server/  # consume alias `embedded`
samples/foundation-profile-standard-server/  # NEW — consume alias `standard`
.github/workflows/ci_footprint.yml    # 4-profile matrix, step-summary tables, leak guards
docs/setup.md                         # measured size matrix + posture/unification caveats
docs/profile-size-report.md           # further-savings report (US6)
```

**Structure Decision**: gating lives in `async-opcua-server` (the crate that owns the
code); the facade only composes. No new library crates. One new sample crate. The types
crate is deliberately untouched (decode surface stays complete in every build).

## Complexity Tracking

No constitution violations. Two structural risks tracked for the analyze pass:

| Risk | Mitigation |
|------|-----------|
| `NodeManager` trait surface changes per feature (cfg-gated methods) | additive-only: features add methods; all in-tree impls gated consistently; full build unchanged |
| Combination explosion (13 gates) | FR-006 CI checks: each alias standalone + each single gate off full + no-default; not the full 2^13 lattice (documented sampling rationale) |
