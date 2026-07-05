# Implementation Plan: Backlog Closeout Batch

**Branch**: `058-backlog-closeout-batch` | **Date**: 2026-07-04 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/058-backlog-closeout-batch/spec.md`

## Summary

Close out the five remaining items from the completeness and TODO backlogs: (1) OCSP responder infrastructure to complement the live-fetch client from feature 057; (2) SDK node-manager tooling to reduce boilerplate for custom node managers; (3) an RSA-KEM encrypted UserName token integration test; (4) un-ignore the embedded profile secure channel smoke test via two-phase client connect; (5) un-ignore the standard profile X509 user token and RegisterServer2 tests.

## Technical Context

**Language/Version**: Rust 1.75+ (workspace, edition 2021)
**Primary Dependencies**: tokio, opcua-crypto (x509-cert, der, x509-ocsp, ureq), opcua-server, opcua-client
**Storage**: In-memory certificate status database (OCSP responder), N/A for other USs
**Testing**: cargo test (existing integration suite in `async-opcua/tests/`, profile smoke tests in `samples/foundation-profile-*/tests/`)
**Target Platform**: Linux, Windows, macOS (cross-platform)
**Project Type**: Library crate workspace + server binary + example binaries
**Performance Goals**: OCSP responder <1s response time (SC-001)
**Constraints**: Backward-compatible SDK API (FR-009), reuse existing OCSP codec from feature 057, no new mandatory dependencies, test certificates auto-generated at test time
**Scale/Scope**: 5 user stories across `async-opcua-crypto/ocsp/`, `async-opcua-server/src/node_manager/`, `async-opcua/tests/`, `samples/foundation-profile-embedded-server/tests/`, `samples/foundation-profile-standard-server/tests/`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Notes |
|-----------|-----------|-------|
| I. Correctness Over Completion | PASS | Each US has independent acceptance criteria with measurable outcomes |
| II. Do It Right Once | PASS | OCSP responder reuses the codec built in feature 057; SDK tooling extends existing NodeManager trait rather than replacing it; test improvements fix known gaps |
| III. Individual Task Discipline | PASS | Five independent USs, each self-contained and one-task-at-a-time viable |
| IV. Security Is Paramount | PASS | OCSP responder is network-facing (must fail-closed, bound resources); RSA-KEM test validates cryptographic path; no security-relevant defaults changed |
| V. Leave It Better Than You Found It | PASS | SDK tooling improves developer experience; un-ignoring deferred tests improves coverage; OCSP responder completes the PKI story |

**Gate: ALL PASS** — no violations, no complexity tracking needed.

## Project Structure

### Documentation (this feature)

```text
specs/058-backlog-closeout-batch/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── ocsp-responder.md
│   ├── node-manager-builder.md
│   └── test-harness.md
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
# US1 — OCSP responder
async-opcua-crypto/src/ocsp/
├── mod.rs                 # + responder module export
├── responder.rs           # NEW: OCSP response builder + signing
└── tests/
    └── ocsp_responder.rs  # NEW: responder unit tests

# US2 — SDK node-manager tooling
async-opcua-server/src/node_manager/
├── builder.rs             # NEW: ergonomic NodeManager builder API
└── mod.rs                 # re-export builder types
docs/
├── advanced_server.md     # update with builder examples
samples/node-managers/     # update to use builder where applicable

# US3 — RSA-KEM integration test
async-opcua/tests/
└── integration/
    └── rsa_kem.rs         # NEW: RSA-KEM encrypted UserName token test

# US4 — Embedded profile secure channel test
samples/foundation-profile-embedded-server/tests/
├── common/mod.rs          # + connect_secure_two_phase helper
└── profile_smoke.rs       # un-ignore secure_channel_basic256sha256_sign_encrypt

# US5 — Standard profile X509/RegisterServer2 tests
samples/foundation-profile-standard-server/tests/
├── common/mod.rs          # + connect_secure_two_phase + spawn_lds_peer helpers
└── profile_smoke.rs       # un-ignore x509_user_token_activation + register_server2_flow
```

**Structure Decision**: Follow existing workspace conventions. OCSP responder goes in `async-opcua-crypto/src/ocsp/` alongside the existing client code. SDK builder goes in `async-opcua-server/src/node_manager/` alongside the existing trait. Integration test for RSA-KEM follows the existing pattern in `async-opcua/tests/integration/`. Profile tests stay in their respective sample crates with enhanced test harness helpers.

## Complexity Tracking

*No violations — this section intentionally empty.*
