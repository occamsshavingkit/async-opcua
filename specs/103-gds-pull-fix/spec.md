# Feature Specification: GDS Pull Model Fix (Run 1)

**Feature Branch**: `103-gds-pull-fix`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "GDS Pull Model Fix (Run 1 of 2): fix CU 2230 'GDS Certificate Manager Pull Model' -- a fundamentally broken implementation in gds/pull_methods.rs. The real CertificateDirectoryType Pull-model surface doesn't exist anywhere in this server (wrong NodeIds, wrong concepts, and the type itself is only available via a currently-dormant companion NodeSet import); this run instantiates a real CertificateDirectoryType and implements its Mandatory Pull-model methods. The client-side GDS helpers (a separate, equally-broken fabricated-NodeId defect discovered during investigation) are an explicit Run 2."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An application obtains a new certificate from this server acting as its Certificate Manager (Priority: P1)

An administrator, using a certificate-management tool that speaks the OPC UA
Pull model, connects to this server (now acting as a Certificate Manager for
registered applications), requests a new certificate be signed either using
an application's own key (`StartSigningRequest`) or with a newly generated
key pair (`StartNewKeyPairRequest`), and later polls for the result
(`FinishRequest`) to retrieve the signed certificate.

**Why this priority**: This is the core, Mandatory workflow the Pull model
exists for (Part 12 §7.9) and the reason CU 2230 exists. The *existing* code
claims to support a Pull-model workflow but, per investigation, implements
entirely the wrong concepts (Push-model `GetRejectedList`/`UpdateCertificate`)
against NodeIds that resolve to nothing — the feature has never worked, and
worse, its target Object Type doesn't even exist in this server's
AddressSpace until this run builds it.

**Independent Test**: With the companion GDS NodeSet imported, call
`StartNewKeyPairRequest` for a registered application; confirm a request id
is returned. Call `FinishRequest` with that id before the request is
resolved; confirm it reports the request is still pending. Resolve the
request (server-internal signing step) and call `FinishRequest` again;
confirm it returns a real, valid certificate (and, since a new key pair was
requested, a private key).

**Acceptance Scenarios**:

1. **Given** a Certificate Manager administrator connected over an
   authenticated channel with the appropriate role, **When** they call
   `StartSigningRequest` with a valid CSR-equivalent public key and a
   supported certificate group/type, **Then** they receive a request id
   usable to later retrieve the signed certificate.
2. **Given** the same administrator, **When** they call
   `StartNewKeyPairRequest` instead, **Then** they receive a request id, and
   the eventual `FinishRequest` result includes both a certificate and a
   newly generated private key.
3. **Given** a request id that hasn't been resolved yet, **When**
   `FinishRequest` is called, **Then** the server reports the request is
   still pending rather than an error or a fabricated result.
4. **Given** a resolved request id, **When** `FinishRequest` is called,
   **Then** the actual signed certificate (and private key, if requested) is
   returned exactly once in a form the caller can install.
5. **Given** the `companion-gds` feature is not enabled, **When** the server
   starts, **Then** none of this behavior is present and no other server
   functionality is affected.

---

### User Story 2 - An application discovers what certificate groups and trust lists apply to it (Priority: P2)

An application registered with this Certificate Manager wants to know which
certificate groups it may request certificates from, which TrustList
governs its trust decisions, and whether its current certificate needs to be
updated.

**Why this priority**: `GetCertificateGroups`, `GetTrustList`, and
`GetCertificateStatus` are the remaining Mandatory `CertificateDirectoryType`
methods (Part 12 §7.9.7/§7.9.9/§7.9.10) that round out the conformance
surface independent of the certificate-issuance workflow in User Story 1.

**Independent Test**: Call `GetCertificateGroups` for a registered
application; confirm it returns the `DefaultApplicationGroup` NodeId. Call
`GetTrustList` for that group; confirm it returns the TrustList object's
NodeId. Call `GetCertificateStatus`; confirm it reports whether an update is
required.

**Acceptance Scenarios**:

1. **Given** a registered application, **When** `GetCertificateGroups` is
   called, **Then** the real `DefaultApplicationGroup` NodeId is returned
   (not a fabricated one).
2. **Given** a certificate group NodeId, **When** `GetTrustList` is called,
   **Then** the real TrustList object NodeId belonging to that group is
   returned.
3. **Given** a certificate group and type, **When** `GetCertificateStatus`
   is called, **Then** the server reports whether the application's
   certificate needs updating, based on real certificate state rather than
   a placeholder.

---

### Edge Cases

- A method requiring the appropriate administrative role, called by a
  session that lacks it, is rejected without taking any action.
- A method requiring an authenticated channel, called over an unauthenticated
  one, is rejected.
