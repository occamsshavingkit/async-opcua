# Feature Specification: Spec Compliance Audit Fixes

**Feature Branch**: `059-spec-compliance-audit-fixes`  
**Created**: 2026-07-05  
**Status**: Draft  
**Input**: User description: "bring the implementation into compliance using the findings in @docs/spec-compliance-audit-2026-07-05.md"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Fix HIGH-Severity Spec Compliance Gaps (Priority: P1)

An OPC UA server operator expects that session creation assigns a default session name when none is provided, that client nonces are validated against the OPC UA specification range [32,128] bytes (not a config value), that Browse results respect the RESULT_MASK_IS_FORWARD bit, that CloseSecureChannel generates the mandatory audit event, that unactivated sessions are evicted at capacity, and that X509 user token signatures are independently validated during ActivateSession.

**Why this priority**: These are mandatory behaviors required by the OPC UA specification (Part 4). Non-compliance can cause interoperability failures, security audit gaps, denial-of-service vulnerabilities, and authentication bypass risks.

**Independent Test**: Each fix can be verified independently by a targeted conformance test. A server built with these fixes passes all six HIGH-severity assertions.

**Acceptance Scenarios**:

1. **Given** a CreateSession request with null or empty sessionName, **When** the server processes the request, **Then** the server assigns a default session name (e.g., "Session-<sessionId>") as required by Part 4 §5.7.2.2 Table 15.

2. **Given** a CreateSession request with clientNonce of 16 bytes (below spec minimum), **When** the server processes the request, **Then** the server returns BadNonceInvalid. Given a clientNonce of 256 bytes (above spec maximum), the server returns BadNonceInvalid.

3. **Given** a Browse request with resultMask that does NOT include bit 1 (RESULT_MASK_IS_FORWARD), **When** the server builds ReferenceDescriptions, **Then** the `is_forward` field is cleared to false for all returned references.

4. **Given** a CloseSecureChannel request, **When** the server processes it, **Then** the server dispatches an audit Event of type AuditChannelEventType per Part 4 §6.5.5.

5. **Given** the server is at maximum session capacity and all existing sessions are unactivated, **When** a new CreateSession request arrives, **Then** the server closes the oldest unactivated session and creates the new session, rather than returning BadTooManySessions, per Part 4 §5.7.2.1.

6. **Given** an ActivateSession request with an X509IdentityToken and a missing or invalid userTokenSignature, **When** the server processes activation, **Then** the server returns BadApplicationSignatureInvalid.

---

### User Story 2 - Fix MEDIUM-Severity Spec Mismatches (Priority: P2)

An OPC UA server operator expects that ActivateSession preserves localeIds when a subsequent call passes null (rather than clearing them), that Browse rejects BrowseDirection::INVALID (value 3) with BadBrowseDirectionInvalid, that FindServersOnNetwork uses the correct comparison operator (strictly greater-than for record IDs), that generated serverNonce lengths are validated against [32,128], and that the security token CreatedAt accurately reflects renewal time.

**Why this priority**: These non-compliant behaviors can cause subtle interoperability issues with strict clients, unnecessary locale state loss, off-by-one duplication bugs, and timing mismatches during secure channel renewal.

**Independent Test**: Each fix has an independent conformance test. A server passes all MEDIUM-severity assertions.

**Acceptance Scenarios**:

1. **Given** a session with localeIds set to ["en-US"], **When** ActivateSession is called again with null localeIds, **Then** the session retains ["en-US"] rather than being cleared, per Part 4 §5.7.3.2 Table 17.

2. **Given** a Browse request with browseDirection set to 3 (INVALID), **When** the server processes the request, **Then** the server returns BadBrowseDirectionInvalid per Part 4 §5.9.2.4 Table 36.

3. **Given** a FindServersOnNetwork request with startingRecordId of 5, **When** the server queries registered servers, **Then** only records with record_id strictly greater than 5 are returned (not >= 5), per Part 4 §5.5.3.1.

4. **Given** a server configuration with sessionNonceLength set to 16 (below spec minimum), **When** the server generates a serverNonce, **Then** the nonce length is clamped to 32 bytes minimum. Given a config with nonce length 256, the nonce length is clamped to 128 bytes maximum.

