# Contract: Profiling Methodology

## Scope

Define a repeatable profiling methodology for measuring the localhost benchmark throughput with hardware performance counters, to be used for comparing baseline (pre-059) and HEAD (post-059) builds.

## Hardware Counter Events

| Event | What It Measures | Relevance to Regression |
|-------|-----------------|------------------------|
| `instructions` | Total instructions executed | Increase >5% indicates de-inlining (more instruction-retirement overhead per request) |
| `cycles` | Total CPU cycles | Baseline for IPC (instructions-per-cycle) comparison |
| `cache-misses` | L1/L2/L3 cache misses (all levels) | Increase >10% indicates i-cache or d-cache pressure from larger binary |
| `branch-misses` | Mispredicted branches | Increase >10% indicates `.text` layout disruption affecting branch predictor |
| `L1-icache-load-misses` | L1 instruction cache misses | Direct measure of i-cache pressure — the primary hypothesized mechanism |

## Test Command

```bash
perf stat -e instructions,cycles,cache-misses,branch-misses,L1-icache-load-misses \
  ./target/release/async-opcua-localhost-bench run --op <read|write> --warmup 3 --measure 5
```

## Protocol

1. Build release binary for commit under test: `cargo build --release --bin async-opcua-localhost-bench`
2. Run the `perf stat` command above for both `read` and `write` modes
3. Run each configuration **3 times** and record **median** values
4. Compute per-request metrics: divide instruction/cache-miss/branch-miss counts by the benchmark's `ok` count (total successful operations)
5. Report the relative change: `(HEAD_value - baseline_value) / baseline_value * 100%`

## Comparable Commits

| Label | Commit SHA | Description |
|-------|-----------|-------------|
| Baseline | `983b222a7` | Pre-059: merge of feature 058 (backlog closeout batch) |
| HEAD | `765434c3b` | Post-059: merge of feature 059 (spec compliance audit fixes) |

## Expected Output

A markdown table in `docs/perf-analysis.md`:

```markdown
| Metric | Baseline (983b222a7) | HEAD (765434c3b) | Delta |
|--------|----------------------|-------------------|-------|
| Throughput (req/s) | XXXXX | XXXXX | ±XX% |
| Instructions/req  | XXXXX | XXXXX | ±XX% |
| Cache-misses/req  | XXXXX | XXXXX | ±XX% |
| Branch-misses/req | XXXXX | XXXXX | ±XX% |
| L1i-misses/req    | XXXXX | XXXXX | ±XX% |
```

## CPU Model

Record the CPU model and cache topology for reproducibility:

```bash
lscpu | grep -E 'Model name|L1[di]|L2|L3'
```
