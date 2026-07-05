# Implementation Plan: Hot Path Audit Fixes

**Branch**: `061-hot-path-audit-fixes` | **Date**: 2026-07-05 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/061-hot-path-audit-fixes/spec.md`

## Summary

Apply the "Hot Paths" skill patterns to the async-opcua Rust library. Fix 5 performance issues found in a three-path audit (startup, per-request, per-message):
1. `DecodingOptions` struct cloned on every encode/decode → share via `Arc`
2. Type tree rebuilt N times during startup → build once after all managers
3. `RequestContextInner` allocated per Read/Write → cache on `SessionActor`
4. `SecurityPolicy::from_uri()` called per chunk → cache on `SecureChannel`
5. Certificate loading sequential I/O → parallel `tokio::join!`

## Technical Context

**Language/Version**: Rust 1.75+ (workspace, edition 2021)
**Primary Dependencies**: tokio, arc-swap, parking_lot, dashmap
**Affected Crates**: async-opcua-types, async-opcua-core, async-opcua-server, async-opcua-crypto
**Testing**: `cargo test --locked --all-features`
**Target Platform**: Linux, Windows, macOS
**Performance Goals**: Eliminate per-message heap allocations; reduce startup type-tree work 30-50% with multiple managers
**Constraints**: No OPC UA protocol behavior change; no wire format change; no service semantics change

## Constitution Check

| Principle | Assessment | Notes |
|-----------|-----------|-------|
| I. Correctness Over Completion | PASS | Each US has independent test criteria; Arc-share, cache, and parallelization are well-understood patterns |
| II. Do It Right Once | PASS | `Arc<DecodingOptions>` replaces clone at the type level — correct by construction. Type-tree build-once prevents rebuilds permanently |
| III. Individual Task Discipline | PASS | Five independent USs across different crates and files |
| IV. Security Is Paramount | PASS | No cryptographic or authentication code changes; `SecurityPolicy` caching preserves validation semantics |
| V. Leave It Better Than You Found It | PASS | Eliminating redundant allocations and rebuilds improves efficiency for all workloads |

**Gate: ALL PASS**

## Project Structure

```text
specs/061-hot-path-audit-fixes/
├── spec.md        # Feature specification
├── plan.md        # This file
├── research.md    # Phase 0 output
├── data-model.md  # Phase 1 output
├── quickstart.md  # Phase 1 output
├── contracts/     # Phase 1 output
└── tasks.md       # Phase 2 output
```

### Source Code

```text
# US1 — DecodingOptions Arc
async-opcua-types/src/encoding.rs              # Add Arc wrapper or refactor DecodingOptions
async-opcua-types/src/type_loader/mod.rs       # Change context() to Arc::clone

# US2 — Type Tree Build Once
async-opcua-server/src/node_manager/memory/mod.rs   # Remove load_into_type_tree/ensure_browse_name_index/publish from init()
async-opcua-server/src/node_manager/memory/core.rs  # Remove equivalent rebuild logic
async-opcua-server/src/server.rs                    # Move rebuilds to after the init loop

# US3 — RequestContext Caching
async-opcua-server/src/session/actor.rs        # Cache Arc<RequestContextInner>, invalidate on token change

# US4 — SecurityPolicy Caching
async-opcua-core/src/comms/secure_channel.rs   # Store validated SecurityPolicy, replace match with flag
async-opcua-core/src/comms/security_header.rs  # Use cached policy instead of from_uri()

# US5 — Parallel Certificate Loading
async-opcua-crypto/src/certificate_store.rs    # Add async read_cert/read_pkey
async-opcua-server/src/server.rs               # tokio::join! cert+key reads per endpoint
```

## Complexity Tracking

*No violations.*