5. **Given** an OpenSecureChannel RENEW request, **When** the server creates the response SecurityToken, **Then** the token's CreatedAt accurately reflects the time of renewal rather than a stale value.

---

### User Story 3 - Fix MINOR and LOW-Severity Deviations (Priority: P3)

An OPC UA server operator expects that CreateSession rejects requests with non-null authenticationToken (which must always be null), that the revisedSessionTimeout is never 0, that ECC security policies have nonzero security levels, that Browse subscription precision is consistent between create and modify paths, and that set_browse_name is not publicly mutable.

**Why this priority**: These are low-impact deviations that improve spec fidelity and code hygiene but do not cause interoperability failures. Fixing them rounds out spec compliance and reduces technical debt.

**Independent Test**: Each fix has a targeted test. A server passes all remaining assertions.

**Acceptance Scenarios**:

1. **Given** a CreateSession request with a non-null authenticationToken, **When** the server validates the request, **Then** the server returns BadUnexpectedError or BadRequestTypeInvalid, per Part 4 §5.7.2.2 Table 15.

2. **Given** a server configuration where maxSessionTimeoutMs is 0 and requestedSessionTimeout is 0, **When** the server assigns revisedSessionTimeout, **Then** the assigned value is greater than 0, per Part 4 §5.7.2.2 Table 15.

3. **Given** ECC security policies in the endpoint configuration, **When** the server reports security levels, **Then** ECC policies receive nonzero security level values reflecting their cryptographic strength.

4. **Given** a subscription creation request with a sub-millisecond publishingInterval, **When** the server converts the interval, **Then** the precision is preserved (microsecond-level) rather than truncated to milliseconds, matching the ModifySubscription path.

5. **Given** external code calling set_browse_name on a node, **When** building the library, **Then** the function is not publicly exposed, per Part 3 §6.2.7 immutability constraint.

---

### Edge Cases

- **Nonce length at boundaries**: clientNonce and serverNonce of exactly 32 bytes and exactly 128 bytes must be accepted. Lengths of 31 and 129 must be rejected.
- **Session eviction with mixed states**: When some sessions are activated and some are not, the eviction algorithm must only consider unactivated sessions. If all sessions are activated, BadTooManySessions is returned.
- **BrowseDirection value 3**: Explicitly validate that value 3 (invalid) is rejected while 0 (Forward), 1 (Inverse), and 2 (Both) are accepted.
- **LocaleIds edge cases**: Empty localeIds array and null localeIds must both preserve the existing localeIds. Only a non-null, non-empty array should overwrite.
- **RevisedSessionTimeout with low request timeout**: If the client requests 1ms and server minimum is 1ms, the result must still be > 0.
- **Record ID wrapping at boundary**: For FindServersOnNetwork, startingRecordId of 0 (first request) should not include record 0 on subsequent requests.
- **Browse resultMask interaction with add_unchecked**: When external references are added via add_unchecked, the result mask should still be applied to maintain consistent behavior regardless of reference source.
- **Cancel stub**: The existing Cancel no-op stub remains as-is since returning Good with cancelCount=0 is spec-compliant for having no outstanding requests.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Server MUST assign a default session name when CreateSession receives a null or empty sessionName (OPC-10000-4 §5.7.2.2).
- **FR-002**: Server MUST validate clientNonce length against the OPC UA specification range [32,128] bytes, independent of server configuration (OPC-10000-4 §5.7.2.2).
- **FR-003**: Server MUST validate generated serverNonce length is in the range [32,128] bytes, clamping or rejecting configuration values outside this range (OPC-10000-4 §5.7.2.2).
- **FR-004**: Server MUST apply RESULT_MASK_IS_FORWARD (bit 1) when building Browse ReferenceDescriptions, clearing is_forward when the bit is not set (OPC-10000-4 §5.9.2.2).
- **FR-005**: Server MUST dispatch an AuditChannelEventType audit event on CloseSecureChannel (OPC-10000-4 §6.5.5).
- **FR-006**: Server MUST evict the oldest unactivated session when at maximum session capacity and a new CreateSession arrives (OPC-10000-4 §5.7.2.1).
- **FR-007**: Server MUST independently validate userTokenSignature when an X509IdentityToken is used in ActivateSession (OPC-10000-4 §7.40.5, §5.7.3.2).
- **FR-008**: Server MUST preserve existing localeIds when ActivateSession receives null or empty localeIds on subsequent calls (OPC-10000-4 §5.7.3.2).
- **FR-009**: Server MUST reject BrowseDirection value 3 (Invalid) with BadBrowseDirectionInvalid (OPC-10000-4 §5.9.2.4).
- **FR-010**: Server MUST use strictly greater-than comparison for startingRecordId in FindServersOnNetwork (OPC-10000-4 §5.5.3.1).
- **FR-011**: Server MUST update tokenCreatedAt during OpenSecureChannel RENEW to accurately reflect renewal time (OPC-10000-6 §6.7.4).
- **FR-012**: Server MUST reject CreateSession requests with non-null authenticationToken (OPC-10000-4 §5.7.2.2).
- **FR-013**: Server MUST ensure revisedSessionTimeout is always greater than 0 (OPC-10000-4 §5.7.2.2).
- **FR-014**: Server MUST assign non-zero security levels to ECC security policies reflecting their cryptographic strength (OPC-10000-7 §4.8).
- **FR-015**: Subscription publishingInterval precision MUST be consistent between CreateSubscription and ModifySubscription paths (OPC-10000-4 §5.14.1.2).
- **FR-016**: The set_browse_name method MUST NOT be publicly exposed on node types, preserving BrowseName immutability (OPC-10000-3 §6.2.7).

