# Quickstart: Complete the 27 Partial Conformance Units

How to work a task and how to verify the feature. Aimed at a small local coding model executing tasks.md one item at a time.

## Working a single CU task (the loop)

1. **Read the task** — it names one CU, the symbol to grep, the OPC UA Part/§ to read, the behavior to implement/assert, and the test file.
2. **Ground it**:
   - `grep -rn "<symbol>" async-opcua-server/src` (and related crates) to find the CURRENT code — ignore the audit's line numbers, they are stale.
   - Read the cited OPC UA Part/§ via the OPC-UA reference MCP (`search_terms`/`text`/`nodes`/`cu`). If the MCP is unavailable, use the local PDFs at `~/opcua-specs/` with `pdftotext -layout`.
3. **Implement** (Type-B only): make the minimal wiring/population/emission the task describes — nothing more. Do not refactor neighbouring code.
4. **Write the test**: assert the spec's observable behavior. For a Type-A CU, this is the whole task.
5. **Verify the one CU**:
   ```
   cargo test -p <crate> --features <needed> <test_name>
   ```
6. **Do not** touch other CUs' code or tests in the same task (Principle III — one task at a time).

## Feature-flag cheat-sheet (which features compile which CU)

| Subsystem | Feature(s) needed |
|---|---|
| Alarms & Conditions | `alarms` (pulls `events`, `method-call`) |
| History (in-memory) | `history` |
| History (sqlite) | the `async-opcua-history-sqlite` crate |
| Audit / RBAC | `rbac` |
| Subscriptions / MonitoredItems | `subscriptions`, `subscriptions-standard` |
| Method / Call | `method-call` |
| Event filter | `events` |

Most server tests run under `--all-features`. The `no-default-features` workspace build must still compile.

## Verifying the whole feature (end gate)

```bash
# All CU tests + full regression
cargo test -p async-opcua-server --all-features
cargo test -p async-opcua-history-sqlite            # sqlite history CUs
cargo test -p async-opcua-cu-coverage-report        # ledger self-tests

# Feature-gate sanity
cargo build -p async-opcua-server --no-default-features

# Lint + format (workspace)
cargo clippy --workspace --all-features --all-targets
cargo fmt --all -- --check

# Regenerate the coverage ledger and confirm the 27 flipped
cargo run -p async-opcua-cu-coverage-report --quiet -- \
  /home/quackdcs/micro-opcua/profiles/opcua-profile-normalized-snapshot.json \
  specs/conformance-tester/CU-COVERAGE.md
grep -E "\| (2275|2811|2814|2918|4466|2289|2950|2422|3224|3542|3968|3539|3540|3541|2318|2818|3142|5208|3544|2203|2454|3605|2476|3546|3194|3201|2823) \|" \
  specs/conformance-tester/CU-COVERAGE.md | grep -c "implemented"
# expect: 27
```

## Done criteria (per feature spec SC-001..SC-005)

- All 27 rows show `implemented` in the regenerated CU-COVERAGE.md.
- No CU regressed to a lower status; the 3 Extensible + 141 Gap CUs unchanged.
- Every CU is traceable to a named test in its evidence string.
- test / clippy / fmt all green on the full workspace.
