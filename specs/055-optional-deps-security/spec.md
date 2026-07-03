# Feature Specification: Optional Dependencies and Security Hardening

**Feature Branch**: `055-optional-deps-security`  
**Created**: 2026-07-03  
**Status**: Draft  
**Input**: User description: "Make async-opcua-pubsub and async-opcua-history-sqlite optional facade deps; add RSA-DH authenticated-encryption for identity tokens; implement better server security checks framework"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Optional pubsub and history-sqlite dependencies (Priority: P1)

An integrator building a server that does not use MQTT-based publish/subscribe or SQLite-based
history storage should not have those crates compiled into their binary. Today both are
unconditionally pulled into the umbrella crate's dependency tree regardless of the selected
feature set.

**Why this priority**: Eliminates dead code from every non-MQTT/non-SQLite build. Affects all
profile sizes immediately with zero behavior change. The dominant user is the embedded/Nano
profile integrator.

**Independent Test**: `cargo tree -p async-opcua --no-default-features --features nano`
shows zero `async-opcua-pubsub` and `async-opcua-history-sqlite` crates in the dependency
graph. A full-featured build (`--all-features`) still has both. Existing integration tests
pass unchanged.

**Acceptance Scenarios**:

1. **Given** a project depending on `async-opcua` with `features = ["nano"]`, **When** the
   dependency tree is resolved, **Then** `async-opcua-pubsub` and `async-opcua-history-sqlite`
   are absent from the resolved crates.
2. **Given** a project depending on `async-opcua` with `--all-features`, **When** the
   dependency tree is resolved, **Then** both crates are present as before (no regression).
3. **Given** the nano profile benchmark binary, **When** measured, **Then** the binary size
   is unchanged or smaller (the crates were already LTO-dead-stripped from the `base-server`
   binary, but removing them from the dependency tree prevents future regressions).

---

### User Story 2 - RSA-DH encrypted identity token support (Priority: P2)

A client connecting to a server that advertises an RSA-DH-based UserTokenPolicy can
authenticate with a UserName identity token encrypted using the server's RSA public key
via the RSA-DH algorithm defined in OPC UA Part 6 §6.7. The current implementation
supports legacy RSA (RSA15), RSA-OAEP, and ECC encrypted secrets, but not RSA-DH or
authenticated-encryption variants.

**Why this priority**: Enables interop with servers that require RSA-DH token encryption.
Part 4 §7.41 allows RSA-based encryption for USERNAME tokens even in Sign-only channels.
This is a spec-mandated capability for conformance with security profiles that require
RSA-DH key transport.

**Independent Test**: A test harness creates a server endpoint advertising
`SecurityPolicy::Basic256Sha256` with a USERNAME UserTokenPolicy whose
`securityPolicyUri` specifies RSA-DH. A client connects, creates a session, and activates
it with an encrypted UserName identity token. The activation succeeds with `StatusCode::Good`.

**Acceptance Scenarios**:

1. **Given** a server advertising Basic256Sha256 with RSA-DH encrypted USERNAME tokens,
   **When** a client activates a session with an RSA-DH-encrypted UserName identity token,
   **Then** the activation succeeds.
2. **Given** a server advertising the same endpoint, **When** a client sends a malformed
   RSA-DH encrypted token, **Then** the server rejects with `BadIdentityTokenRejected`.
3. **Given** the existing RSA-OAEP encrypted token test, **When** run, **Then** it
   continues to pass (no regression in existing crypto paths).

---

### User Story 3 - Server security checks framework (Priority: P3)

Server operators can inspect, audit, and configure the security posture of a running
server through a structured security checks framework. Security-relevant validation
results (certificate chain validation, user authentication decisions, channel security
negotiation) are centrally accessible rather than scattered across session, transport,
and crypto modules.

**Why this priority**: Auditing is a requirement in many regulated systems (OPC UA Part 2).
A centralized framework makes it possible to add security checks consistently and to expose
their results for diagnostics and compliance reporting. The current implementation
dispatches checks in ad-hoc locations with no uniform reporting.

**Independent Test**: A test server configured with a specific security posture (e.g.,
enforcing Basic256Sha256 with certificate validation) reports its security check results
through a defined interface. A test client connects and the server records security-relevant
events (certificate accepted/rejected, user authenticated/rejected) that can be enumerated.

**Acceptance Scenarios**:

1. **Given** a server with certificate validation enabled, **When** a client connects with
   an untrusted certificate, **Then** the rejection is recorded in the security checks
   framework with the reason code and client identity.
2. **Given** a server with user authentication enabled, **When** a valid user activates a
   session, **Then** the authentication result is recorded as a security check outcome.
3. **Given** the security checks framework, **When** a server operator queries the
   framework, **Then** they receive a list of all security-relevant events since server
   start, each with a timestamp, outcome, and affected identity.

---

### Edge Cases

- What happens when an integrator enables `pubsub` but not `server`? The feature flag
  should be independent — enabling pubsub without the server crate should not cause
  compilation errors.
