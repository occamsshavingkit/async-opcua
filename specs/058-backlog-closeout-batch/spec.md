# Feature Specification: Backlog Closeout Batch

**Feature Branch**: `058-backlog-closeout-batch`  
**Created**: 2026-07-04  
**Status**: Draft  
**Input**: User description: "close out the remaining backlogs: OCSP responder infrastructure, SDK node-manager tooling, RSA-KEM integration test, embedded profile smoke test, standard profile X509/RegisterServer2 tests"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - OCSP Responder Infrastructure (Priority: P1)

Server operators deploying in air-gapped or enterprise PKI environments need the ability to run
an OCSP responder alongside the OPC UA server, so that other servers on the network can perform
live certificate revocation checking without external infrastructure. The OCSP client (live fetch)
was completed in feature 057; this completes the responder side of the OCSP story.

**Why this priority**: The OCSP live-fetch client (feature 057) is only half the picture — the
completeness-backlog explicitly calls out "OCSP operational infrastructure" as the remaining scope.
A deployment that needs to serve as its own OCSP responder must be able to produce valid,
standards-compliant OCSP responses.

**Independent Test**: Start a minimal OCSP responder that serves OCSP responses for a local CA
certificate, send an OCSP request from an external tool (e.g., openssl ocsp), and verify the
response is a valid RFC 6960 OCSP response with correct signature and certificate status.

**Acceptance Scenarios**:

1. **Given** a local CA certificate and its private key, **When** an OCSP request arrives for a certificate issued by that CA, **Then** the responder returns a valid signed OCSP response with status "good", "revoked", or "unknown" matching the certificate's known state.
2. **Given** a responder with a revocation list, **When** an OCSP request arrives for a revoked certificate, **Then** the response correctly indicates "revoked" with the revocation time.
3. **Given** a responder receiving a malformed OCSP request, **When** the request cannot be parsed as a valid RFC 6960 OCSPRequest, **Then** the responder returns a properly formed OCSP error response rather than crashing or hanging.
4. **Given** a responder receiving a request for a certificate from an unknown issuer, **When** the issuer is not recognized by the responder, **Then** the response indicates "unknown" status.

---

### User Story 2 - SDK Node-Manager Tooling (Priority: P2)

Developers building custom OPC UA servers need an ergonomic SDK for creating and managing custom
node managers. The TODO.md calls for fleshing out server SDK tooling to make it easier to
implement custom node managers. Current documentation (`docs/advanced_server.md`) describes the
`NodeManager` trait with ~30 methods, and the `InMemoryNodeManager` provides a solid foundation,
but the ergonomics for custom implementations need improvement — common patterns should be
one-liners, not boilerplate.

