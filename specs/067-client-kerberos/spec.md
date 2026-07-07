# Feature Specification: Client Kerberos SSO Support

**Feature Branch**: `067-client-kerberos`  
**Created**: 2026-07-07  
**Status**: Draft  
**Input**: User description: "Add Kerberos SSO support to the OPC UA client — automatically acquire a Kerberos service ticket via GSSAPI and send it as an IssuedToken."

## User Scenarios & Testing

### User Story 1 — Client auto-acquires Kerberos ticket (Priority: P1)

A domain-joined operator runs an OPC UA client application. The client automatically acquires a Kerberos service ticket for the target server's SPN and authenticates without any password prompt.

**Independent Test**: From a domain-joined machine with a valid Kerberos TGT, run the client against a Kerberos-enabled server. The client acquires a ticket, sends it as an IssuedToken, and the session is activated with the operator's principal name.

### User Story 2 — Client builder exposes Kerberos config (Priority: P2)

A developer configures the client to use Kerberos via `ClientBuilder::kerberos_spn("OPCUA/hostname@REALM")`. The client handles ticket acquisition internally.

**Independent Test**: Build a client with `kerberos_spn()` configured. Connect to a Kerberos-enabled server. Verify the client sends a GSSAPI token as the IssuedToken.

## Functional Requirements

- **FR-001**: Client MUST support acquiring a Kerberos service ticket via GSSAPI when `kerberos` feature is enabled.
- **FR-002**: Client MUST wrap the GSSAPI token as `GSSAPI <base64>` in an `IdentityToken::IssuedToken`.
- **FR-003**: `ClientBuilder` MUST expose `kerberos_spn()` and `kerberos_keytab()` methods behind the `kerberos` feature.
- **FR-004**: Client MUST fall back to other identity tokens when Kerberos is not configured.
- **FR-005**: Kerberos client support MUST be behind the existing `kerberos` Cargo feature flag.
- **FR-006**: All existing client tests MUST continue to pass.

## Success Criteria

- **SC-001**: A domain-joined client authenticates to a Kerberos-enabled server without a password prompt.
- **SC-002**: All existing tests pass.
