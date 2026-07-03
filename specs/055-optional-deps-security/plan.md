# Implementation Plan: Optional Dependencies and Security Hardening

**Feature Branch**: `055-optional-deps-security`  
**Created**: 2026-07-03  
**Spec**: [spec.md](./spec.md)

## Technical Context

- **Language**: Rust 2021 edition
- **Crates affected**: `async-opcua` (umbrella), `async-opcua-server`, `async-opcua-crypto`, `async-opcua-core`
- **New deps**: None (RSA-DH is already available in the crypto crate's algorithm set)
- **Crypto backend**: `aws-lc-rs` (default), `ring` (legacy). RSA-DH key transport uses existing RSA key-pair infrastructure.
- **Profile impact**: `nano`/`micro`/`embedded`/`standard` aliases gain no new features; `pubsub`/`history-sqlite` removed from their dependency tree.
- **Risk**: FR-005–FR-007 add a new crypto code path that touches network-input decryption — must pass security review per Principle IV.

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Correctness Over Completion | PASS | Each US is independently verifiable with its own test |
| II. Do It Right Once | PASS | RSA-DH uses existing crypto infrastructure; no copy-paste |
| III. Individual Task Discipline | PASS | Tasks below are one-per-change, independently verifiable |
| IV. Security Is Paramount | PASS | RSA-DH path fails closed (bad decrypt → reject token); security registry is bounded; no secrets logged |
| V. Leave It Better | PASS | Removing unused deps improves build health; security registry improves auditability |

## Phases

### Phase 0: Research

See [research.md](./research.md).

Key decisions:
- **RSA-KEM algorithm**: Part 6 §6.7.3 defines RSA-KEM (key encapsulation). The server uses its RSA private key to decrypt the client's symmetric wrapping key, then uses AES to unwrap the identity token. Already supported by the crypto backend.
- **Feature flag design**: `pubsub` and `history-sqlite` are Boolean features on `async-opcua` umbrella crate, default ON. Profile aliases explicitly disable them. Follows the same pattern as the 15 subsystem gates from feature 054.
- **Security check registry**: Bounded `VecDeque` behind an `RwLock` on `ServerInfo`, exposed through `ServerHandle`. Not persisted. Maximum entry count is a `ServerConfig` field defaulting to 1000 (same order as diagnostics summary).

### Phase 1: Design

See [data-model.md](./data-model.md) and [contracts/feature-flags.md](./contracts/feature-flags.md).

## Complexity Tracking

None — all three user stories are additive changes to existing infrastructure.
