# Quickstart: OPC UA 2017 Profile Minimal Builds (054)

## Build a profile-minimal server (consumer view)

```toml
[dependencies.async-opcua]
default-features = false
features = ["micro"]    # nano / micro / embedded / standard
```

## Build + measure the four profile benchmarks

ONE PACKAGE PER INVOCATION — combined builds unify features and poison the numbers
(research.md R10).

```bash
for p in nano micro embedded standard; do
  pkg="async-opcua-foundation-profile-${p}-server"
  cargo build --locked --profile embedded -p "$pkg"
  stat -c '%n %s bytes' "target/embedded/$pkg"
done
```

Pre-feature baselines (2026-07-02, rustc 1.96.0, x86-64): nano/micro 7,636,6xx B;
embedded 9,906,256 B; simple-server (full nodeset) 15,862,224 B. SC-001: post-gating nano
must land strictly below 7,636,648 B and the ladder must be strictly monotonic.

## Verify absence (what CI's leak guards do)

```bash
# dependency/feature guard
cargo tree --locked -p async-opcua-foundation-profile-nano-server -e features \
  | grep -E 'subscriptions|core-namespace' && echo LEAK || echo clean
# symbol spot-check
nm -C target/embedded/async-opcua-foundation-profile-nano-server 2>/dev/null \
  | grep -ci 'SubscriptionCache' # expect 0
```

## Verify behavior (profile smoke)

```bash
cargo test -p async-opcua-foundation-profile-nano-server --features profile-tests
```

Each sample serves its profile's mandated ops; excluded services must fault cleanly, e.g.
CreateSubscription against the nano sample → `Bad_ServiceUnsupported`, deadband filter
against micro → `Bad_MonitoredItemFilterUnsupported`. Smoke tests live with the samples
and run against the in-tree client. They are gated behind the sample's non-default
`profile-tests` feature: a multi-package run (`cargo test --workspace`) unifies the full
feature set into the test build and inverts rejection semantics — ONLY the isolated
invocation above is valid.

## Feature-lattice compile checks (FR-006)

```bash
# each alias standalone
for a in nano micro embedded standard; do
  RUSTFLAGS="-D warnings" cargo check -p async-opcua --no-default-features --features "$a"
done
# each gate individually removed from the full server surface (script enumerates)
# + the existing no-default-features and foundation-profile pre-push legs
```

## Full-build regression (FR-005)

`cargo test --workspace` with default features — must pass unchanged, plus the standard
pre-push gate (fmt, clippy all-targets all-features, no-default checks, cargo deny).