**Why this priority**: Directly addresses the oldest remaining TODO.md item ("Flesh out the server
and client SDK with tooling for ease of use"), which has been deferred across multiple features.
Lower ceremony for SDK users is a prerequisite for broader adoption.

**Independent Test**: Write a custom node manager that adds a single Variable node with a read
callback, registers it with a server, and verifies a client can read the value. The implementation
should require no more than ~5 lines of setup code beyond the callback itself.

**Acceptance Scenarios**:

1. **Given** a developer wants to create a custom node manager with a single Variable, **When** they use the SDK tooling, **Then** they can create and register it with minimal boilerplate (no more than 5-10 lines of setup code for a simple case).
2. **Given** a developer uses the improved `InMemoryNodeManager` or a new builder API, **When** they add nodes with read callbacks and write callbacks, **Then** each callback type follows a consistent, documented pattern.
3. **Given** existing node-manager samples (`samples/node-managers/`), **When** the SDK tooling is applied, **Then** the samples compile and demonstrate the new, more ergonomic patterns.

---

### User Story 3 - RSA-KEM Integration Test (Priority: P3)

The RSA-KEM identity token decryption path (`async-opcua-crypto/src/identity/rsa_kem.rs`)
has unit-level correctness tests, but lacks an end-to-end integration test that exercises
the full client→server path: client encrypts a UserName token with RSA-KEM, server decrypts
it, and the session activates. This test was deferred in feature 055 because it requires
a full client+server setup with RSA certificates and two-phase secure client connect.

**Why this priority**: RSA-KEM is the mandatory-to-implement encrypted secret algorithm for
RSA-based security policies (Part 4 §6.7.1). Without an integration test, regressions in
the full round-trip path could go undetected. The deferred status since feature 055 makes
this the oldest known test gap.

**Independent Test**: Run `cargo test -p async-opcua --test integration -- rsa_kem` and
verify it passes, exercising a client connecting with RSA-KEM-encrypted UserName token
against a server with an RSA certificate.

**Acceptance Scenarios**:

1. **Given** a server with an RSA Application Instance Certificate, **When** a client connects with an RSA-KEM-encrypted UserName identity token, **Then** the server successfully decrypts the token and activates the session.
2. **Given** a server with an RSA certificate, **When** a client sends a UserName token with a deliberately corrupted RSA-KEM ciphertext, **Then** the server rejects the activation with an appropriate status (e.g., Bad_IdentityTokenInvalid).
3. **Given** the integration test, **When** `cargo test` runs, **Then** the RSA-KEM test is not `#[ignore]`d and passes reliably.

---

### User Story 4 - Embedded Profile Secure Channel Smoke Test (Priority: P3)

The embedded profile sample (`samples/foundation-profile-embedded-server/`) has a
`#[ignore]`d test `secure_channel_basic256sha256_sign_encrypt` that verifies the server's
application-instance certificate works for secure channel establishment. The test is
ignored because the client needs a two-phase connect (GetEndpoints to extract the server
cert, then reconnect with Sign&Encrypt). This was deferred in feature 054.

**Why this priority**: The embedded profile is the second most capable foundation profile
(after standard), targeting resource-constrained but security-capable devices. Without a
passing secure-channel test, there's no automated verification that the embedded server's
SSL/TLS certificate pipeline works end-to-end with a real client.

**Independent Test**: Run `cargo test -p async-opcua-foundation-profile-embedded-server`
and verify the `secure_channel_basic256sha256_sign_encrypt` test passes (not ignored).

**Acceptance Scenarios**:

1. **Given** the embedded profile server started with its auto-generated application instance certificate, **When** a client performs a two-phase connect (GetEndpoints over None, then reconnect with Sign&Encrypt), **Then** the secure channel is established and a Read of ServerStatus returns Good.
2. **Given** the embedded profile test suite, **When** `cargo test -p async-opcua-foundation-profile-embedded-server` runs, **Then** no tests are `#[ignore]`d that can be reasonably un-ignored with test harness improvements.

---

### User Story 5 - Standard Profile X509/RegisterServer2 Tests (Priority: P3)

The standard profile sample (`samples/foundation-profile-standard-server/`) has two
`#[ignore]`d tests: `x509_user_token_activation` (needs two-phase secure client connect
+ X509 token provisioning) and `register_server2_flow` (needs in-process LDS peer).
These were deferred in feature 054.

**Why this priority**: The standard profile is the most capable foundation profile. X509
user tokens and RegisterServer2 are mandatory CUs for the Standard 2017 UA Server Profile
(Part 12 §4.2.2). Without passing tests, regressions in these surface areas are invisible.

**Independent Test**: Run `cargo test -p async-opcua-foundation-profile-standard-server`
and verify both previously `#[ignore]`d tests pass (not ignored).

**Acceptance Scenarios**:

1. **Given** the standard profile server with an X509 user token configured, **When** a client performs a two-phase connect and activates a session with an X509 identity token, **Then** the session activates successfully.
2. **Given** the standard profile server and an in-process LDS peer server, **When** the standard server performs periodic RegisterServer2, **Then** the LDS peer receives and acknowledges the registration.
3. **Given** the standard profile test suite, **When** `cargo test -p async-opcua-foundation-profile-standard-server` runs, **Then** the X509 and RegisterServer2 tests pass (not ignored).

---

### Edge Cases

- **OCSP responder**: What happens when the responder's signing certificate expires? How does it handle OCSP request extensions (nonce, acceptable response types)? What about responses larger than a single HTTP response can carry? How does the responder handle concurrent OCSP requests?
- **SDK tooling**: Can the new tooling coexist with existing direct `NodeManager` trait implementations? Does it impose any allocation or performance overhead over the raw trait? What about `no_std` or embedded targets that the foundation profiles target?
- **RSA-KEM test**: What key sizes are tested (2048, 4096)? Does the test handle the case where both client and server have RSA certs but the client chooses a different security policy? What about ECC-only servers — does the test infrastructure handle the case where no RSA endpoint exists?
- **Embedded/Standard profile tests**: What happens when the two-phase connect times out in CI? How are test certificates provisioned — are they generated at test time or checked in? What about test isolation — does the LDS peer for RegisterServer2 interfere with other tests?
- **Cross-item**: Are any of these items blocked by the two-phase secure client connect limitation that has been noted since feature 054? If so, does that infrastructure need to be built first?

## Requirements *(mandatory)*

### Functional Requirements

#### US1 — OCSP Responder

- **FR-001**: System MUST provide an OCSP responder that can generate RFC 6960 compliant OCSP responses for certificates issued by a configured CA.
- **FR-002**: System MUST support configuring the responder with a CA certificate and private key, a certificate status database (at minimum: serial number → status mapping), and a response signing key.
- **FR-003**: System MUST respond to valid OCSP requests with a signed OCSP response containing the certificate status (good, revoked, or unknown) for each requested certificate.
- **FR-004**: System MUST include the correct `thisUpdate` and `nextUpdate` fields in responses, derived from the current time and a configurable response validity interval.
- **FR-005**: System MUST respond to malformed OCSP requests with a properly formed OCSP error response (per RFC 6960 §4.2.1) rather than crashing or producing invalid output.
- **FR-006**: System MUST support the OCSP nonce extension — if a request includes a nonce, the response MUST echo it back.

#### US2 — SDK Node-Manager Tooling

- **FR-007**: System MUST provide a builder or helper API that reduces the boilerplate required to create a custom node manager from ~30 lines to ≤10 lines for the common case of adding a few Variables with callbacks.
- **FR-008**: System MUST support adding read callbacks and write callbacks through the new tooling API with consistent, documented patterns. Method callbacks are out of scope for the builder API — advanced users use the raw `MethodProvider` trait.
- **FR-009**: System MUST ensure existing direct `NodeManager` trait implementations continue to work alongside the new tooling without breakage or behavioral change.
- **FR-010**: System MUST update the `samples/node-managers/` example to use the new tooling where it reduces boilerplate, while keeping the example functional.
- **FR-011**: System MUST update `docs/advanced_server.md` and/or `docs/server.md` to document the new SDK tooling patterns.

#### US3 — RSA-KEM Integration Test

- **FR-012**: System MUST provide an integration test that exercises the full client→server path for UserName identity token activation encrypted with RSA-KEM.
- **FR-013**: The integration test MUST use a real (auto-generated at test time) RSA Application Instance Certificate on the server side.
- **FR-014**: The integration test MUST be included in the standard `cargo test` suite (not `#[ignore]`d).
- **FR-015**: The test MUST verify both the success path (valid token accepted) and the failure path (corrupted ciphertext rejected).

#### US4 — Embedded Profile Secure Channel Test

- **FR-016**: System MUST un-ignore and implement the `secure_channel_basic256sha256_sign_encrypt` test in `samples/foundation-profile-embedded-server/tests/profile_smoke.rs`.
- **FR-017**: The test client MUST perform a two-phase connect: first fetch endpoints over None policy to obtain the server certificate, then reconnect with Sign&Encrypt using that certificate.
- **FR-018**: The test MUST be included in the standard `cargo test` suite (not `#[ignore]`d).

#### US5 — Standard Profile X509/RegisterServer2 Tests

- **FR-019**: System MUST un-ignore and implement the `x509_user_token_activation` test in `samples/foundation-profile-standard-server/tests/profile_smoke.rs`.
- **FR-020**: For the X509 test: the test MUST provision an X509 user certificate, perform a two-phase secure client connect, and activate a session with the X509 identity token.
- **FR-021**: System MUST un-ignore and implement the `register_server2_flow` test in `samples/foundation-profile-standard-server/tests/profile_smoke.rs`.
- **FR-022**: For the RegisterServer2 test: the test MUST spawn an in-process LDS peer server (with the `discovery-mdns` feature), start the standard server, and verify the periodic RegisterServer2 call is received.

### Key Entities

- **OCSP Responder**: A service component that receives OCSP requests (RFC 6960), looks up certificate status in a configured status database, and returns signed OCSP responses. Configured with a CA certificate, signing key, and certificate status map.
- **Node Manager Builder**: A high-level SDK API that streamlines the creation of custom node managers by providing defaults for most `NodeManager` trait methods, requiring the developer to specify only the nodes and callbacks relevant to their use case.
- **Two-Phase Secure Connect**: A client connection pattern where the client first calls GetEndpoints over a None-policy channel to obtain the server's Application Instance Certificate, then establishes a secure channel with Sign&Encrypt using that certificate. Currently implemented in test utility helpers but not used consistently across all profile tests.
- **LDS Peer Server**: A lightweight OPC UA Local Discovery Server started in-process during integration tests to receive and verify RegisterServer2 calls from the server under test.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An OCSP responder returns a valid, standards-compliant (RFC 6960) signed response for a known certificate within 1 second of receiving the request.
- **SC-002**: A custom node manager with one Variable and a read callback can be created and registered in 10 lines of chained builder code or fewer (excluding imports, `build()` call, and `with_node_manager()` registration).
- **SC-003**: The RSA-KEM integration test passes reliably in CI, with 0 `#[ignore]` annotations on RSA-KEM tests.
- **SC-004**: The embedded profile test suite has 0 `#[ignore]`d tests (that can be un-ignored with test harness improvements).
- **SC-005**: The standard profile test suite has 0 `#[ignore]`d tests (that can be un-ignored with test harness improvements).
- **SC-006**: The completeness-backlog.md is updated to reflect that all remaining items are complete.

## Assumptions

- The two-phase secure client connect helper needed for items 3, 4, and 5 already exists in some form (e.g., in `tests/common/` or can be extracted from existing integration test helpers). If not, building it is within scope for US3-US5.
- The OCSP responder can reuse the OCSP codec (`async-opcua-crypto/src/ocsp/codec.rs`) built in feature 057; only the responder-side request handling and response generation need to be added.
- "SDK node-manager tooling" means ergonomic wrappers and builders around the existing `InMemoryNodeManager` / `NodeManager` trait, not a ground-up rewrite. The existing trait decomposition (capability sub-traits from feature 009) remains the foundation.
- The LDS peer server for the RegisterServer2 test can be a minimal in-process server configured with the `discovery-mdns` feature, not a full production-grade LDS.
- Test certificates are auto-generated at test time (following existing patterns in the codebase) rather than checked in as static files.
- All work is Rust-only; the CTT certification run (needs Windows + OPC Foundation CTT) remains excluded from this feature per the SESSION-HANDOFF.
- This feature is the final backlog closeout; after this, the completeness-backlog.md and conformance-gap-backlog.md should be empty (or contain only the CTT item).
