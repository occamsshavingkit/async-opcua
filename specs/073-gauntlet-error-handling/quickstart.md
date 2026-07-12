# Quickstart: Gauntlet Error-Handling Fixes

**Feature**: 073-gauntlet-error-handling
**Date**: 2026-07-12

## Prerequisites

- Rust toolchain (stable)
- Working directory: `/home/quackdcs/async-opcua`
- Branch: `072-gauntlet-error-handling`
- The `node-management` feature must be enabled for NodeManagement tests:
  `cargo test -p async-opcua --features node-management`

## Verify Current State

```bash
# Confirm on correct branch
git branch --show-current
# Should output: 072-gauntlet-error-handling

# Run existing tests to establish baseline
cargo test -p async-opcua-server
cargo test -p async-opcua --test integration_tests
```

## Key Files to Modify

| Area | File | What Changes |
|------|------|-------------|
| NodeManagement validation | `async-opcua-server/src/session/services/node_management.rs` | Add input validation before NM dispatch |
| SetTriggering | `async-opcua-server/src/subscriptions/actor.rs` | Validate monitored item IDs |
| QueryFirst | `async-opcua-server/src/session/services/query.rs` | Return Good with empty results |
| HistoryUpdate | `async-opcua-server/src/session/services/attribute.rs` | Correct operation-level codes |

## Build & Test Commands

```bash
# Full workspace build
cargo build --workspace

# NodeManagement tests (requires feature flag)
cargo test -p async-opcua --features node-management --test integration_tests

# Specific test examples
cargo test -p async-opcua --test integration_tests -- integration::read::
cargo test -p async-opcua --test integration_tests -- integration::write::
cargo test -p async-opcua --test integration_tests -- integration::methods::

# Format and lint
cargo fmt --check
cargo clippy --workspace --all-features

# Full CI gate (before PR)
tools/ci-playbook.sh --ci
```
