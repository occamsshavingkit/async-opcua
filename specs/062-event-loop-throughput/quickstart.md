# Quickstart: Event Loop Throughput

## Prerequisites

- Rust toolchain (stable, no specific MSRV)
- Linux with `perf` (for profiling, optional)
- Git checkout on branch `062-event-loop-throughput`

## Build

```bash
# Debug build (fast compile, for iteration)
cargo build --bin async-opcua-localhost-bench

# Release build (for benchmarking)
cargo build --release --bin async-opcua-localhost-bench
```

## Run Benchmark (Baseline)

```bash
# Record baseline throughput before changes
cargo build --release --bin async-opcua-localhost-bench

# Read benchmark (3 runs)
for i in 1 2 3; do
  ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
done

# Write benchmark (3 runs)
for i in 1 2 3; do
  ./target/release/async-opcua-localhost-bench run --op write --warmup 3 --measure 5
done
```

## Profile with perf (Optional)

```bash
perf stat -e instructions,cycles,cache-misses,branch-misses \
  ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5
```

## Test

```bash
# Full test suite (gate before any code change)
cargo test --locked --all-features

# Targeted tests for transport/session
cargo test -p async-opcua-server -- session
cargo test -p async-opcua-core -- comms
```

## Lint & Format

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings
```

## CI Playbook

```bash
tools/ci-playbook.sh --ci
```

## Key Files to Modify

| File | Purpose |
|------|---------|
| `async-opcua-server/src/session/controller.rs` | US1: Move encoding into async task; US2: Batch-drain messages from transport |
| `async-opcua-server/src/transport/tcp.rs` | US1: Update trait/impl for batched poll result; US2: Drain loop in poll_inner |
| `async-opcua-core/src/comms/buffer.rs` | US1: Add `push_encoded_chunks()` to `SendBuffer` |

## Verification After Each Change

1. `cargo test --locked --all-features` — must pass
2. `cargo fmt --all -- --check` — no formatting issues
3. `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` — no warnings
4. `cargo build --release --bin async-opcua-localhost-bench` — release build succeeds
5. `./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5` — benchmark runs correctly
6. Compare throughput against baseline — no regression

## Performance Tuning

The workspace Cargo.toml already has:
```toml
[profile.release]
codegen-units = 1
lto = true
```

These give LLVM full visibility for inlining, which is important for the event loop hot path. No additional profile tuning needed for this feature.
