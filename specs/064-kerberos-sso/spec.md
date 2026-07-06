# Feature Specification: Kerberos SSO Authentication

**Feature Branch**: `064-kerberos-sso`  
**Created**: 2026-07-06  
**Status**: Draft  
**Input**: User description: "Add Kerberos single sign-on to the OPC UA server so domain-joined operators can authenticate silently using their existing Windows/Linux session credentials — no password prompts — in air-gapped industrial environments."

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Operator opens web UI and is auto-authenticated (Priority: P1)

An operator sits at a Windows domain-joined workstation on the plant floor. They open the plant web UI (or Excel with an OPC UA plugin). The web UI connects to the local OPC UA server. Without any login prompt, the operator is authenticated as their domain identity and sees data scoped to their role.

**Why this priority**: This is the core value — zero-interaction SSO for domain users in air-gapped environments.

**Independent Test**: Start the OPC UA server with a Kerberos keytab. From a domain-joined client, connect with an IssuedToken containing a Kerberos service ticket. Verify the server accepts the connection and returns the principal's display name. Run `kdestroy` on the client (destroying tickets) — the next connection must fail with `BadIdentityTokenRejected`.

**Acceptance Scenarios**:

1. **Given** a domain-joined operator workstation and an OPC UA server configured with a valid Kerberos keytab, **When** the operator opens the plant web UI which connects via OPC UA using a Kerberos IssuedToken, **Then** the server authenticates the operator and the web UI displays data without a login prompt.
2. **Given** the same setup, **When** the operator's Kerberos ticket is expired, **Then** the server rejects the connection with an appropriate OPC UA status code.
3. **Given** the server configured for Kerberos, **When** a client presents a ticket for the wrong service principal, **Then** the server rejects it.

---

### User Story 2 — Administrator configures Kerberos via server config (Priority: P2)

An IT administrator deploys the OPC UA server on a Linux industrial PC joined to the plant Active Directory domain. They specify the service principal name (SPN) and path to a keytab file in the server configuration. The server automatically accepts Kerberos-authenticated connections.

**Why this priority**: Enables the feature; without configuration there's nothing to test.

**Independent Test**: Start the server with a config specifying `kerberos.keytab = "/etc/opcua.keytab"` and `kerberos.spn = "opcua/hostname@PLANT.LOCAL"`. Verify the server loads the keytab without errors and is ready to accept Kerberos tokens. Verify the server fails cleanly on startup if the keytab file is missing with a clear error message.

**Acceptance Scenarios**:

1. **Given** a valid keytab file at the configured path, **When** the server starts, **Then** it loads the keytab and logs readiness.
2. **Given** a missing keytab file, **When** the server starts, **Then** it reports a clear error and fails to start.
3. **Given** no Kerberos configuration at all, **When** the server starts, **Then** Kerberos authentication is disabled and all other authentication methods work normally.

---

### User Story 3 — RBAC role assignment from Kerberos principal (Priority: P3)

A plant administrator creates RBAC roles in OPC UA for "Operator", "Supervisor", and "Engineer". The Kerberos principal `engineer3@PLANT.LOCAL` is mapped to the Engineer role. When engineer3 connects, they can write to setpoint variables that operators cannot.

**Why this priority**: Identity without authorization is incomplete. But core authentication (US1) delivers immediate value even with a default role.

**Independent Test**: Configure the server with a principal-to-role mapping file. Connect as `engineer3@PLANT.LOCAL` and verify the session has the Engineer role. Attempt to write a variable restricted to Engineer — succeeds. Connect as `operator1@PLANT.LOCAL` — write is rejected.

**Acceptance Scenarios**:

1. **Given** a principal-to-role mapping `engineer3@PLANT.LOCAL → Engineer`, **When** engineer3 connects, **Then** the session carries the Engineer role.
2. **Given** a principal not in the mapping, **When** they connect, **Then** they receive a default role (e.g., Observer or Anonymous).

---

### Edge Cases

