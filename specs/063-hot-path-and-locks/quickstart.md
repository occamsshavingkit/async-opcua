# Quickstart: Hot-Path and Lock Optimization

## Prerequisites

- Rust toolchain (stable)
- Linux with `perf` (for profiling baseline)
- Git checkout on branch `063-hot-path-and-locks`

## Build

```bash
# Debug build (fast compile, for iteration)
cargo build

# Release build (for benchmarking)
cargo build --release --bin async-opcua-localhost-bench
```

## Run Baseline Benchmark

Record before/after throughput for each optimization:

```bash
# Build release
cargo build --release --bin async-opcua-localhost-bench

# Baseline — read benchmark (3 runs, 5s each)
for i in 1 2 3; do
  echo "=== Run $i ==="
  ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
done

# Baseline — write benchmark (3 runs, 5s each)
for i in 1 2 3; do
  echo "=== Run $i ==="
  ./target/release/async-opcua-localhost-bench run --op write --warmup 3 --measure 5
done
```

## Profile Baseline (CPU Hotspots)

```bash
# Profile the read benchmark
perf record -g --call-graph dwarf \
  ./target/release/async-opcua-localhost-bench run --op read --warmup 2 --measure 3

perf report --no-children --sort symbol -F 'overhead,symbol' | head -30
```

Key symbols to watch for reduction:
- `parking_lot::raw_rwlock::RawRwLock::lock_shared` — US1 (AddressSpace lock)
- `SessionManager::find_by_token` — US2 (session cache)
- `tokio::time::driver::TimerEntry::reset` / `drop` — US3 (deadline queue)
- `arc_swap::Debt::pay_all` — US4 (ArcSwap investigation)

## Run Test Suite

```bash
# Full test suite (must pass after each change)
cargo test --locked --all-features

# Clippy (must be clean)
cargo clippy --workspace --all-targets --all-features -- -Dwarnings

# Interop tests
tools/ci-playbook.sh --ci
```

## Implementation Order

1. **US1 — AddressSpace split**: Largest structural change. Run tests after each sub-step.
2. **US2 — Session cache**: Add caching field, update dispatch sites.
3. **US3 — Deadline queue**: Replace `sleep_until` with `BTreeMap`-backed queue.
4. **US4 — ArcSwap investigation**: Profile, identify, fix/replace/document.

Between each user story, run the benchmark and profile to verify the targeted overhead is reduced.

## Rollback

Each change is independent. If a change causes a regression:
1. Revert that change's commit
2. Re-run the benchmark to confirm baseline is restored
3. Investigate and re-apply with fix

## Verify Success

```bash
# After all changes, compare vs baseline
cargo build --release --bin async-opcua-localhost-bench

# Aggregate throughput should improve ≥3%
for i in 1 2 3; do
  ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
done

# CI check: all tests + interop must pass
cargo test --locked --all-features
tools/ci-playbook.sh --ci
```