- What happens when the server's certificate does not support RSA-DH key transport
  (e.g., an EC-only certificate)? The server should not advertise RSA-DH-based
  UserTokenPolicies, or should reject them gracefully.
- How does the security checks framework handle a flood of rapid validation failures?
  It must not allocate unboundedly or block the connection handler.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The umbrella crate MUST gate `async-opcua-pubsub` behind a `pubsub` feature
  flag that defaults to ON (preserving the current default surface).
- **FR-002**: The umbrella crate MUST gate `async-opcua-history-sqlite` behind a
  `history-sqlite` feature flag that defaults to ON.
- **FR-003**: The `nano`, `micro`, `embedded`, and `standard` profile aliases MUST NOT
  enable `pubsub` or `history-sqlite`.
- **FR-004**: The `server` and `base-server` feature aliases MUST continue to enable
  both `pubsub` and `history-sqlite` (preserving their pre-feature surface).
- **FR-005**: The server MUST support decryption of UserName identity tokens encrypted
  with the RSA-KEM algorithm as defined in OPC 10000-6 §6.7.3 (RSA Key Encapsulation
  Mechanism: RSA-OAEP key transport + AES-256-KeyWrap).
- **FR-006**: The server MUST advertise RSA-based encrypted UserTokenPolicies on endpoints
  whose certificate uses an RSA key, in accordance with OPC 10000-4 §7.41 Table 192
  (RSA-based SecurityPolicies are allowed for USERNAME tokens even when SecurityMode is
  None). On EC-only certificate endpoints the RSA-DH policy MUST be omitted.
- **FR-007**: RSA-DH token decryption MUST coexist with existing RSA15, RSA-OAEP, and
  ECC encrypted secret paths without regression.
- **FR-008**: The server MUST provide a centralized security check registry accessible
  during session setup, channel negotiation, and user authentication. This implements
  the audit log concept from OPC 10000-4 §6.5.1 (method 1: log the audit entry in a
  storage location), complementing the existing method 2 (event-based audit dispatch).
- **FR-009**: Each security check entry MUST record the fields defined in the
  SecurityCheckEntry entity (data-model.md): timestamp, category, outcome, reason code,
  and affected identity label.
- **FR-010**: The security check registry MUST be bounded — older entries are evicted
  when a configurable maximum count is reached.
- **FR-011**: The security check registry MUST be queryable through the `ServerHandle`
  (`security_checks()` and `security_check_count()` methods).
- **FR-012**: The `rbac` feature gate (already in place, per OPC 10000-18 §6.3) disables
  role-based access enforcement; the security checks framework MUST record RBAC decisions
  when the feature is enabled but MUST NOT fail or panic when it is disabled.

### Key Entities

- **Security Check Entry**: A record of a security-relevant validation performed by the
  server. Contains a timestamp, check category, Boolean outcome, StatusCode reason, and
  identity label. Stored in a bounded ring buffer configurable via `ServerConfig`.
- **UserTokenPolicy with RSA-DH**: An extension of the existing `UserTokenPolicy` structure
  that specifies `SecurityPolicy::RsaDh` as the encryption algorithm for UserName tokens.
- **Facade feature flags**: `pubsub` and `history-sqlite` Boolean feature flags on the
  `async-opcua` umbrella crate, defaulting to ON for backward compatibility.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A `nano`-profile build with the new feature flags has zero dependency on
  `async-opcua-pubsub` and `async-opcua-history-sqlite` (verified by `cargo tree`).
- **SC-002**: The existing workspace test suite passes with all feature combinations
  unchanged (full build, no-default build, per-profile builds).
- **SC-003**: An RSA-DH encrypted UserName token is successfully decrypted by the server
  and the session is activated (verified by an integration test).
- **SC-004**: The security check registry captures at least one certificate validation
  event and one user authentication event during a standard session lifecycle (verified
  by a unit test).
- **SC-005**: The security check registry does not exceed its configured maximum entry
  count under a sustained connection storm of 100 rapid connect/disconnect cycles.

## Assumptions

- `async-opcua-pubsub` and `async-opcua-history-sqlite` are already LTO-dead-stripped in
  profile builds that don't use them; the feature flags prevent them from being resolved
  in the dependency tree at all, improving `cargo check`/`cargo build` time and preventing
  future regressions.
- RSA-DH key transport follows the RSA-KEM (Key Encapsulation Mechanism) pattern defined
  in OPC UA Part 6 §6.7.3, using the server's RSA public key to encrypt a symmetric key
  which then wraps the identity token.
- The security check registry is an in-memory, process-local data structure exposed through
  the existing `ServerHandle` or diagnostics node manager. It does not persist across
  server restarts.
- The `rbac` feature (gated in feature 054) restricts role-based access enforcement but
  does not remove the identity infrastructure; the security checks framework can record
  identity information regardless of RBAC state.
- Existing RSA-OAEP and ECC encrypted secret implementations are correct and will not be
  refactored — the RSA-DH path is additive.
