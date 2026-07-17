# Feature Specification: GDS Pull Directory Singleton Correction (Run 1 rework)

**Feature Branch**: `104-gds-pull-directory-fix`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "GDS Pull Directory Singleton Correction (Run 1 rework): correct a wrong research finding and resulting over-engineering in feature 103 (GDS Pull Model Fix, merged PR #308)."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Server dispatches Pull-model calls against the real GDS Directory object (Priority: P1)

An operator running this server with the `companion-gds` feature enabled imports the real GDS companion NodeSet. A GDS client calls `StartSigningRequest`/`StartNewKeyPairRequest`/`FinishRequest`/`GetCertificateGroups`/`GetTrustList`/`GetCertificateStatus` against the server's actual, spec-shipped `CertificateDirectoryType` "Directory" object instance — not a parallel, hand-built stand-in object that happens to share a display name.

**Why this priority**: This is a correctness fix for already-merged, released behavior. The current implementation builds a duplicate "Directory" object with fabricated string-based NodeIds instead of resolving the real, spec-mandated instance the companion NodeSet ships — meaning any real GDS-aware client that browses the address space would see two different "Directory" objects (a confusing, non-conformant address space) and would have no reason to trust the fabricated one is the canonical one.

**Independent Test**: Import the real GDS companion NodeSet into a running server, browse the `ObjectsFolder` for a "Directory" object, confirm exactly one exists at the real, spec-defined NodeId, and confirm a Call-service request to each of its six Mandatory methods reaches this server's registered Pull-model handlers.

**Acceptance Scenarios**:

1. **Given** the GDS companion NodeSet has been imported, **When** the server resolves the Pull-model Directory instance, **Then** it resolves the real, already-imported object (not a newly constructed one) and every Mandatory method NodeId it returns belongs to that real object.
2. **Given** the real Directory object's method NodeIds, **When** a client calls `StartNewKeyPairRequest` against the resolved object/method NodeId pair, **Then** the call reaches this server's registered Pull-model handler exactly as it does today.
3. **Given** a server without the GDS companion NodeSet imported, **When** the server attempts to resolve the Directory instance, **Then** resolution fails closed (returns nothing usable, logs a warning, never panics) — unchanged from current behavior.

### Edge Cases

- Companion XML present but missing the expected Directory object or expected method children (e.g. a future or non-conformant companion NodeSet build): resolution must fail closed per node, matching today's fail-closed pattern, not partially wire a broken object.
- Optional methods (`RevokeCertificate`/`GetCertificates`/`CheckRevocationStatus`): their real NodeIds now resolve successfully, but no method callback is registered for them in this fix — a Call to one of them must behave exactly as it would for any other unregistered method (no regression, no attempt at partial semantics).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST resolve the Pull-model "Directory" object and its Mandatory method NodeIds from the real object the GDS companion NodeSet ships, rather than constructing a new, separate object with fabricated identifiers.
- **FR-002**: The system MUST verify each resolved NodeId actually exists in the address space (fail closed with a logged warning, never panic) before using it, exactly as the current implementation already does for its own constructed NodeIds.
- **FR-003**: The system MUST continue routing `StartSigningRequest`, `StartNewKeyPairRequest`, `FinishRequest`, `GetCertificateGroups`, `GetTrustList`, and `GetCertificateStatus` Call-service requests to their existing registered handlers, now dispatched against the corrected (real) NodeIds instead of the previous fabricated ones.
- **FR-004**: The system MUST continue to resolve the real `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree the same real Directory object exposes, rather than a separately constructed one.
- **FR-005**: The system MUST NOT change externally observable Call-service behavior for any of the six Mandatory Pull-model methods (same inputs/outputs/status codes as before this fix, only the underlying NodeIds change).
- **FR-006**: The system MUST continue to have zero effect on any server built without the `companion-gds` feature enabled.
- **FR-007**: The system's documentation of deferred Optional methods (`RevokeCertificate`/`GetCertificates`/`CheckRevocationStatus`) MUST be updated to reflect the corrected reason for deferral (real NodeIds now resolve; missing ledger/CRL-mutation business logic is the actual remaining gap) rather than the previous, incorrect reason (no real object to register against).

### Key Entities

- **DirectoryInstanceNodeIds**: unchanged public shape (Directory object NodeId, six Mandatory method NodeIds, CertificateGroups/DefaultApplicationGroup/TrustList NodeIds) — only how its values are obtained changes, from constructing new nodes to resolving real, pre-existing ones.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After this fix, browsing a running server's address space (with `companion-gds` enabled and the companion NodeSet imported) finds exactly one "Directory" object of `CertificateDirectoryType`, not two.
- **SC-002**: All existing Pull-model integration and unit tests (feature 103's test suite) continue to pass unchanged in externally observable outcome, now exercising the real NodeIds.
- **SC-003**: Zero regression in the existing GDS Push-model (features 101/102) test suites, and zero build/behavior change for servers built without `companion-gds`.
- **SC-004**: The conformance evidence register (AUDIT_TABLE / CU-COVERAGE.md) and TODO.md accurately describe the corrected design and the updated (non-stale) reason the three Optional methods remain unimplemented.

## Assumptions

- The verified NodeIds cited in the originating investigation (Directory object and its method/subtree children) are correct as found against the local `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml`; this feature's own research phase re-verifies them independently before they are hardcoded, per this project's standing empirical-verification discipline.
- Implementing real semantics for the three Optional methods (an issuance ledger, real CRL mutation, a revocation-status lookup) remains explicitly out of scope for this fix — only the deferral's documented reasoning is corrected, not its outcome.
- The client-side GDS Pull fix ("Run 2", `async-opcua-client/src/gds/`) is deliberately sequenced after this correction and is not part of this feature.
