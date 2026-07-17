# Feature Specification: GDS Pull Model Client-Side Fix (Run 2)

**Feature Branch**: `105-gds-pull-client-fix`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "GDS Pull Model Client-Side Fix (Run 2): async-opcua-client/src/gds/ (gds_client.rs/csr.rs/registration.rs) hardcodes fabricated NodeIds for an external, real GDS product's Directory object and methods. Fix via dynamic Browse/TranslateBrowsePath discovery, since every real GDS deployment assigns its own namespace index to the companion types."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Application registers with and requests a certificate from a real external GDS (Priority: P1)

An application using this SDK as an OPC UA client connects to a real, external Global Discovery Server (a separate product, not this SDK's own server) to register itself and obtain a signed application instance certificate. Today this always fails against a real GDS, because the client calls hardcoded NodeIds (`ns=0;i=22384` etc.) that don't correspond to anything on a real server — every real GDS deployment assigns its own namespace index to the GDS companion types, and the hardcoded namespace-0 identifiers were never valid to begin with (confirmed: they don't even match this SDK's own now-fixed server-side Pull-model NodeIds).

**Why this priority**: This is the only user-facing capability the GDS client module exists to provide (register + get a certificate). Without it, `GdsClient` cannot function against any real GDS product at all.

**Independent Test**: Connect to a real (or realistically namespace-shifted test double) GDS server, call `GdsClient::register_application`, `request_signing_csr`, and `poll_signing_request` in sequence, and confirm each call resolves and dispatches against the target server's actual NodeIds rather than failing with `Bad_NodeIdUnknown`/`Bad_MethodInvalid`.

**Acceptance Scenarios**:

1. **Given** a connected session to a GDS server that assigns the GDS companion namespace to some index the client doesn't know in advance, **When** the client resolves the Directory object and its methods, **Then** it discovers the real NodeIds dynamically (namespace index read from the server, node identities resolved via a standard path-based lookup) rather than using any hardcoded namespace-0 constant.
2. **Given** the resolved NodeIds, **When** `register_application`/`request_signing_csr`/`poll_signing_request` are called, **Then** each dispatches its Call request against the correct, resolved object/method pair.
3. **Given** a server that does not expose the GDS companion namespace at all (not a real GDS), **When** discovery is attempted, **Then** it fails with a clear, specific error (not a panic, not a confusing generic failure) before any Call is attempted.
4. **Given** discovery has already succeeded once for a session, **When** subsequent GDS calls are made on the same client, **Then** discovery is not needlessly repeated (the resolved NodeIds are reused for the life of the client/session pairing).

### Edge Cases

- A GDS server whose Directory object is present but missing an expected method (a non-conformant or partial deployment): discovery must fail closed for that specific unresolvable method with a clear error, not silently substitute a wrong NodeId or panic.
- Namespace index differs between separate sessions to the same or different GDS servers (e.g. a client reconnects, or talks to two different GDS deployments): each independent discovery must use that specific session's own resolved namespace/NodeIds, never a value cached from a different session.
- The "CertificateManager" root that `StartSigningRequest`/`FinishRequest` are called against is, per spec and per this SDK's own now-corrected server-side implementation, the same Directory object `RegisterApplication` is called against — not a separate object. The client's discovery and resulting data model must reflect this (one resolved root object, not two).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The client MUST discover the real NodeIds of the GDS Directory object and its `RegisterApplication`, `StartSigningRequest`, and `FinishRequest` methods (the methods this client already calls) via standard OPC UA discovery services against the connected session, not hardcoded namespace-0 constants.
- **FR-002**: Discovery MUST determine the target server's actual namespace index for the GDS companion namespace URI before attempting to resolve any GDS-specific node, since that index varies per deployment.
- **FR-003**: Discovery MUST resolve the Directory object and its `RegisterApplication`/`StartSigningRequest`/`FinishRequest` methods as children of the single real Directory object (not a separate "CertificateManager" object), matching the real GDS companion specification and this SDK's own corrected server-side model.
- **FR-004**: If any required node cannot be discovered (missing namespace, missing object, missing method), the client MUST fail closed with a specific, actionable error before attempting any Call — never panic, never fall back to a guessed NodeId.
- **FR-005**: Once discovery succeeds for a given client/session, subsequent calls on that same client MUST reuse the discovered NodeIds rather than re-discovering on every call.
- **FR-006**: `register_application`, `request_signing_csr`, and `poll_signing_request`'s existing public signatures and behavior (arguments, return types, error mapping) MUST NOT change for callers — only how the target NodeIds are obtained changes.
- **FR-007**: The fix is primarily client-side. **Revised during implementation**: writing a genuine end-to-end test (client vs. this SDK's own server, per the Independent Test above) surfaced two small, narrowly-scoped, genuinely necessary server-side infrastructure bugs that silently broke namespace discovery for *any* client (not just this SDK's own) against *any* server with a runtime-imported companion namespace -- see research.md's "Server-side infrastructure bugs found during testing" section. These were fixed as an unavoidable prerequisite for this feature's own testability, not as scope creep; server-side business logic/behavior is otherwise unchanged.

### Key Entities

- **Discovered GDS NodeIds**: the real Directory object NodeId and its `RegisterApplication`/`StartSigningRequest`/`FinishRequest` method NodeIds, resolved once per client instance against a live session and cached for reuse. Replaces the current `GdsRegistrationClient`/`GdsCsrClient` structs' hardcoded-default NodeId fields.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Against a GDS server that assigns the companion namespace to a non-zero, non-default index, the client successfully registers an application and completes a certificate signing round trip without any hardcoded NodeId matching by coincidence.
- **SC-002**: Against a server lacking the GDS companion namespace entirely, the client reports a clear discovery failure before any Call attempt, with no panic.
- **SC-003**: Repeated calls on the same client instance perform discovery at most once, verified by call-count instrumentation in tests.
- **SC-004**: Zero regression to any other client-crate functionality; zero change to this SDK's own server behavior.

## Assumptions

- "Real Browse/TranslateBrowsePath-based dynamic discovery" (as specified in TODO.md's tracked description of this Run 2 item) is implemented using this project's existing `Session::get_namespace_index` (namespace-index resolution) and `Session::translate_browse_paths_to_node_ids` (path-based NodeId resolution) client APIs — both already exist and are the standard OPC UA Part 4 §5.8.4 mechanism for exactly this "find a well-known node in an unknown namespace" scenario; no new session-level capability needs to be built.
- The GDS companion namespace URI (`http://opcfoundation.org/UA/GDS/`) and the well-known BrowseNames (`Directory`, `RegisterApplication`, `StartSigningRequest`, `FinishRequest`) are stable across conformant GDS deployments, per the companion specification — this is the same assumption this SDK's own server-side fix (features 103/104) already relies on and empirically verified.
- This feature does not add `StartNewKeyPairRequest`/`GetCertificateGroups`/`GetTrustList`/`GetCertificateStatus`/Optional-method client helpers — only fixes the NodeId resolution for the three methods the client already calls (`RegisterApplication`, `StartSigningRequest`, `FinishRequest`). Adding client support for the other Pull-model methods is a separate, not-yet-requested enhancement.