- `RevokeCertificate`, `GetCertificates`, and `CheckRevocationStatus` are
  Optional per specification; they are implemented only if unambiguous from
  the specification text without requiring new infrastructure beyond what
  this run already builds — otherwise explicitly deferred and documented,
  matching how prior GDS work deferred genuinely out-of-reach Optional
  methods.
- If the `companion-gds` Cargo feature is disabled, or the operator has not
  cloned the GDS companion NodeSet locally, none of this feature's behavior
  is present; the server otherwise builds and runs exactly as before.
- The previously-registered `GetRejectedList`/`UpdateCertificate` callbacks
  (which implemented the wrong model entirely, against non-existent
  NodeIds) are removed so they no longer silently do nothing when called.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: When the `companion-gds` feature is enabled, the server MUST
  be able to import the GDS companion NodeSet and construct a real, live
  `CertificateDirectoryType` instance in its own AddressSpace, including all
  of that type's Mandatory child methods and its
  `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` object subtree.
- **FR-002**: The server MUST expose a working `StartSigningRequest` method
  that begins a certificate-issuance workflow using a caller-supplied key.
- **FR-003**: The server MUST expose a working `StartNewKeyPairRequest`
  method that begins a certificate-issuance workflow including generation of
  a new private key.
- **FR-004**: The server MUST expose a working `FinishRequest` method that
  reports a pending request as not yet complete, and returns the actual
  signed certificate (and private key, if one was generated) once complete.
- **FR-005**: The server MUST expose a working `GetCertificateGroups`
  method returning real certificate group NodeIds.
- **FR-006**: The server MUST expose a working `GetTrustList` method
  returning the real TrustList NodeId for a given certificate group.
- **FR-007**: The server MUST expose a working `GetCertificateStatus`
  method reporting real certificate-update-needed status.
- **FR-008**: All methods MUST enforce the access-control requirements
  specified for them (authenticated channel, appropriate administrative
  role).
- **FR-009**: This entire feature MUST be inert when the `companion-gds`
  feature is disabled, with zero effect on any other server functionality.
- **FR-010**: The previously-registered, incorrectly-modeled
  `GetRejectedList`/`UpdateCertificate` Pull-model callbacks (which pointed
  at non-existent AddressSpace nodes) MUST be removed.

### Key Entities

- **CertificateDirectoryType instance**: the server's own
  Certificate-Manager root object, instantiated at startup from the
  imported companion type definition, exposing the Pull model's methods.
- **Pull-model request**: a staged, asynchronous
  certificate-signing-or-key-generation request created by
  `StartSigningRequest`/`StartNewKeyPairRequest`, resolved and retrieved via
  `FinishRequest`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A full Pull-model certificate-issuance workflow
  (`StartNewKeyPairRequest` → resolve → `FinishRequest`) results in a real,
  valid certificate and private key being returned, verified end-to-end.
- **SC-002**: `FinishRequest` reliably distinguishes a pending request from
  a completed one, never returning fabricated or empty certificate material
  for either.
- **SC-003**: `GetCertificateGroups`/`GetTrustList`/`GetCertificateStatus`
  return real, verifiable AddressSpace state rather than placeholders.
- **SC-004**: Every method rejects callers that don't meet its access
  requirements.
- **SC-005**: CU 2230 evidence in the project's conformance ledger cites
  real, verified AddressSpace NodeIds (dynamically resolved after
  companion import, not fabricated compile-time constants) and real tests
  exercising each method's success and failure paths.
- **SC-006**: A server built without the `companion-gds` feature is
  provably unaffected by this feature (no new behavior, no new failure
  modes).

## Assumptions

- This run instantiates exactly the object graph `CertificateDirectoryType`
  needs (its Mandatory methods and the `CertificateGroups` subtree) rather
  than building a fully generic "instantiate any ObjectType from its
  Mandatory modelling rules" engine. The instantiation logic is structured
  so it could be generalized later if another companion spec needs the same
  capability, but that generalization is not built speculatively now.
- `RevokeCertificate`, `GetCertificates`, and `CheckRevocationStatus` are
  Optional per OPC-10000-12 Table 74; they are implemented only if
  unambiguous from the specification text without requiring materially new
  infrastructure. Any left unimplemented are documented as a follow-up, not
  silently dropped.
- The client-side GDS helpers (`async-opcua-client/src/gds/gds_client.rs`,
  `csr.rs`, `registration.rs`) were found during investigation to have the
  *same* fabricated-NodeId defect this run fixes on the server side, but
  fixing them correctly requires dynamic NodeId discovery (Browse/
  TranslateBrowsePath against whatever external, real GDS product the
  client connects to) rather than the instantiation work this run does.
  This is explicitly out of scope for this run and tracked as Run 2.
- The GDS companion NodeSet2.xml file itself is never committed to this
  repository (per `schemas/companion/README.md`'s existing policy);
  developing and testing this feature requires it to be present locally at
  `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml`.
