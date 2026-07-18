# Feature Specification: GDS Directory Application-Registry Services

**Feature Branch**: `108-gds-directory-app-registry`
**Created**: 2026-07-18
**Status**: Draft
**Input**: User description: "GDS Directory application-registry services: implement DirectoryType's RegisterApplication/QueryApplications/FindApplications/UpdateApplication/UnregisterApplication/GetApplication/QueryServers methods, closing CUs 2232 and 3581."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Operator manages the registered-application inventory (Priority: P1)

A system integrator or Global Discovery Server operator wants to register OPC UA applications
(clients and servers) with the GDS so they can be discovered, looked up, updated, and removed
through the standard application-registry operations, enabling centralized fleet visibility
instead of manual, out-of-band tracking.

**Why this priority**: This is the entire scope of the feature -- every other behavior exists to
make this registry lifecycle (register, find, inspect, update, remove) correct and safe.

**Independent Test**: Register a new application, confirm it is immediately findable by a
general query and by its own identifier, update one of its details and confirm the change is
visible, then unregister it and confirm it no longer appears anywhere. This can be exercised
completely independent of any other GDS feature (certificate issuance, trust lists, etc.).

**Acceptance Scenarios**:

1. **Given** an authorized operator, **When** they register a new application with a unique
   identifying URI, **Then** the server assigns it a unique registry identifier and the
   application becomes findable.
2. **Given** an application already registered under a given URI, **When** a second registration
   is attempted under that same URI, **Then** the server rejects it as a duplicate rather than
   silently creating a second entry.
3. **Given** one or more registered applications, **When** an operator queries the registry
   (optionally filtered by name, URI, type, product, or capability), **Then** the server returns
   the matching records, correctly paging through results when there are more than fit in one
   response.
4. **Given** a specific known registry identifier, **When** an operator requests that
   application's full details, **Then** the server returns its current record.
5. **Given** a registered application's URI, **When** an operator looks it up specifically by
   that URI, **Then** the server returns the matching record without requiring the general
   filtered query.
6. **Given** a registered application whose details have changed (for example, a new discovery
   address), **When** an operator submits an update, **Then** subsequent lookups reflect the new
   information.
7. **Given** a registered application no longer needed, **When** an operator unregisters it,
   **Then** it stops appearing in any subsequent query or lookup.
8. **Given** a caller without administrative permission, **When** they attempt to register,
   update, or unregister an application, **Then** the server rejects the request.
9. **Given** a client still using the older, deprecated server-discovery lookup mechanism,
   **When** it queries through that legacy path, **Then** it still receives a usable, correct
   result drawn from the same registry.

---

### Edge Cases

- **Operating on an unknown registry identifier** (update, unregister, or detail lookup): the
  server returns a clear "not found" error rather than silently doing nothing.
- **A registry large enough that a single query can't return everything at once**: the server
  returns a partial result the caller can resume from, rather than truncating silently or
  failing.
- **Registering an application missing required identifying information** (for example, no
  URI): the server rejects the registration rather than storing an incomplete record.
- **Registering while already at capacity**: the server rejects new registrations with a clear
  resource-limit error rather than silently evicting existing entries.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST allow an authorized operator to register a new application by
  submitting its identifying details (name, URI, type, product URI, discovery addresses), and
  MUST return a unique registry identifier for it.
- **FR-002**: The system MUST reject a registration attempt whose URI duplicates an
  already-registered application, rather than creating a duplicate entry.
- **FR-003**: The system MUST allow querying the set of registered applications with optional
  filters (name, URI, type, product, capability) and MUST support paging through results larger
  than fit in a single response.
- **FR-004**: The system MUST allow retrieving a specific registered application's full details
  by its registry identifier.
- **FR-005**: The system MUST allow finding a registered application specifically by its own URI,
  independent of the general filtered query.
- **FR-006**: The system MUST allow updating a previously registered application's details.
- **FR-007**: The system MUST allow unregistering (removing) a previously registered
  application; once removed, it MUST NOT appear in any subsequent query or lookup.
- **FR-008**: The system MUST support the older, deprecated application-discovery mechanism for
  backward compatibility, returning results consistent with the current registry rather than a
  separately maintained data source.
- **FR-009**: The system MUST restrict registration, update, and removal to callers with
  administrative permission; read-only lookups remain available under this system's existing,
  broader permission model.
- **FR-010**: The system MUST reject an update, removal, or detail request for a registry
  identifier that does not exist, with a clear error rather than a silent no-op.
- **FR-011**: The system MUST reject a registration that is missing required identifying
  information.

### Key Entities

- **Registered Application**: one application's registry record -- its assigned identifier,
  name, URI, application type, product URI, discovery addresses, and capabilities.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator can register a new application and find it via a query within the same
  administrative session, with no server restart required.
- **SC-002**: Duplicate registration attempts for the same application URI never result in two
  entries for it.
- **SC-003**: A client relying on the older, deprecated discovery-query mechanism continues to
  receive usable, correct results after this feature ships, with no separate migration required.
- **SC-004**: An unregistered application never appears in any subsequent query or lookup result.

## Assumptions

- The deprecated legacy discovery-query mechanism is served by adapting the same underlying
  registry to its own, simpler output shape, rather than maintaining a second data source; if
  that adaptation turns out to lose information the legacy mechanism's own definition requires,
  that gap will be documented as a follow-up rather than blocking this feature or forcing an
  ill-fitting adaptation.
- The application registry is in-memory and scoped to one running server instance, consistent
  with this project's existing Global Discovery Server state; surviving a server restart is not
  required for this feature and is a possible future enhancement, not a gap this feature must
  close.
- The Authorization Service (OAuth2 token issuance), KeyCredential Service, JWT/OAuth2 authority
  discovery, and Local Discovery Server-Multicast-Extension connectivity are explicitly out of
  scope -- each needs substantially more new infrastructure than this feature builds, and is
  tracked separately.
- Removing a registered application does not revoke any certificates that were issued to it --
  doing so correctly needs an issuance ledger and certificate-revocation infrastructure this
  system does not yet have (the same gap already tracked for this system's optional certificate
  revocation capability). This is a known, documented limitation, not a silent gap.
- No administrative-change notification (audit event) is generated when an application is
  registered, updated, or removed, consistent with this system's current Global Discovery Server
  behavior more broadly (no administrative action in this area currently generates one either).
