# Feature Specification: Kerberos SSO Integration Test & Keytab Plumbing

**Feature Branch**: `065-kerberos-finish`  
**Created**: 2026-07-07  
**Status**: Draft  
**Input**: User description: "Complete the deferred items from feature 064: integration test with local KDC in CI, and plumbing the keytab_path config through to GSSAPI."

## User Scenarios & Testing *(mandatory)*

### User Story 1 — CI proves Kerberos SSO works end-to-end (Priority: P1)

A developer pushes a change to the async-opcua server. The CI pipeline automatically sets up a local MIT Kerberos KDC, starts the server with a Kerberos service principal, acquires a Kerberos ticket as a test user, sends it as an OPC UA IssuedToken, and verifies the user is authenticated with their correct principal name. The developer does not need a domain controller or IT setup — everything runs in CI.

**Why this priority**: Without a CI test, Kerberos SSO is untested and could silently regress. This is the last missing piece to make Kerberos SSO production-ready.

**Independent Test**: Run `tools/ci-playbook.sh --ci` on a machine with `libkrb5-dev` installed. The Kerberos integration test step (new) passes: server starts, test user `operator1@PLANT.LOCAL` authenticates via GSSAPI, session identity is `operator1@PLANT.LOCAL`. Then `kdestroy` destroys tickets, next attempt fails with `BadIdentityTokenRejected`.

**Acceptance Scenarios**:

1. **Given** a CI runner with `libkrb5-dev`, **When** the Kerberos test step runs, **Then** a local MIT KDC is started, a test user and service principal are created, a keytab is exported, and the server starts with Kerberos enabled.
2. **Given** the server running with Kerberos, **When** a client acquires a Kerberos service ticket via GSSAPI and sends it as an IssuedToken, **Then** the server authenticates the user and returns the correct principal name.
3. **Given** the same setup, **When** the client destroys its Kerberos tickets and retries, **Then** the server rejects the connection with `BadIdentityTokenRejected`.

---

### User Story 2 — Server uses keytab_path instead of env var (Priority: P2)

An IT administrator deploys the OPC UA server and specifies `kerberos_keytab("/etc/opcua.keytab")` in the server builder. The server uses this file directly for GSSAPI credential acquisition, without requiring the `KRB5_KTNAME` environment variable to be set. The admin can deploy the server with a simple file path — no shell environment configuration needed.

**Why this priority**: Reliance on an env var is fragile in production deployments (systemd, containers, etc.). A direct file path is the standard expectation for any config option.

**Independent Test**: Start the server with `kerberos_keytab("/tmp/test.keytab")`. Verify GSSAPI credential acquisition succeeds using the specified keytab file. Verify the server does NOT use the `KRB5_KTNAME` env var (can be confirmed by unsetting it before server start).

**Acceptance Scenarios**:

1. **Given** `kerberos_keytab("/tmp/test.keytab")` is set, **When** the server starts, **Then** the validator uses the specified keytab file for credential acquisition.
2. **Given** `kerberos_keytab()` is NOT called, **When** the server starts, **Then** the server falls back to the GSSAPI default keytab path (the same behavior as before — system default or `KRB5_KTNAME`).

---

### Edge Cases

- **KDC fails to start**: CI step fails with a clear error message indicating KDC startup failure.
- **Keytab has wrong permissions**: Server rejects with a clear error if the keytab file exists but cannot be read.
- **Clock skew in CI**: The test KDC and server run on the same machine, so clock skew is not an issue. The test does not cover clock skew.
- **Multi-step GSSAPI handshake**: The integration test only tests single-step (the typical Kerberos case). Multi-step is covered by the existing `GssapiIdentityValidator` code but not tested.
- **Keytab file contains wrong principal**: Server rejects with a clear error when the keytab doesn't contain a key for the configured SPN.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: CI MUST set up a local MIT Kerberos KDC with a test realm, test user, and service principal in `tools/ci-playbook.sh`.
- **FR-002**: CI MUST run an integration test that completes a full Kerberos SSO handshake: client acquires ticket via GSSAPI, server validates it, user is authenticated.
- **FR-003**: The integration test MUST verify the authenticated user's principal name matches the expected test user.
- **FR-004**: The integration test MUST destroy tickets and verify the next connection attempt is rejected.
- **FR-005**: The server MUST support using the configured `keytab_path` directly for GSSAPI credential acquisition instead of relying on `KRB5_KTNAME`.
- **FR-006**: The server MUST fall back to the GSSAPI default keytab path when `kerberos_keytab()` is not called.
- **FR-007**: The server MUST report a clear error if the configured keytab file cannot be read or is empty.
- **FR-008**: All existing tests MUST continue to pass regardless of whether the Kerberos KDC is running (the test is skipped gracefully when `libkrb5-dev` is absent).
- **FR-009**: The `libgssapi` dependency binding MUST be used to call `gss_acquire_cred_from` (or equivalent) to pass the keytab path explicitly.

### Key Entities

- **KDC Setup Script**: A bash function `setup_kerberos_kdc()` added to `tools/ci-playbook.sh` that installs `krb5-kdc`, creates realm `PLANT.LOCAL`, creates user `operator1` (password `testpass`), creates service principal `OPCUA/localhost`, exports keytab to `/tmp/opcua.keytab`.
- **Integration Test**: A Rust test in `async-opcua-server/tests/kerberos_sso.rs` that uses `libgssapi` client API to acquire a service ticket and send it as an OPC UA IssuedToken, then verifies the server response.
- **GssapiAcceptorCred**: A new abstraction wrapping `gss_acquire_cred_from` with an explicit keytab path, replacing the current implicit keytab resolution in `GssapiIdentityValidator::validate_token`.

## Success Criteria *(mandatory)*

- **SC-001**: A CI push triggers the Kerberos integration test, which passes on a machine with `libkrb5-dev` installed.
- **SC-002**: The integration test is skipped gracefully (not failed) on machines without `libkrb5-dev`.
- **SC-003**: The server authenticates a Kerberos user when `kerberos_keytab()` is called with a valid keytab path and `KRB5_KTNAME` is unset.
- **SC-004**: All 618+ existing tests continue to pass.

## Assumptions

- MIT Kerberos `krb5-kdc` and `krb5-admin-server` packages are available in the Ubuntu 24.04 CI image.
- The `libgssapi` crate exposes a way to pass a keytab path (via `gss_acquire_cred_from` or an extended `Cred::acquire` API) — if not, we fall back to setting `KRB5_KTNAME` in a child process.
- The integration test runs the server in-process or as a background process on localhost:4840.
- The `KerberosConfig` struct already has the `keytab_path` field from feature 064; this feature plumbs it through to the GSSAPI call.
