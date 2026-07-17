# Feature Specification: GDS Push Model Fix + Completion (Run 1)

**Feature Branch**: `101-gds-push-fix`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "GDS Push model fix + completion (Run 1 of 2): fix broken NodeId wiring in gds/push_methods.rs (CU 2231) and implement the real ServerConfigurationType Push-model methods -- UpdateCertificate, ApplyChanges, CancelChanges, CreateSigningRequest, GetRejectedList, ResetToServerDefaults. TrustList/CertificateGroup methods deferred to a follow-up run."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An administrator pushes a new certificate to the server (Priority: P1)

A security administrator, using a certificate-management tool that speaks
the OPC UA Push model, connects to the server over an encrypted channel,
asks the server to prepare a signing request for its current key,
receives a newly-signed certificate back from a CA out of band, and pushes
that certificate to the server. The server stages the change, and only
takes effect once the administrator explicitly applies it — at which
point existing connections eventually pick up the new identity.

**Why this priority**: This is the core, mandatory workflow the Push
model exists for (Part 12 §7.10) and the reason CU 2231 exists. It is
also the workflow the *existing* code claims to support but, per
investigation, does not — the methods it registers point at the wrong
AddressSpace nodes entirely.

**Independent Test**: Call `CreateSigningRequest` on the server's
`ServerConfiguration` object; use the returned request to obtain a
certificate; call `UpdateCertificate` with that certificate; confirm the
server reports a pending change is required; call `ApplyChanges`; confirm
the server's application certificate has actually changed.

**Acceptance Scenarios**:

1. **Given** an administrator connected over an encrypted, authenticated
   channel, **When** they call `CreateSigningRequest`, **Then** they
   receive a certificate request usable to obtain a signed certificate
   from a CA.
2. **Given** a new, valid certificate for the server's key, **When** the
   administrator calls `UpdateCertificate`, **Then** the server reports
   that `ApplyChanges` must be called before the certificate takes effect,
   and the server's active certificate is unchanged until then.
3. **Given** a pending certificate update, **When** the administrator
   calls `ApplyChanges`, **Then** the server's application certificate is
   actually updated on disk and reloaded into active use.
4. **Given** a pending certificate update the administrator no longer
   wants, **When** they call `CancelChanges` instead, **Then** the
   pending update is discarded and the server's certificate is unchanged.
5. **Given** no pending update exists, **When** `ApplyChanges` or
   `CancelChanges` is called, **Then** the server reports there is
   nothing to do, rather than silently succeeding or erroring unclearly.

---

### User Story 2 - An administrator reviews and resets certificate state (Priority: P2)

An administrator wants to see which certificates the server has rejected
(to decide whether to trust one), and, separately, wants a way to reset
the server's security configuration back to its defaults if something
has gone wrong.

**Why this priority**: These are the two remaining Mandatory/commonly-
implemented ServerConfiguration methods that round out the Push model's
administrative surface, independent of the certificate-update workflow
in User Story 1.

**Independent Test**: Reject a certificate (by having a client attempt to
connect with an untrusted cert), then call `GetRejectedList` and confirm
it appears. Separately, call `ResetToServerDefaults` and confirm the
server signals an impending shutdown with a warning message.

**Acceptance Scenarios**:

1. **Given** at least one certificate the server has rejected, **When** an
   administrator calls `GetRejectedList`, **Then** they receive that
   certificate's DER-encoded bytes.
2. **Given** an authenticated administrator session, **When** they call
   `ResetToServerDefaults`, **Then** the server transitions toward
   shutdown and warns connected clients before doing so.

---

### Edge Cases

- Two different sessions cannot both have an active certificate-update
  transaction at once; the second is rejected until the first is
  resolved (applied or cancelled).
- A method requiring SecurityAdmin privilege, called by a session that
  lacks it, is rejected without taking any action.
- A method requiring an encrypted channel, called over a channel that is
  merely signed (not encrypted) or unsecured, is rejected.
