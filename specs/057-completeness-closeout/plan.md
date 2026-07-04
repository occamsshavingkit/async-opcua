# Implementation Plan: Completeness Closeout

**Branch**: `057-completeness-closeout` | **Date**: 2026-07-04 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/057-completeness-closeout/spec.md`

## Summary

Close out the four remaining items from the completeness backlog: (1) live OCSP revocation fetching to complete Part 4 §6.1.3; (2) per-endpoint certificate configuration to support mixed RSA+ECC servers (Part 4 §5.5.4.1); (3) remove the LegacyCall dynamic-dispatch variant from the subscription actor; (4) add four "bad ideas" example servers demonstrating SDK flexibility including a chat server implementing the `cactuaroid/OpcUaChatServer` information model for interop testing.

## Technical Context

**Language/Version**: Rust 1.75+ (workspace, edition 2021)
**Primary Dependencies**: tokio, opcua-crypto (x509-cert, der, p256/p384, rsa), reqwest (OCSP HTTP), aws-lc-rs
**Storage**: CertificateStore (in-memory + pki directory), N/A for other USs
**Testing**: cargo test (existing suites: core 89, server 306, nodes 48)
**Target Platform**: Linux, Windows, macOS (cross-platform)
**Project Type**: Library crate workspace + server binary + example binaries
**Performance Goals**: OCSP fetch <5s timeout (FR-005), no subscription path regressions (FR-014)
**Constraints**: Backward-compatible config API (FR-010), no new mandatory dependencies, OCSP default-off (FR-006)
**Scale/Scope**: 4 user stories across `async-opcua-crypto`, `async-opcua-server`, `async-opcua-server/src/subscriptions/`, and 4 new `samples/` crates

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Notes |
|-----------|-----------|-------|
| I. Correctness Over Completion | PASS | Each US has independent acceptance criteria; SC-001 through SC-006 are measurable and verifiable |
| II. Do It Right Once | PASS | Per-endpoint certs replace the global cert model cleanly; LegacyCall removal is the final phase of a multi-feature refactor |
| III. Individual Task Discipline | PASS | Four independent USs, each decomposable into sequential tasks. US1–US4 can proceed in parallel |
| IV. Security Is Paramount | PASS | OCSP must fail-closed (FR-004 strict/soft/off), default-off (FR-006), timeout/resource bounds (FR-005). Multi-cert startup validation prevents silent mismatches (FR-009) |
| V. Leave It Better Than You Found It | PASS | Multi-cert eliminates a documented limitation. LegacyCall removal eliminates the last dynamic dispatch in the actor. Bad ideas servers add SDK documentation value |

**Gate: ALL PASS** — no violations, no complexity tracking needed.

## Project Structure

### Documentation (this feature)

```text
specs/057-completeness-closeout/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── ocsp-fetch-policy.md
│   ├── endpoint-cert-config.md
│   ├── subscription-commands.md
│   └── chat-server-model.md
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
# Existing crates touched
async-opcua-crypto/
├── src/
│   ├── certificate_store.rs       # + OCSP fetch policy, live fetch integration
│   ├── ocsp/                      # NEW: OCSP request/response codec + HTTP client
│   │   ├── mod.rs
│   │   ├── fetch.rs
│   │   └── codec.rs
│   └── tests/
│       └── ocsp.rs                # NEW: OCSP fetch tests
async-opcua-server/
├── src/
│   ├── config/server.rs           # + per-endpoint certificate_path, private_key_path
│   ├── server.rs                  # + endpoint→cert mapping at startup
│   ├── info.rs                    # + per-endpoint certificate storage
│   ├── session/manager.rs         # + cert selection per secure channel
│   └── subscriptions/
│       ├── actor.rs               # + new enum variants, - LegacyCall
│       └── mod.rs                 # - legacy() helper, + typed send methods

# New example crates (samples/)
samples/chat-server/               # cactuaroid/OpcUaChatServer model
├── Cargo.toml
├── README.md
└── src/
    └── main.rs
samples/chaos-server/              # Error-handling exercise
├── Cargo.toml
├── README.md
└── src/
    └── main.rs
samples/filesystem-bridge/         # Filesystem mirror
├── Cargo.toml
├── README.md
└── src/
    └── main.rs
samples/reverse-bridge/            # OPC UA mirror
├── Cargo.toml
├── README.md
└── src/
    └── main.rs
```

**Structure Decision**: Follow existing workspace conventions. New OCSP module goes in `async-opcua-crypto` (where `certificate_store.rs` lives). Per-endpoint cert config extends the existing `ServerConfig` struct. `SubscriptionCommand` enum lives in `actor.rs`. Example servers follow the pattern of `samples/persistent-store/`.

## Complexity Tracking

*No violations — this section intentionally empty.*
