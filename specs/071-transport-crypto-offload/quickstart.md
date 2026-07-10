# Quickstart: Transport Asymmetric Crypto Offload

Developer workflow for implementing and verifying feature 071. Assumes the branch
`071-transport-crypto-offload` (already created, off post-fix `master`).

## Build

```bash
# Fast inner loop — the crates this feature touches
cargo build -p async-opcua-core -p async-opcua-server -p async-opcua-client
```

## Verify — behavioral & structural (the CI gate for this feature)

```bash
# Unit: extracted crypto cores round-trip + existing secure-channel crypto vectors
cargo test -p async-opcua-core --lib comms::secure_channel

# Structural proof: OSC handshake completes on a busy current_thread runtime
#   (can only pass if crypto runs on the blocking pool)
cargo test -p async-opcua-server --test <offload_test> current_thread_handshake

# All-policies handshake correctness (RSA + ECC, Sign + SignAndEncrypt)
cargo test -p async-opcua-server --test <offload_test> all_policies

# Equivalence / no-wire-change guard — MUST stay green unchanged
cargo test -p async-opcua --test integration_tests \
  --features all,json,xml,legacy-crypto,wss,pubsub,history conformance::
```

## Verify — status-code fidelity & symmetric path untouched

```bash
# Fault contract: malformed/untrusted handshake returns the same StatusCode as before
cargo test -p async-opcua-server --test security_tests
# Hot-path unchanged: symmetric Read/Write incurs no new off-thread dispatch
cargo test -p async-opcua-server --test 'hot_path_*'
```

## Perf — the DoS/fairness property (manual, #[ignore]'d)

```bash
# 50-client handshake storm; assert an established session's latency stays bounded.
# Pin a core to remove scheduler noise (repo convention).
taskset -c 11 cargo test -p async-opcua-server --release --test <storm_test> \
  -- --ignored --nocapture handshake_storm_established_session_latency
```

## Full pre-PR gate (mandatory, per AGENTS.md)

```bash
tools/ci-playbook.sh --ci
```

## Success signals (map to spec Success Criteria)

- **SC-001** — storm test: established-session latency bounded, does not scale with concurrent
  handshakes.
- **SC-002 / SC-004** — `conformance::` matrix green and byte-identical wire output.
- **SC-003** — `current_thread` handshake test passes (crypto off the request path).
- **SC-005** — full `ci-playbook --ci` green; handshake success across the whole policy matrix.

> Replace `<offload_test>` / `<storm_test>` with the concrete test-binary names chosen during
> `/speckit-tasks` (kept out of this quickstart so it doesn't drift from the task list).
