# Quickstart: Hot Path Audit Fixes

## Build and Test

```bash
cargo build
cargo test --locked --all-features
cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings
```

## Run Benchmark

```bash
cargo build --release --bin async-opcua-localhost-bench
taskset -c 11 ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
```

## Per-US Verification

```bash
# US1 — DecodingOptions Arc: all encoding tests must pass
cargo test -p async-opcua-types -- encoding

# US2 — Type Tree Build Once: server integration tests
cargo test -p async-opcua-server

# US3 — RequestContext Caching: session tests
cargo test -p async-opcua-server -- session

# US4 — SecurityPolicy Caching: secure channel tests
cargo test -p async-opcua-core -- secure_channel

# US5 — Parallel Certificate Loading: crypto tests
cargo test -p async-opcua-crypto -- certificate_store
```

## Implementation Order

US1 (CRITICAL, per-message), US2 (CRITICAL, startup), and US3 (HIGH, per-request) can all start in parallel (different crates/files). US4-US5 are MEDIUM priority and can follow.
