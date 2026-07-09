# Quickstart: Async Lock Audit Remediation

**Feature**: 070-async-lock-fixes | **Date**: 2026-07-07

## Full Validation

```bash
cargo test --locked --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
./tools/ci-playbook.sh --ci
```

## Per-User Story Verification

- US1 (Crypto): `cargo test -p async-opcua --test integration secure_channel`
- US2 (Waker): `cargo test -p async-opcua --test integration subscriptions`
- US3 (Deadlock): `cargo test -p async-opcua-client --test secure_channel_renewal_singleflight`
- US4 (SQLite): `cargo test -p async-opcua-history-sqlite`
- US5 (Session mgr): `cargo test -p async-opcua --test integration sessions`
- US6 (Client): `cargo test -p async-opcua-client`
- US7-9: Covered by existing subscription and client tests