- **Expired tickets**: Server rejects with `BadIdentityTokenRejected`. Client refreshes via OS ticket renewal (transparent to user).
- **Clock skew**: Kerberos is sensitive to clock skew (default 5-minute window). Server logs a clear warning if the ticket is outside the window and rejects it.
- **Keytab rotation**: Admin replaces the keytab file on disk. Server picks up the new keytab on next restart (no hot-reload in initial implementation).
- **Cross-realm trust**: If the plant domain trusts a corporate domain, tickets from the corporate domain are accepted if the trust relationship is configured in the Kerberos infrastructure (handled by GSSAPI, not the server).
- **GSSAPI library not installed**: Server fails at startup with a clear error message indicating the missing system library.
- **Multiple service principals in keytab**: Server uses the principal matching the configured SPN; others are ignored.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Server MUST accept Kerberos service tickets wrapped in OPC UA IssuedToken identity tokens when the `kerberos` feature is enabled.
- **FR-002**: Server MUST validate Kerberos tickets using the GSSAPI `gss_accept_sec_context` flow against a configured keytab file.
- **FR-003**: Server MUST extract the client principal name from the validated GSSAPI context.
- **FR-004**: Server MUST map the client principal to a `UserToken` string, enabling the existing RBAC resolver to assign roles.
- **FR-005**: Server MUST support configuring the service principal name (e.g., `opcua/hostname@REALM`) and keytab file path via builder methods or config file.
- **FR-006**: Server MUST report a clear startup error if the keytab file is missing or unreadable when Kerberos is enabled.
- **FR-007**: Server MUST reject Kerberos-authenticated connections with `BadIdentityTokenRejected` when ticket validation fails (expired, wrong SPN, clock skew).
- **FR-008**: Client MUST be able to send a Kerberos service ticket as the token data in an OPC UA ActivateSession IssuedToken.
- **FR-009**: Kerberos support MUST be behind a Cargo feature flag (`kerberos`) so native GSSAPI dependencies are opt-in.
- **FR-010**: Server MUST function normally (all other auth methods: Anonymous, UserName, X509, JWT IssuedToken) when the `kerberos` feature is enabled or disabled.
- **FR-011**: Code implementing Kerberos GSSAPI calls MUST be isolated behind the `OAuth2IdentityValidator` trait so the validation pipeline remains pluggable.

### Key Entities

- **KerberosIssuedToken**: An OPC UA IssuedToken whose `tokenData` is a GSSAPI context-level token (a Kerberos AP-REQ wrapped in a GSSAPI token).
- **Service Principal Name (SPN)**: The Kerberos principal for the OPC UA service, e.g. `opcua/hostname.example.com@PLANT.LOCAL`. The server's keytab must contain keys for this SPN.
- **Keytab**: A file containing Kerberos encryption keys for service principals. Generated by the domain administrator (e.g., `ktpass` on Windows AD, `kadmin` on MIT Kerberos).
- **GssapiIdentityValidator**: A new implementation of `OAuth2IdentityValidator` that validates GSSAPI/Kerberos tokens instead of JWTs.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A domain-joined operator can open the plant web UI and see OPC UA data without typing a username or password.
- **SC-002**: A user whose Kerberos ticket has expired is rejected within the standard OPC UA ActivateSession round-trip time.
- **SC-003**: Server startup fails with a clear message (not a panic or cryptic error) when the configured keytab file is missing.
- **SC-004**: All existing tests (618 tests, `cargo test --all-features`) continue to pass regardless of whether the `kerberos` feature is enabled.
- **SC-005**: A principal-to-role mapping enables at least three distinct role levels (e.g., Operator, Supervisor, Engineer).
- **SC-006**: The Kerberos authentication path adds no measurable latency beyond the GSSAPI ticket validation time (typically < 50ms) to the ActivateSession handshake.

## Assumptions

- The plant environment runs a Kerberos realm (Active Directory or MIT Kerberos).
- Domain-joined client workstations have valid Kerberos TGTs acquired at user login.
- The `libgssapi` Rust crate provides adequate cross-platform GSSAPI bindings for both Linux (MIT Kerberos) and Windows (SSPI).
- The keytab file is provisioned out-of-band by the domain administrator; the server does not auto-generate keytabs.
- The OPC UA client can be configured to send Kerberos tokens as IssuedToken data; client-side ticket acquisition is handled by the OS GSSAPI library, not by the OPC UA client SDK.
- The `OAuth2IdentityValidator` trait is broad enough to encompass GSSAPI validation (the trait name is historical; the method signature `validate_token(&self, token: &str) -> Result<ClaimProfile, StatusCode>` works for GSSAPI tokens if we accept the token as base64-encoded binary data).
