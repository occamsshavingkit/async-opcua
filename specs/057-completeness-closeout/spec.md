# Feature Specification: Completeness Closeout

**Feature Branch**: `057-completeness-closeout`  
**Created**: 2026-07-04  
**Status**: Draft  
**Input**: User description: "close out the remaining completeness backlog items: OCSP live fetching, multi-cert mixed server, async-delivery actor phases 2 & 4, bad ideas example servers"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Live OCSP Revocation (Priority: P1)

Server operators deploying OPC UA in environments where CRLs are not distributed (or are stale) need
online certificate revocation checking. The OPC UA specification (Part 4 §6.1.3) defines OCSP as a
required revocation method alongside CRLs. Currently, the validator processes supplied/stapled OCSP
responses but does not perform live fetching — operators must manually supply OCSP responses or rely
solely on CRLs.

**Why this priority**: Completes Part 4 §6.1.3 certificate validation — the last remaining gap in the
PKI/revocation story. Without live OCSP, deployments in regulated environments (utilities, manufacturing)
must supplement with external OCSP infrastructure, undermining the "complete reference implementation" goal.

**Independent Test**: Configure a server with an OCSP responder URL (in the certificate's AIA extension),
present a certificate whose issuer has a live OCSP endpoint, and verify the server fetches and validates
the OCSP response before accepting the connection. Test with an OCSP response indicating "revoked" and
verify the connection is rejected.

**Acceptance Scenarios**:

1. **Given** a server configured with OCSP fetching enabled, **When** a client presents a certificate with an AIA OCSP URL, **Then** the server fetches a valid OCSP response from the responder, validates it, and accepts the connection.
2. **Given** a server configured with OCSP fetching enabled, **When** a client presents a certificate that has been revoked (OCSP response status = "revoked"), **Then** the server rejects the connection with an appropriate certificate-revoked status.
3. **Given** a server configured with OCSP fetching enabled, **When** the OCSP responder is unreachable, **Then** the server either hard-fails (if strict mode) or falls back to CRL checking (if soft mode), as configured by the operator.
4. **Given** a server configured with OCSP fetching disabled (default), **When** a client connects, **Then** behavior is unchanged — only supplied/stapled OCSP responses are validated, preserving backward compatibility.

---

### User Story 2 - Multi-Cert Mixed Server (Priority: P2)

A single OPC UA server currently supports either RSA or ECC endpoints, but not both simultaneously.
Production deployments often need to serve both RSA and ECC endpoints from the same server instance to
accommodate diverse client capability profiles. The OPC UA specification (Part 4 §5.4.2, §6.1) allows a
server to advertise multiple endpoints with different security policies, including mixed RSA and ECC.

**Why this priority**: Closes a documented limitation in the transport layer — the conformance test
harness currently acknowledges "a single server cert cannot serve both RSA and ECC." This is the
highest-impact remaining non-YAGNI gap for production deployment flexibility.

**Independent Test**: Configure a server with both an RSA certificate and an ECC certificate, create
endpoints for RSA (Basic256Sha256) and ECC (EccNistP256), start the server, and verify both endpoints
accept connections from clients using the corresponding security policy.

**Acceptance Scenarios**:

1. **Given** a server configured with both an RSA and an ECC certificate, **When** a client connects to an RSA endpoint, **Then** the server selects the RSA certificate and completes the RSA handshake successfully.
2. **Given** a server configured with both an RSA and an ECC certificate, **When** a client connects to an ECC endpoint, **Then** the server selects the ECC certificate and completes the ECC handshake successfully.
3. **Given** a server configured with only an RSA certificate, **When** a client connects to an ECC endpoint, **Then** the endpoint fails with a clear error (cannot serve ECC without an ECC cert).
4. **Given** a server with no certificate at all, **When** starting, **Then** only None-policy endpoints succeed; any security-policy endpoint fails at startup with a clear diagnostic.

---

### User Story 3 - Delete LegacyCall Actor Variant (Priority: P3)

The session subscription actor currently uses a `LegacyCall` variant — a boxed dynamic closure — for
management operations (CreateSubscription, ModifySubscription, DeleteSubscriptions, etc.). This was a
phase-1 escape hatch to ship the actor architecture without migrating every call site. All other actor
messages are statically-typed enum variants; `LegacyCall` is the only remaining dynamic-dispatch path
and the only use of heap-allocated closures on the actor message channel.

**Why this priority**: Completes the actor refactor started in features 006/044/046. `LegacyCall`
represents technical debt in the hottest subscription path — every message to the actor goes through
the same channel, and the `LegacyCall` variant adds indirection and prevents exhaustive compiler checks.

**Independent Test**: After removal, `rgrep LegacyCall` returns zero results in the codebase. All
existing subscription tests (306 in `async-opcua-server --lib`) pass without modification.

**Acceptance Scenarios**:

1. **Given** the current codebase with `LegacyCall` used for management operations, **When** each management operation is migrated to a dedicated enum variant, **Then** `LegacyCall` is deleted from the actor enum.
2. **Given** all management operations use dedicated variants, **When** the full server test suite runs, **Then** all 306 tests pass with identical behavior.
3. **Given** the actor no longer uses boxed closures, **When** a new subscription operation is added, **Then** the compiler exhaustively checks all match arms for the new variant.

---

### User Story 4 - "Bad Ideas" Example Servers (Priority: P4)

The TODO.md calls for example servers that demonstrate the library's flexibility in unconventional ways.
These serve as living documentation and stress-test the SDK's extensibility. A persistent-store example
already exists; what's missing are creative demonstrations that show users how far they can push the
node manager abstraction.

**Why this priority**: Part of the SDK completeness story. After three years of building spec-conformance
features, the library needs examples that inspire users and prove the API surface is expressive enough
for real-world use.

**Independent Test**: Each example server compiles, starts, and can be browsed with a standard OPC UA
client (e.g., UaExpert) showing its address space. Each example has a README explaining what it
demonstrates.

**Acceptance Scenarios**:

1. **Given** the "chaos server" example, **When** a client browses its address space, **Then** it shows nodes that randomly change type, value, or status to exercise client error-handling paths.
2. **Given** the "filesystem bridge" example, **When** a client browses its address space, **Then** it mirrors the local filesystem as an OPC UA hierarchy — directories as folders, files as variables with contents as values.
3. **Given** any "bad ideas" example server, **When** `cargo run` is executed in its directory, **Then** the server starts and logs binding information without panicking.

---

### Edge Cases

- **OCSP**: What happens when the OCSP responder returns a response signed by a certificate not in the trust chain? What about OCSP responses that are valid but stale (thisUpdate/nectUpdate outside window)? What if the certificate has no AIA OCSP extension?
- **Multi-cert**: What if the server has TLS (opc.wss) endpoints alongside TCP endpoints — does cert selection work for both transports? What about wildcard or SAN-based cert matching?
- **LegacyCall removal**: Are there any management operations that fundamentally cannot be expressed as enum variants due to borrow/lifetime constraints? What about operations that need access to non-Send types?
- **Bad ideas servers**: How do they behave when the underlying system state changes (files deleted, network lost, etc.)? Do they need their own CI to prevent bit-rot?

## Requirements *(mandatory)*

### Functional Requirements

#### US1 — Live OCSP

- **FR-001**: System MUST provide an OCSP client that fetches OCSP responses from responder URLs found in the certificate's Authority Information Access (AIA) extension.
- **FR-002**: System MUST validate fetched OCSP responses: verify the responder certificate chains to a trusted root, the response signature, the response is within its validity window, and the serial number matches the certificate being checked.
- **FR-003**: System MUST integrate OCSP fetch results into the existing certificate validation pipeline (the `ChainValidationContext` / `CertificateStore`), treating a valid OCSP "good" response as satisfying the revocation check for that certificate.
- **FR-004**: System MUST support a configurable OCSP fetch policy: strict (hard-fail on unreachable responder), soft (fall back to CRL on unreachable), and off (stapled/supplied only, the current behavior).
- **FR-005**: System MUST enforce a configurable timeout on OCSP fetches and a configurable maximum response size, with sensible defaults, to prevent denial-of-service via slow or malicious responders.
- **FR-006**: System MUST NOT perform live OCSP fetching by default — the default policy is "off" (stapled/supplied only), preserving backward compatibility and avoiding unexpected network egress.

#### US2 — Multi-Cert Mixed Server

- **FR-007**: System MUST accept multiple certificates in server configuration, each associated with one or more security policies (e.g., one RSA cert for Basic256Sha256, one ECC cert for EccNistP256).
- **FR-008**: System MUST select the appropriate certificate for each endpoint based on the endpoint's security policy at connection time.
- **FR-009**: System MUST validate at startup that every security-policy endpoint has at least one compatible certificate configured; if not, it MUST fail with a clear diagnostic message naming the policy and the missing certificate type.
- **FR-010**: System MUST continue to support single-certificate configuration (the current model) as a valid subset of multi-cert configuration, with no change in behavior for single-cert deployments.
- **FR-011**: System MUST handle the opc.wss (WebSocket) transport identically to opc.tcp for certificate selection — the transport layer is transparent to cert-to-policy mapping.

#### US3 — Delete LegacyCall

- **FR-012**: System MUST replace each dynamic-dispatch subscription operation with a dedicated, statically-typed operation message.
- **FR-013**: System MUST remove the dynamic-dispatch operation pathway and all associated code paths (heap-allocated closures, type erasure, catch-all handlers).
- **FR-014**: System MUST preserve exact behavioral semantics for all subscription management operations — no observable difference in subscription lifecycle, monitoring, publishing, or error handling.
- **FR-015**: System MUST pass all existing subscription tests without modification, confirming identical behavior before and after the refactor.

#### US4 — Bad Ideas Servers

- **FR-016**: System MUST include a "chaos server" example that exposes an address space where node types, values, and status codes change unpredictably to exercise client error-handling.
- **FR-017**: System MUST include a "filesystem bridge" example that mirrors the local filesystem as an OPC UA address space (directories as Object nodes, files as Variable nodes with contents as values).
- **FR-018**: Each example server MUST compile and start without panicking when `cargo run` is invoked in its directory.
- **FR-019**: Each example server MUST include a README.md explaining what it demonstrates and how to run it.
- **FR-020**: Each example server MUST be discoverable via standard OPC UA Browse — a client connecting to the server can see and navigate the full address space.

### Key Entities

- **OCSP Fetch Policy**: Configuration setting with three modes — Off (no live fetching, current behavior), Soft (fall back to CRL when unreachable), Strict (reject on unreachable) — governing how the server performs online certificate revocation checks.
- **Certificate-to-Policy Association**: Mapping that links a certificate (identified by its key type, e.g., RSA or ECC) to the OPC UA security policies it can serve, enabling the server to select the correct certificate for each endpoint.
- **Subscription Management Operation**: A structured command — such as create, modify, or delete — sent to the subscription subsystem, carrying the operation's input data and a mechanism to return the result.
- **Example Server**: Standalone runnable program in the examples directory that demonstrates a specific library capability through a documented, browsable OPC UA address space.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A server configured with live OCSP fetching rejects a revoked certificate within 5 seconds of the client connection attempt (including OCSP fetch latency).
- **SC-002**: A single server instance accepts both RSA and ECC client connections simultaneously, each using the correct certificate for its security policy.
- **SC-003**: The subscription management subsystem uses only statically-typed operation messages with no dynamic dispatch or heap-allocated closures on the message channel.
- **SC-004**: All existing subscription lifecycle tests pass with identical behavior after the internal message refactor — no regressions in subscription create, modify, delete, publish, or monitoring.
- **SC-005**: All "bad ideas" example servers compile, start, and are browsable by a standard client within 30 seconds of `cargo run`.
- **SC-006**: The completeness-backlog.md is updated to reflect that all spec-conformance items are complete.

## Assumptions

- OCSP live fetching requires no new external dependencies beyond what the Rust ecosystem already provides (HTTP client, X.509 parsing). If a pure-Rust OCSP client library is unavailable, a minimal OCSP request/response codec will be built on existing `x509-cert` / `der` crates already in the dependency tree.
- Multi-cert support does not require changes to the client crate — only the server needs to select certificates per endpoint.
- `LegacyCall` migration does not require changes to the public API of `SessionSubscriptions` — the actor message interface is internal.
- "Bad ideas" servers are examples, not production code — they may panic on edge cases and are excluded from CI test gates beyond "compiles and starts."
- The persistent-store example server (already in `samples/persistent-store/`) is sufficient for the "sophisticated server with persistence" TODO.md item.
