# Technical Debt Report - 2026-07-07

## Executive Summary

This repository has strong engineering controls: pinned GitHub Actions, `cargo-deny`, broad feature-matrix builds, `clippy -D warnings`, coverage, footprint builds, codegen cleanliness checks, fuzz targets, interop tests, and a local CI playbook in `tools/ci-playbook.sh`.

The main debt is not missing process. It comes from protocol scale, generated-code volume, optional dependency risk, async/concurrency pressure points, and current companion-spec feature drift.

The most urgent finding is that companion specification feature names in `async-opcua-server/Cargo.toml` do not fully match the `#[cfg(feature = "...")]` names in `async-opcua-server/src/companion/mod.rs`. Because `src/companion/mod.rs` currently allows `unexpected_cfgs`, normal warning gates may not catch these mismatches.

## Evidence Snapshot

Metrics gathered from tracked repository files on 2026-07-07:

```yaml
rust_files: 1046
rust_loc_total: 587910
rust_loc_non_generated: 230610
cargo_manifests: 43
todo_fixme_hack_rust: 18
unsafe_usages_total: 27
blocking_lock_keyword_hits: 44
largest_generated_file: async-opcua-types/src/generated/node_ids.rs
largest_generated_file_loc: 51472
```

Top generated-code areas:

```yaml
async-opcua-core-namespace: 249149 LOC
async-opcua-types: 110495 LOC
```

Top non-generated Rust files by LOC:

```text
2762 async-opcua-client/src/session/services/subscriptions/service.rs
2750 async-opcua/tests/integration/subscriptions.rs
2689 async-opcua-server/src/node_manager/memory/mod.rs
2687 async-opcua-server/src/node_manager/memory/memory_mgr_impl.rs
2557 async-opcua-server/src/session/manager.rs
2541 async-opcua-server/tests/security_tests.rs
2445 async-opcua/tests/integration/alarms.rs
2365 async-opcua/tests/integration/read.rs
2219 async-opcua-core/src/comms/secure_channel.rs
2121 async-opcua-server/src/subscriptions/mod.rs
2038 async-opcua-server/src/subscriptions/subscription.rs
1952 async-opcua-crypto/src/ecc.rs
1923 async-opcua-types/src/variant/mod.rs
1900 async-opcua-server/src/node_manager/memory/simple.rs
1893 async-opcua-server/src/info.rs
1764 async-opcua-server/src/subscriptions/monitored_item.rs
1704 async-opcua-server/src/subscriptions/session_subscriptions.rs
```

## High-Priority Findings

### 1. Companion Feature Drift

Files:

- `async-opcua-server/Cargo.toml`
- `async-opcua-server/src/companion/mod.rs`

Features used in code but missing from Cargo feature declarations:

```text
companion-gms
companion-isa95
companion-isa95_jobcontrol
companion-pndrv
companion-pnenc
companion-pngsdgm
companion-pnrio
```

Features declared in Cargo but missing matching companion importer gates:

```text
companion-demomodel
companion-isa_95
companion-onboarding
```

Impact:

- Users enabling some documented companion features may not get the expected NodeSet import.
- Some companion imports may be impossible to activate through Cargo features.
- `#![allow(..., unexpected_cfgs)]` in `async-opcua-server/src/companion/mod.rs:8` can hide mismatch warnings.

Risk: High.

Recommended remediation:

- Make Cargo feature names and `companion!(...)` feature strings identical.
- Decide whether names with underscores or normalized names are canonical.
- Remove or narrow `unexpected_cfgs` suppression.
- Add a small validation script or test that checks every companion feature has a matching importer and every importer has a declared feature.

### 2. Spec Kit State Drift

Files:

- `specs/069-companion-specs/tasks.md`
- `specs/069-companion-specs/spec.md`
- `async-opcua-server/Cargo.toml`
- `async-opcua-server/src/companion/mod.rs`

Observation:

`specs/069-companion-specs/tasks.md` still marks companion work open, but implementation artifacts already exist. The current code appears to use runtime XML imports from `schemas/companion/`, while the plan says generated Rust types and node registrations should be produced per spec.

Impact:

- Future agents and reviewers may work from stale task state.
- Implementation strategy may diverge from the plan without explicit decision records.
- The repository can end up with mixed generated-code and runtime XML-import assumptions.

Risk: High.

Recommended remediation:

- Reconcile `tasks.md` with the current implementation.
- Update the companion-spec plan/spec if runtime XML import is the intended strategy.
- If generated Rust modules are still required, document that current `src/companion/mod.rs` is temporary or incomplete.

### 3. Generated-Code Volume

Files and areas:

- `async-opcua-types/src/generated/node_ids.rs`
- `async-opcua-core-namespace/src/generated/`
- `samples/custom-codegen/src/generated/`
- `code_gen_config.yml`

Observation:

Generated code accounts for a large share of repository size and build/review load. The current companion-spec feature would significantly increase this if every public companion spec is generated into Rust.

Impact:

- Slower builds and CI runs.
- Larger diffs that are hard to review.
- Higher risk of generated-code drift and feature-lattice breakage.
- Increased binary footprint unless feature boundaries remain strict.

Risk: Medium to High.

Recommended remediation:

- Decide explicitly between generated Rust, runtime XML imports, or a hybrid strategy for companion specs.
- Measure compile time and binary footprint before and after enabling `companion`.
- Keep generated companion artifacts out of default builds unless required.