### Key Entities

- **Session**: Represents a client-server session with properties including sessionName, sessionId, activation state (activated/unactivated), localeIds, and creation timestamp. Session eviction operates on unactivated sessions ordered by age.
- **SecureChannel**: Represents a cryptographic channel between client and server with properties including securityToken, tokenCreatedAt, and role. Closing a channel must generate an audit event.
- **Browse Result**: A ReferenceDescription built from address space nodes, filtered by resultMask bits including IS_FORWARD (bit 1), BROWSE_NAME (bit 6), DISPLAY_NAME (bit 7), NODE_CLASS (bit 8), REFERENCE_TYPE (bit 9), and TYPE_DEFINITION (bit 10).
- **Security Policy**: Represents a cryptographic profile (None, Basic128Rsa15, Basic256, Basic256Sha256, Aes128Sha256RsaOaep, Aes256Sha256RsaPss, ECC variants) with an associated securityLevel indicating cryptographic strength.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 6 HIGH-severity findings from the compliance audit are resolved and pass targeted conformance tests.
- **SC-002**: All 6 MEDIUM-severity findings from the compliance audit are resolved and pass targeted conformance tests.
- **SC-003**: All 4 MINOR-severity findings from the compliance audit are resolved and pass targeted conformance tests.
- **SC-004**: All 9 LOW-severity findings from the compliance audit are resolved.
- **SC-005**: No existing conformance tests regress — the full test suite passes with the same or better results.
- **SC-006**: Any OPC-UA CTT (Compliance Test Tool) run shows fewer failures than the pre-fix baseline, with 100% pass rate on the specific areas addressed by these fixes.

## Assumptions

- The existing test infrastructure (cargo test, integration tests) can be extended to cover the new spec-compliant behaviors.
- The server configuration defaults are reasonable; fixes add validation/clamping rather than requiring configuration file changes from users.
- X509 user token signature validation reuses existing cryptographic infrastructure in async-opcua-crypto.
- The AuditChannelEventType type exists in the generated address space from the official NodeSet2.xml and can be instantiated for dispatch.
- The existing Cancel stub (CANCEL-01) is intentionally deferred and is not in scope for this feature.
- The ECC asymmetric encryption gap (DISC-05) is a known limitation documented in the code and is not in scope for this feature.
- The subscription state machine deviation (SUB-02) is an intentional documented deviation and is not in scope beyond code documentation updates if needed.
- Deferred resource cleanup (SC-03) and redundant set_role (SC-04) are code hygiene items addressed at LOW priority.
