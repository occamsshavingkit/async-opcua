# Feature Specification: GDS Push Model TrustList Completion (Run 2)

**Feature Branch**: `102-gds-push-trustlist`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "GDS Push Model Run 2: TrustList / CertificateGroup completion for CU 2231. Implements the TrustListType file-based read/write protocol (Open/OpenWithMasks/Read/Write/CloseAndUpdate) plus AddCertificate/RemoveCertificate on the DefaultApplicationGroup's TrustList, extending Run 1's certificate-rotation transaction to also cover TrustList changes."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An administrator updates which certificates the server trusts (Priority: P1)

A security administrator, using a certificate-management tool that speaks
the OPC UA Push model, connects to the server's TrustList, downloads the
current set of trusted/issuer certificates and their revocation lists,
constructs an updated set (adding a new trusted root, removing an expired
one), uploads it, and applies the change. Until they explicitly apply it,
the server's actual trust decisions are unaffected.

**Why this priority**: This is the core, mandatory workflow the TrustList
half of the Push model exists for (Part 12 §7.8.2) and the primary reason
CU 2231 was still only partially closed after Run 1. Without it, an
administrator can rotate the server's own certificate (Run 1) but can
never change which *other* certificates the server trusts.

**Independent Test**: Open the TrustList for reading, read back the
current trusted/issuer certificates and CRLs. Separately, open the
TrustList for writing, write an updated set, call `CloseAndUpdate`,
confirm the server reports a pending change is required, call
`ApplyChanges`, and confirm the server's actual trusted-certificate store
has changed.

**Acceptance Scenarios**:

1. **Given** an administrator connected over an authenticated channel with
   SecurityAdmin access, **When** they call `Open` in read mode against
   the TrustList, **Then** they can `Read` back the current TrustList
   contents as a well-formed structure and `Close` without side effects.
2. **Given** the same administrator, **When** they call `OpenWithMasks`
   requesting only the trusted-certificates portion, **Then** the returned
   data includes only that portion, not CRLs or issuer certificates.
3. **Given** a new, valid set of trusted/issuer certificates, **When** the
   administrator opens the TrustList for writing, writes the new data, and
   calls `CloseAndUpdate`, **Then** the server reports that `ApplyChanges`
   must be called before the change takes effect, and the server's actual
   trust decisions are unchanged until then.
4. **Given** a pending TrustList update, **When** the administrator calls
   `ApplyChanges` (the same method Run 1 built for certificate rotation),
   **Then** the server's trusted/issuer certificate stores actually change
   on disk and take effect.
5. **Given** a pending TrustList update the administrator no longer wants,
   **When** they call `CancelChanges` instead, **Then** the pending update
   is discarded and the server's trust decisions are unchanged.
6. **Given** an uploaded TrustList containing an invalid certificate,
   **When** `CloseAndUpdate` is called, **Then** the whole update is
   rejected and discarded, and the server's existing TrustList is
   unaffected.

---

### User Story 2 - An administrator makes a single trust change without a full replace (Priority: P2)

An administrator wants to add or remove one certificate from the trusted
or issuer list without downloading, editing, and re-uploading the entire
TrustList.

**Why this priority**: `AddCertificate`/`RemoveCertificate` are the
lightweight, common-case alternative to the full read/modify/write
workflow in User Story 1, and round out the TrustList method surface
required by CU 2231.

**Independent Test**: Call `AddCertificate` with a new trusted
certificate's DER bytes; confirm it is immediately reflected in the
TrustList. Call `RemoveCertificate` with a certificate's thumbprint;
confirm it is immediately removed.

**Acceptance Scenarios**:

1. **Given** a valid certificate's DER bytes, **When** an administrator
   calls `AddCertificate`, **Then** the certificate is immediately added
   to the trusted (or issuer) certificate store, with no separate
   `ApplyChanges` step required.
2. **Given** a certificate currently in the trusted store, **When** an
   administrator calls `RemoveCertificate` with its thumbprint, **Then**
   it is immediately removed.
3. **Given** a certificate that is a CA needed to validate another
   certificate still in the TrustList, **When** `RemoveCertificate` is
   called against it, **Then** the removal is rejected rather than leaving
   the remaining certificate un-validatable.
4. **Given** a TrustList already opened for writing by another
   administrator session, **When** a different session calls
   `AddCertificate` or `RemoveCertificate`, **Then** the call is rejected
   rather than silently interleaved with the pending write.

---

### Edge Cases

- The TrustList file handle is scoped to the session that opened it; a
  different session cannot `Read`/`Write`/`Close` using another session's
  handle.
- If an administrator opens the TrustList for writing and then goes idle
  without calling `Close`/`CloseAndUpdate`, the server automatically closes
  the file and discards any partial write after a bounded period of
  inactivity (Part 12 §7.8.2.1 ActivityTimeout), rather than leaving the
  TrustList locked indefinitely.
- A method requiring SecurityAdmin privilege, called by a session that
  lacks it, is rejected without taking any action.
- A method requiring an authenticated channel, called over an
  unauthenticated one, is rejected.
- A `CloseAndUpdate`-staged TrustList change and a Run-1-staged certificate
  update are mutually exclusive: only one pending transaction exists at a
  time server-wide, matching Run 1's existing single-transaction model.
