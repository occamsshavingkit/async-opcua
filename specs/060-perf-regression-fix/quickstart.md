# Quickstart: Performance Regression Fix — Localhost Benchmark

## Prerequisites

- Rust 1.75+ with `cargo`
- Linux with `perf` (packaged as `linux-perf` or `linux-tools-common` on Ubuntu)
- Working `async-opcua` workspace checkout on branch `060-perf-regression-fix`

## Build and Test

```bash
# Build the full workspace
cargo build

# Build the benchmark in release mode (required for meaningful profiling)
cargo build --release --bin async-opcua-localhost-bench

# Run all tests (pre-PR gate)
tools/ci-playbook.sh --ci

# Run the benchmark standalone
./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
```

## US1: Profiling

```bash
# Build release for both baseline and HEAD (need to git checkout each)
cargo build --release --bin async-opcua-localhost-bench

# Profile with perf stat
perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses \
  ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5

# Also profile write path
perf stat -e instructions,cycles,cache-misses,branch-misses \
  ./target/release/async-opcua-localhost-bench run --op write --warmup 3 --measure 5
```

Compare counters between baseline (commit `983b222a7`) and HEAD (commit `765434c3b`). Document results in `docs/perf-analysis.md`.

## US2: VIEW-03 Revert

The revert replaces `strip_result_mask_fields()` method calls in `async-opcua-server/src/node_manager/view.rs` with inline field-clearing code. Verify with:

```bash
cargo test -p async-opcua-server -- node_manager
cargo test --locked --all-features
./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
```

## US3: #[inline] Annotations

Add `#[inline]` to hot-path functions in `async-opcua-server/src/session/controller.rs`. Verify with:

```bash
cargo build --release --bin async-opcua-localhost-bench
./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
```

## US4: Release Profile Tuning

Add to workspace `Cargo.toml`:
```toml
[profile.release]
codegen-units = 1
lto = true
```

Rebuild and test:
```bash
cargo build --release --bin async-opcua-localhost-bench
./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
cargo test --locked --all-features
```

## Development Order

1. **US1 (Profiling)** — profile baseline vs HEAD, confirm mechanism
2. **US2 (VIEW-03 Revert)** — apply first targeted fix, re-benchmark
3. **US3 (#[inline])** — apply second targeted fix, re-benchmark
4. **US4 (Release Profile)** — apply profile tuning, final benchmark
5. Run full CI playbook: `tools/ci-playbook.sh --ci`