### 4. Runtime Locking in Async Paths

Files:

- `async-opcua-core/src/lib.rs:242-247`
- `async-opcua-history-sqlite/src/backend.rs:21-24`
- `async-opcua-server/src/session/manager.rs:533-548`
- `async-opcua-server/src/session/manager.rs:637-662`

Observation:

The codebase is async, but shared server/session/history state still uses synchronous locks (`parking_lot::RwLock`, `parking_lot::Mutex`, and some `std::sync` usage in tests). This may be valid for short critical sections, but it is a latency and throughput risk if held across heavy work or close to awaits.

Impact:

- Potential executor stalls under high concurrency.
- Harder deadlock analysis.
- Increased risk as session/subscription/history code grows.

Risk: Medium.

Recommended remediation:

- Audit lock hold times around session manager, secure channel, subscriptions, and SQLite history backend.
- Confirm no synchronous lock guards are held across `.await` points.
- Add focused concurrency tests for high-contention session and history paths.

### 5. Unsafe Startup Mutations

Files:

- `async-opcua-server/src/info.rs:294-302`
- `async-opcua-server/src/server.rs:765-775`

Observation:

The server uses unsafe writes through `Arc::as_ptr` for startup-only mutation of shared data. Comments document the startup-only invariant.

Impact:

- The invariant is not compiler-enforced.
- Future lifecycle changes could accidentally make the write concurrent with readers.

Risk: Medium.

Recommended remediation:

- Replace with `OnceLock`, `ArcSwapOption`, or explicit startup-owned mutable state if possible.
- If retained, add stronger tests or debug assertions around startup order.

## Medium-Priority Findings

### Dependency Duplication and Accepted Advisory Debt

Files:

- `Cargo.toml`
- `Cargo.lock`
- `deny.toml`

Observation:

`deny.toml` documents several accepted advisories and optional/sample path exceptions. `cargo tree -d --locked` shows duplicate stacks, including multiple versions of `tokio-tungstenite`, `thiserror`, `rand`, `getrandom`, `rustix`, `async-lock`, and `async-io`.

Impact:

- Larger dependency tree and build graph.
- Higher audit burden.
- Optional transports and samples can drag stale transitive dependencies into all-feature CI.

Risk: Medium.

Recommended remediation:

- Periodically review `deny.toml` exceptions.
- Track upstream fixes for `rumqttc`, `log4rs`, and related transitive advisories.
- Try to collapse duplicated major versions where compatible.

### Documentation Drift in Crypto Description

File:

- `docs/compatibility.md:136-139`

Observation:

The documentation says crypto uses OpenSSL. Current dependencies and comments indicate `aws-lc-rs`, `rustls`, and Rust crypto crates are used in important paths.

Impact:

- Misleads users and security reviewers.
- Can cause incorrect setup expectations.

Risk: Low to Medium.

Recommended remediation:

- Update the crypto section to describe the current backend split accurately.

### Known Incomplete Protocol Areas

Files:

- `async-opcua-server/src/session/services/query.rs:131-134`
- `async-opcua-types/src/variant/mod.rs:1646`
- `async-opcua-nodes/src/events/evaluate.rs:220`

Observation:

There are explicit TODOs for query partial success behavior, `Variant::set_range_of` completeness, and event filter `RelatedTo` support.

Impact:

- Protocol edge cases may remain unsupported or return incomplete results.
- These areas likely need OPC UA specification grounding before changes.

Risk: Medium.

Recommended remediation:

- Turn each TODO into a tracked issue or Spec Kit task with OPC UA section references.
- Prioritize based on conformance-test failures or user-facing impact.

## Quick Wins

1. Fix companion feature-name drift.
2. Remove or narrow `unexpected_cfgs` allowance in `async-opcua-server/src/companion/mod.rs`.
3. Reconcile `specs/069-companion-specs/tasks.md` with actual implementation state.
4. Update stale crypto documentation in `docs/compatibility.md`.
5. Add a companion feature/importer consistency check.

## Roadmap

### Sprint 1

- Fix companion feature mismatches.
- Add a check that compares declared companion features with `companion!(...)` importer gates.
- Update stale companion Spec Kit artifacts or explicitly document the current divergence.
- Update crypto compatibility documentation.

### Month 1

- Audit synchronous lock use in async runtime paths.
- Confirm no lock guard is held across `.await` in session, subscription, secure-channel, and history paths.
- Add targeted high-contention tests for session lookup and SQLite history reads.

### Quarter 1

- Review `deny.toml` advisory exceptions and optional dependency stacks.
- Attempt dependency deduplication for duplicate major versions where upstream allows it.
- Decide and document the companion-spec import strategy: generated Rust, runtime XML, or hybrid.

### Quarter 2+

- Split the largest handwritten modules only where there are stable domain boundaries.
- Convert high-impact protocol TODOs into grounded implementation tasks with OPC UA references.
- Track build time, binary size, and feature-lattice impact as debt KPIs.

## Prevention Plan

- Keep `tools/ci-playbook.sh --ci` as the pre-PR gate.
- Add companion feature consistency validation to CI before the companion feature ships.
- Avoid broad `allow(unexpected_cfgs)` in code that depends on Cargo feature names.
- Require OPC UA section references for protocol TODOs that become implementation tasks.
- Review `deny.toml` exceptions on a scheduled cadence.
- Track generated-code size and release-footprint changes when adding new NodeSets.