- `DefaultHttpsGroup` and `DefaultUserTokenGroup` are separate
  CertificateGroups that exist in this project's generated nodeset
  alongside `DefaultApplicationGroup`, but are out of scope for this
  feature (see Assumptions).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST expose a working `Open` method on the
  TrustList that supports read-only and write(+erase-existing) modes and
  rejects any other requested mode.
- **FR-002**: The server MUST expose a working `OpenWithMasks` method that
  returns only the requested subset (trusted certificates, trusted CRLs,
  issuer certificates, issuer CRLs, or any combination) of the TrustList.
- **FR-003**: The server MUST expose a working `Read` method returning the
  contents of a previously-opened TrustList file handle.
- **FR-004**: The server MUST expose a working `Write` method accepting
  new TrustList content against a previously-opened, write-mode file
  handle.
- **FR-005**: The server MUST expose a working `CloseAndUpdate` method
  that stages the written TrustList content as a pending change requiring
  `ApplyChanges`, validating every certificate in the new trusted list
  before accepting the change, and discarding the entire update if any
  certificate fails validation.
- **FR-006**: `ApplyChanges` and `CancelChanges` (built in Run 1) MUST be
  extended to also commit or discard a pending TrustList change, in
  addition to the certificate-rotation change they already handle.
- **FR-007**: The server MUST expose a working `AddCertificate` method
  that immediately adds a single certificate to the trusted or issuer
  store without requiring `ApplyChanges`.
- **FR-008**: The server MUST expose a working `RemoveCertificate` method
  that immediately removes a single certificate (identified by thumbprint)
  from the trusted or issuer store without requiring `ApplyChanges`, and
  refuses removal of a CA certificate still needed to validate another
  certificate in the list.
- **FR-009**: All eight methods MUST enforce the access-control
  requirements specified for them (authenticated channel, SecurityAdmin
  privilege).
- **FR-010**: A TrustList file handle MUST be usable only by the session
  that opened it, and MUST be automatically closed (discarding any
  pending write) if left idle past a bounded timeout.
- **FR-011**: At most one write-mode TrustList open, or one
  `AddCertificate`/`RemoveCertificate` call, MUST be able to proceed at a
  time server-wide; concurrent attempts MUST be rejected rather than
  silently interleaved, consistent with Run 1's single-transaction model.

### Key Entities

- **TrustList**: the file-like object exposing the server's trusted and
  issuer certificates and their revocation lists for the
  `DefaultApplicationGroup` CertificateGroup.
- **TrustList file handle**: session-scoped state created by `Open`/
  `OpenWithMasks`, tracking the read/write buffer and mode until `Close`/
  `CloseAndUpdate` or timeout.
- **Certificate-update transaction**: extended from Run 1 to optionally
  also carry a pending TrustList change, resolved by the same
  `ApplyChanges`/`CancelChanges` methods.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A full read cycle (`Open`/`OpenWithMasks` → `Read` → `Close`)
  against the TrustList returns the server's actual current trusted and
  issuer certificates and CRLs, verified end-to-end.
- **SC-002**: A full write cycle (`Open` → `Write` → `CloseAndUpdate` →
  `ApplyChanges`) results in the server's trusted/issuer certificate
  stores actually changing, verified end-to-end.
- **SC-003**: `CancelChanges` reliably discards a pending TrustList update
  with no effect on the server's actual trust decisions.
- **SC-004**: `AddCertificate`/`RemoveCertificate` reliably make an
  immediate, single-certificate change without requiring `ApplyChanges`.
- **SC-005**: Every method rejects callers that don't meet its access
  requirements, verified for each of the eight methods.
- **SC-006**: CU 2231 evidence in the project's conformance ledger reflects
  full closure of the `DefaultApplicationGroup` TrustList surface, citing
  real, verified AddressSpace NodeIds and real tests exercising each
  method's success and failure paths.

## Assumptions

- Scope is limited to the `DefaultApplicationGroup` CertificateGroup's
  TrustList — the group backing the server's own application instance
  certificate, which this project's `CertificateStore` already manages.
  `DefaultHttpsGroup` and `DefaultUserTokenGroup` exist as separate,
  empirically-confirmed nodes in this project's generated nodeset but are
  explicitly deferred as a follow-up, mirroring Run 1's precedent of
  narrowing scope to what one feature can verify end-to-end.
- The CertificateGroup-level `GetRejectedList` (Part 12 §7.8.3.2) was
  empirically confirmed absent from this project's generated nodeset
  during Run 1's investigation; Run 1 already satisfies this requirement
  via `ServerConfiguration.GetRejectedList` reading the certificate store
  directly. No new work is needed for it in this feature.
- File-handle state (open mode, pending buffer, owning session, idle
  timeout) is newly built for the TrustList specifically; no existing
  generic FileType dispatcher exists elsewhere in this codebase to reuse.
- A single, whole-list validation failure during `CloseAndUpdate` discards
  the entire pending update rather than attempting partial acceptance,
  matching the specification's stated behavior ("If any errors occur, the
  new TrustList shall be discarded").
- Automatically re-evaluating already-open Sessions/SecureChannels against
  a newly-applied TrustList (so certificates that are no longer trusted
  cause those connections to be closed) is a real, spec-mandated behavior
  (§7.8.2.5) but its full scope (closing live connections mid-flight) is
  deferred as a documented simplification for this run if it proves to
  require broader session-manager changes than this feature's boundaries;
  the TrustList content itself is always correctly updated regardless.
