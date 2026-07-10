# Quickstart: Hot-Path Per-Request Throughput

Developer workflow for feature 072 on branch `072-hot-path-throughput` (off `master`). The golden rule:
**measure before, measure after, keep only what the numbers justify.**

## Task 0 — capture the HEAD baseline FIRST (blocking)

Per `~/scratch/OPCUA-BENCHMARK.md` (CPU isolation: server on an isolated pinned core, clients elsewhere).

```bash
# US1 — single-core, single-client throughput (median of >=3)
taskset -c 11 cargo run --release -p async-opcua-localhost-bench -- run --op read  --clients 1
taskset -c 11 cargo run --release -p async-opcua-localhost-bench -- run --op write --clients 1

# US1 — where the cycles go (single-thread server)
perf record -e cycles:u -F 997 --call-graph dwarf,16384 -o perf-read.data -- \
  taskset -c 10 <server>   # then: perf report -i perf-read.data --stdio

# US2 — multi-core sweep + the MISSING contention profile (multi-thread server)
for c in 1 2 4 8 16 24 32; do
  cargo run --release -p async-opcua-localhost-bench -- run --op read --clients "$c"
done
perf c2c record -o c2c.data -- <multi-thread server under load>   # perf c2c report -i c2c.data
# off-CPU / wakeup latency: perf sched record / an off-CPU flamegraph
```

Record every number in `research.md` (the "HEAD performance baseline" entity). **No code change ships
before this exists.**

## Per-change loop (each S1x / S2 / US2 item, in order)

```bash
# 1. build the crates this feature touches
cargo build -p async-opcua-server -p async-opcua-core

# 2. correctness gate (must stay green + byte-identical)
cargo test -p async-opcua-server
cargo test -p async-opcua --test integration_tests \
  --features all,json,xml,legacy-crypto,wss,pubsub,history conformance::

# 3. measure vs baseline (same pinned core, same command as Task 0)
taskset -c 11 cargo run --release -p async-opcua-localhost-bench -- run --op read --clients 1

# 4. commit iff it clears its gate (C1); revert iff it regresses.
```

## Stage-2 fast-path specific tests

```bash
# panic isolation: a node manager that panics on read must yield BadInternalError,
# and a follow-up request on the same connection must still succeed.
cargo test -p async-opcua-server fast_path_read_panic_is_isolated
# equivalence: all attributes x all policies read the same as the actor path.
cargo test -p async-opcua-server fast_path_read_matches_actor_path
```

## Full pre-PR gate (mandatory, per AGENTS.md)

```bash
tools/ci-playbook.sh --ci
```

## Success signals → Success Criteria

- **SC-001/002** — single-client read ≥1.5× baseline; write measurably up.
- **SC-003** — sweep: per-core efficiency degrades <11% and/or plateau moves up, with a `c2c` HITM drop.
- **SC-004/005** — conformance byte-identical; full suite green.
- **SC-006** — every commit carries its before/after numbers.

> Concrete `<server>` invocation and the exact pinned cores are environment-specific; align them with
> `~/scratch/OPCUA-BENCHMARK.md` before Task 0.
