# Research: Backlog Closeout Batch

## US1 — OCSP Responder Infrastructure

### Decision: Build a minimal OCSP responder using the existing x509-ocsp crate + manual signing

**Rationale**: The `async-opcua-crypto/src/ocsp/` module already has a full RFC 6960 codec (request encoding, response decoding) from feature 057. The responder side needs:
1. An OCSP response builder that constructs `BasicOcspResponse` DER from certificate status data
2. A signing step that uses the CA's private key to sign the response
3. A simple certificate status database (serial number → status mapping)

The `x509-ocsp` crate provides `OcspResponseBuilder` and `BasicOcspResponse` types. The signing can use the existing `PrivateKey` / `X509` infrastructure in `async-opcua-crypto`. This avoids an external OCSP library dependency.

**Alternatives considered**:
- `ocsp-responder` crate: No widely-used Rust crate exists for OCSP responder logic. The `x509-ocsp` builder API is sufficient.
- Full HTTP server integration: Out of scope. The responder module exposes a function that takes a DER-encoded OCSP request and returns a signed response; the transport layer (HTTP) is left to the caller or a future follow-up. This matches the "infrastructure" scope — the building blocks, not a production deployment.

### Decision: Certificate status database is an in-memory HashMap

**Rationale**: The OCSP responder is infrastructure, not a production service. An in-memory `HashMap<SerialNumber, CertStatusRecord>` is sufficient. The caller populates it. No persistence, no CRL parsing needed at this stage. The extensibility point (a trait or function pointer) allows users to plug in their own status source.

**Alternatives considered**:
- CRL-based responder: Would require CRL parsing and is more complex than the scope calls for. The in-memory approach covers the basic "this cert is good/revoked" use case.

### Decision: Support nonce extension echo per RFC 6960

**Rationale**: RFC 6960 §4.4.1 and §4.4.2 require that if a request includes a nonce extension, the response MUST echo it. This is required for replay protection and is checked by standard OCSP clients. The `x509-ocsp` crate's `OcspResponseBuilder` already supports extensions, so this is straightforward.

**Alternatives considered**: Skipping nonce support would break interop with standard OCSP clients like `openssl ocsp`. Must implement.

---

## US2 — SDK Node-Manager Tooling

### Decision: Add a `NodeManagerBuilder` helper that wraps `InMemoryNodeManager` with a fluent API

**Rationale**: The current path for creating a custom node manager is:
1. Implement `NodeManagerBuilder` trait → `build()` returns `Arc<DynNodeManager>`
2. Implement `NodeManagerCore` (owns_node, name, namespaces_for_user, init)
3. Optionally implement `ViewProvider`, `AttributeProvider`, `MonitoredItemProvider`, `MethodProvider`, `HistoryProvider`, `NodeMutator`

For the common case of "add a few Variables with read callbacks to a namespace," steps 2-3 are boilerplate. The `InMemoryNodeManager` already provides a full implementation of all capability traits — the issue is that embedding it requires manual delegation or a builder that registers callbacks on the `InMemoryNodeManager`'s address space.

**Proposed API**:
```rust
let nm = QuickNodeManager::new("my-namespace")
    .variable("MyVar", 42u32)
        .read_callback(|ctx, node, attr| { /* ... */ })
        .writable()
        .add()
    .build(context);
```

**Alternatives considered**:
- Macro-based DSL: Too magical, harder to debug, conflicts with Rust's explicitness philosophy.
- Trait decomposition further refinement: Already done in feature 009; not the bottleneck.
- Derive macro approach: Overengineered for the scope. A builder pattern is simpler and follows existing project patterns (`ServerBuilder`, `ClientBuilder`).

### Decision: Keep existing `NodeManagerBuilder` trait and add `QuickNodeManager` alongside it

**Rationale**: The existing `NodeManagerBuilder` trait is the low-level extension point. `QuickNodeManager` is a concrete implementation of it that uses `InMemoryNodeManager` internally. This preserves backward compatibility (FR-009) and provides a clean upgrade path.

**Alternatives considered**: Modifying `NodeManagerBuilder` to be easier to implement directly (e.g., default method impls). Would be a breaking change or require more involved refactoring. The builder approach is additive.

