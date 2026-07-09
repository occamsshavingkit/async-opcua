# Implementation Plan: Async Lock Audit Remediation

**Branch**: `070-async-lock-fixes` | **Date**: 2026-07-07 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `specs/070-async-lock-fixes/spec.md` and consolidated audit report from `specs/069-companion-specs/audit/report.md`

## Summary

35 audit findings (7 P0, 10 P1, 18 P2) covering ~593 lock acquisition sites across the async-opcua workspace. Fixes are organized into 9 user stories spanning 5 crates. The primary risk areas are: synchronous crypto blocking the async runtime (P0), single-connection SQLite serialization (P1), session manager TOCTOU and O(n) scans (P1), and lock scope cleanup (P2).

## Technical Context

**Language/Version**: Rust (edition 2021, workspace resolver = "2")
**Primary Dependencies**: parking_lot, dashmap, arc-swap, tokio, rusqlite, r2d2-sqlite
**Testing**: `cargo test --locked --all-features` (618+ tests), `cargo clippy --workspace --all-targets --all-features`, `tools/ci-playbook.sh --ci`
**Target Platform**: Linux server (tokio multi-threaded runtime)
**Project Type**: library (workspace of 18+ crates)
**Constraints**: All tests must pass; no public API changes; lock safety preserved; backward compatible

## Project Structure

```text
async-opcua-core/src/
├── comms/secure_channel.rs         # US1: crypto spawn_blocking, US8: std→parking_lot Mutex

async-opcua-server/src/
├── session/manager.rs              # US5: TOCTOU fix, O(n)→O(1) scans, US1: crypto offloading
├── subscriptions/mod.rs            # US2: waker fix, US7: write lock scope fix, US8: scope cleanup
├── subscriptions/actor.rs          # US2: waker fix
├── subscriptions/notify.rs         # US2: waker fix
├── address_space/mod.rs            # US8: browse_name_index TOCTOU, write lock scope
├── programs/engine.rs              # US8: batch write lock acquisitions

async-opcua-client/src/
├── transport/channel.rs            # US3: deadlock fix, US6: stale sender reload
├── transport/state.rs              # US6: client_offset RMW fix
├── session/mod.rs                  # US8: should_reconnect ordering
└── session/services/subscriptions/service.rs  # US6: Drop-lock fix, US9: actor migration

async-opcua-history-sqlite/src/
└── backend.rs                      # US4: connection pool, DashMap continuation points
```

## Verification Strategy

| Phase | Command | Expected |
|-------|---------|----------|
| Unit tests | `cargo test --locked --all-features` | All pass |
| Lint | `cargo clippy --workspace --all-targets --all-features` | No new warnings |
| CI playbook | `tools/ci-playbook.sh --ci` | All steps pass |
