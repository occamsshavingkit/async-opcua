# Quickstart: Backlog Closeout Batch

## Prerequisites

- Rust 1.75+ with `cargo`
- Working `async-opcua` workspace checkout on branch `058-backlog-closeout-batch`

## Build and Test

```bash
# Build the full workspace
cargo build

# Run all tests (pre-PR gate)
tools/ci-playbook.sh --ci

# Run per-US test suites
cargo test -p async-opcua-crypto -- ocsp_responder     # US1
cargo test -p async-opcua-server -- node_manager          # US2
cargo test -p async-opcua --test integration -- rsa_kem  # US3
cargo test -p async-opcua-foundation-profile-embedded-server --features profile-tests  # US4
cargo test -p async-opcua-foundation-profile-standard-server --features profile-tests   # US5
```

## US1: OCSP Responder

The new `async_opcua_crypto::ocsp::responder::build_ocsp_response()` function takes:
- A DER-encoded OCSP request
- An `OcspResponderConfig` with signer cert/key, validity window, and status database

Example usage (conceptual):
```rust
use async_opcua_crypto::ocsp::responder::{build_ocsp_response, CertStatusVariant, OcspResponderConfig};

let config = OcspResponderConfig {
    signer_cert: ca_cert,
    signer_key: ca_private_key,
    response_validity: Duration::from_secs(3600),
    status_db: [(serial_der, CertStatusVariant::Good)].into(),
};
let response_der = build_ocsp_response(&request_der, &config)?;
```

## US2: Quick Node Manager Builder

The `QuickNodeManager` builder reduces boilerplate for simple node managers:

```rust
use async_opcua_server::node_manager::quick::QuickNodeManager;

let nm = QuickNodeManager::new("urn:my-namespace")
    .variable("Counter", 0u32)
        .writable()
        .add()
    .variable("Status", "OK")
        .read_callback(|ctx| Ok(DataValue::new_now("RUNNING".into())))
        .add();

server_builder.with_node_manager(nm);
```

## US3: RSA-KEM Integration Test

The test exercises the full path: client with RSA-KEM-encrypted UserName token → server with RSA cert.
Run with:
```bash
cargo test -p async-opcua --test integration -- rsa_kem
```

## US4-US5: Profile Tests

Run with the `profile-tests` feature:
```bash
cargo test -p async-opcua-foundation-profile-embedded-server --features profile-tests
cargo test -p async-opcua-foundation-profile-standard-server --features profile-tests
```

All previously `#[ignore]`d tests should now pass.

## Development Order

US1, US2, US3, US4, US5 are independent and can be developed in any order. Recommended:
1. US2 (SDK tooling) — no dependencies, purely additive
2. US1 (OCSP responder) — reuses existing codec, independent
3. US3, US4, US5 (tests) — each independent, can run in parallel after US4/US5 harness helpers