- `CreateSelfSignedCertificate`, `DeleteCertificate`, and `GetCertificates`
  are part of the same `ServerConfigurationType` per specification, but
  investigation found their AddressSpace nodes are absent from the
  standard nodeset snapshot this project's code generator currently
  consumes — they cannot be wired to anything and are out of scope for
  this feature (see Assumptions).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST expose a working `CreateSigningRequest`
  method that returns a real, usable certificate signing request signed
  with the server's own key.
- **FR-002**: The server MUST expose a working `UpdateCertificate` method
  that stages a new certificate (and optionally a new private key) without
  taking effect immediately.
- **FR-003**: The server MUST expose a working `ApplyChanges` method that
  commits a pending certificate update and puts it into active use.
- **FR-004**: The server MUST expose a working `CancelChanges` method that
  discards a pending certificate update.
- **FR-005**: The server MUST expose a working `GetRejectedList` method
  that returns certificates the server has rejected.
- **FR-006**: The server MUST expose a working `ResetToServerDefaults`
  method that warns clients and moves the server toward a restart.
- **FR-007**: All six methods MUST enforce the access-control
  requirements specified for them (encrypted-vs-authenticated channel,
  SecurityAdmin privilege).
- **FR-008**: At most one certificate-update transaction MUST be active
  server-wide at a time; a second concurrent attempt MUST be rejected
  rather than silently interleaved.
- **FR-009**: The previously-registered method callbacks (which pointed
  at incorrect AddressSpace nodes) MUST be removed so they no longer
  silently do nothing when called against their old, wrong locations.

### Key Entities

- **ServerConfiguration**: the server's own manageable-configuration
  Object, target of the Push model's methods.
- **Certificate-update transaction**: a staged, not-yet-effective
  certificate change created by `UpdateCertificate`, resolved by
  `ApplyChanges` or `CancelChanges`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A full push-model certificate rotation (CreateSigningRequest
  → UpdateCertificate → ApplyChanges) results in the server's active
  certificate actually changing, verified end-to-end.
- **SC-002**: CancelChanges reliably discards a pending update with no
  effect on the server's active certificate.
- **SC-003**: Every method rejects callers that don't meet its access
  requirements, verified for each of the six methods.
- **SC-004**: CU 2231 evidence in the project's conformance ledger cites
  real, verified AddressSpace NodeIds (not fabricated ones) and real
  tests exercising each method's success and failure paths.

## Assumptions

- `CreateSelfSignedCertificate`, `DeleteCertificate`, and `GetCertificates`
  are Optional per OPC-10000-12 Table 87 and are explicitly deferred: their
  target AddressSpace nodes were verified (empirically, via a live Read
  against a running server) to be absent from the nodeset this project
  currently generates from. They cannot be implemented until the imported
  nodeset schema includes them; documented as a nodeset-source gap, not an
  implementation gap.
- The full multi-method "transaction queue" model (§7.10.2, where several
  Methods can be invoked before one `ApplyChanges`) is scoped down to its
  simplest correct form for this feature: a single pending certificate
  update per transaction (from `UpdateCertificate`), since the
  TrustList-side methods that would also participate in a shared
  transaction are deferred to the follow-up run. This still satisfies the
  spec's stated minimum ("Servers that do not support transactions...
  apply any changes before returning a Method response" is one valid
  model; this feature implements the alternative valid model where
  `ApplyChangesRequired` is `TRUE` and a real `CancelChanges` exists).
- Automatically cancelling a transaction when its owning Session
  disconnects (§7.10.2) is deferred as a documented simplification for
  this run; an abandoned transaction can still be resolved by another
  administrator calling `CancelChanges`, but there is no automatic
  timeout yet.
- A sibling, more severely broken Pull-model implementation
  (`gds/pull_methods.rs`, CU 2230) was discovered during investigation
  (fabricated NodeIds, plus Push-model methods mis-categorized as Pull).
  It is explicitly out of scope for this feature and is recorded as a
  follow-up in TODO.md.