---

## US3 — RSA-KEM Integration Test

### Decision: Add test to `async-opcua/tests/integration/rsa_kem.rs` using the existing `Tester` harness

**Rationale**: The integration test suite in `async-opcua/tests/integration/` has a well-established `Tester` harness (`tests/utils/tester.rs`) that creates a server with auto-generated RSA certificates, connects a client, and provides a session. The RSA-KEM test needs:
1. Server with RSA certificate (already done by `create_sample_keypair(true)`)
2. Client that connects with a UserName token encrypted via RSA-KEM
3. Verification that the session activates

The `Tester` harness already supports UserName tokens (`client_user_token()` helper). The gap is verifying that the encrypted secret path (RSA-KEM specifically, not RSA-OAEP) is exercised. The server negotiates the algorithm based on the client's declared capabilities; the test must ensure the client selects RSA-KEM.

**Alternatives considered**:
- Standalone test binary: Unnecessary; integration tests already have the full pipeline.
- Unit test only: Unit tests in `rsa_kem.rs` cover the decryption function but not the client↔server wire path. Only an integration test catches issues like incorrect algorithm negotiation or serialization mismatch.

### Decision: Test both success and failure paths

**Rationale**: FR-015 requires both success (valid token accepted) and failure (corrupted ciphertext rejected). The success path verifies the happy round-trip. The failure path verifies that `BadIdentityTokenRejected` is returned, not a panic or incorrect status.

**Alternatives considered**: Success-only test. Insufficient — the security-critical path (rejecting bad tokens) must be proven.

---

## US4 — Embedded Profile Secure Channel Smoke Test

### Decision: Add a `connect_secure_two_phase` helper to the embedded test harness

**Rationale**: The current `connect_secure` helper uses `connect_to_matching_endpoint` which does a single-phase connect. For Sign&Encrypt with an unknown server cert, the client needs a two-phase connect:
1. Connect with policy None + `connect_to_matching_endpoint` to get the server's endpoints and certificate
2. Extract the server certificate from the endpoint descriptions
3. Reconnect with Sign&Encrypt using that certificate

The `Tester` harness in the integration suite already does this implicitly. The profile test harness needs a similar two-phase helper.

**Alternatives considered**:
- Fix `connect_to_matching_endpoint` to support two-phase: Would be a broader client change affecting many call sites. Riskier. A helper function scoped to the test harness is safer.
- Keep test ignored: Violates the feature goal. Must un-ignore.

### Decision: Use `GetEndpoints` service call for phase 1 then `connect_to_matching_endpoint` for phase 2

**Rationale**: The `client.get_endpoints()` call returns endpoint descriptions including the server certificate. Phase 1 uses this with policy None to discover the cert. Phase 2 uses the standard `connect_to_matching_endpoint` with the discovered cert.

**Alternatives considered**: Raw socket + handshake. Too low-level and fragile.

---

## US5 — Standard Profile X509/RegisterServer2 Tests

### Decision: X509 test uses the two-phase connect pattern from US4 + X509 identity token

**Rationale**: Same two-phase pattern as US4, plus X509 token provisioned via `IdentityToken::new_x509_path()`. The test utility already has `USER_X509_CERTIFICATE_PATH` and `USER_X509_PRIVATE_KEY_PATH` constants pointing to test certs in `tests/x509/`.

**Alternatives considered**: Auto-generating X509 user certs at test time. More complex and unnecessary — existing test fixtures suffice.

### Decision: RegisterServer2 test spawns an in-process LDS peer server

**Rationale**: The standard server's `register_server2_flow` test needs an LDS (Local Discovery Server) to receive RegisterServer2 calls. The approach:
1. Spawn a minimal in-process server with the `discovery-mdns` feature enabled and a known LDS endpoint
2. Configure the standard server to register with that LDS
3. Verify over a polling interval that the registration was received

The LDS peer can be a second `Server` instance configured as a discovery server, started on an ephemeral port.

**Alternatives considered**:
- Mock LDS (fake RegisterServer2 handler): Wouldn't exercise the real discovery path. The point is to test that the standard server's periodic registration actually works.
- External LDS process: Unreliable in CI, adds setup complexity.
