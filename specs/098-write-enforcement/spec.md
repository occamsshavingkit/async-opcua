# Feature Specification: Address Space Write Enforcement Completion

**Feature Branch**: `098-write-enforcement`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "Address Space Write enforcement completion: close the final 3 required CUs in the Attribute Write / Address Space conformance cluster for the Micro/Embedded/Standard 2025 server profiles (2820 WriteFullArrayOnly enforcement, 2936 StatusCode & Timestamp write test, 4237 NonVolatile/Constant test)."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Reject IndexRange writes to full-array-only Variables (Priority: P1)

A server operator marks a Variable as accepting only whole-array Writes (no
partial/IndexRange writes) by setting the `WriteFullArrayOnly` flag on its
`AccessLevelEx` attribute. A client that attempts to Write a sub-range of
that Variable's array value must be rejected, rather than silently allowed
to mutate part of the array.

**Why this priority**: This is the one item in the cluster with an actual
behavioral gap — the flag is already settable and readable, but nothing on
the Write path currently consults it, so today a client CAN write a partial
range even when the server declared it unsupported. This is a real
conformance/data-integrity gap, not just missing test evidence.

**Independent Test**: Create a Variable with an array value and
`WriteFullArrayOnly` set on `AccessLevelEx`. Attempt an IndexRange Write to
one element. Confirm the Write fails and the stored value is unchanged.
Confirm a full-array (no IndexRange) Write to the same Variable still
succeeds.

**Acceptance Scenarios**:

1. **Given** a Variable with an array value and `WriteFullArrayOnly` set,
   **When** a client Write specifies an IndexRange, **Then** the Write
   fails with a "write not supported" error and the Variable's value is
   unchanged.
2. **Given** the same Variable, **When** a client Write targets the whole
   value with no IndexRange, **Then** the Write succeeds and the new value
   is readable.
3. **Given** a Variable WITHOUT `WriteFullArrayOnly` set, **When** a client
   Write specifies an IndexRange, **Then** the Write succeeds as before
   (no regression to existing IndexRange write support).
4. **Given** a server's own internal logic updates a Variable's value
   directly (not via the client Write service — e.g. an alarm's source
   value sampler), **When** it uses an IndexRange, **Then** the update is
   unaffected by `WriteFullArrayOnly` (the flag only gates client Write
   requests, not internal server state updates).

---

### User Story 2 - Prove StatusCode and Timestamps round-trip through Write (Priority: P2)

A client Write can carry not just a new value but also an explicit
StatusCode and Source/Server Timestamps. The server must actually store and
later return those, not just the value payload.

**Why this priority**: This is evidence-closure, not a code gap — the
underlying storage already exists — but it is the difference between
"believed correct" and "proven correct" for a specific, previously-untested
combination (non-Good status + distinct timestamps).

**Independent Test**: Write a Variable's value with an explicit non-Good
StatusCode and distinct Source/Server Timestamps. Read the Variable back
with both timestamps requested. Confirm the value, StatusCode, and both
timestamps all match what was written.

**Acceptance Scenarios**:

1. **Given** a writable Variable, **When** a client writes a new value with
   StatusCode `Uncertain` (or another non-Good code) and explicit
   SourceTimestamp/ServerTimestamp, **Then** a subsequent Read of that
   Variable returns the same value, the same StatusCode, and the same
   timestamps.

---

### User Story 3 - Prove NonVolatile and Constant flags round-trip (Priority: P3)

A server operator marks a Variable's storage characteristics — whether its
value survives a restart (`NonVolatile`) and whether its value never
changes (`Constant`) — via the `AccessLevelEx` attribute. Clients need to
be able to read those flags back reliably.

**Why this priority**: Also evidence-closure — the generic bit-level
storage for `AccessLevelEx` already handles arbitrary bits including these
two, but no test exercises this specific pair.

**Independent Test**: Create a Variable with both `NonVolatile` and
`Constant` set on `AccessLevelEx`. Read the `AccessLevelEx` attribute back
and confirm both bits are present.

**Acceptance Scenarios**:

1. **Given** a Variable configured with `NonVolatile` and `Constant` both
   set on `AccessLevelEx`, **When** a client reads the `AccessLevelEx`
   attribute, **Then** both flags are present in the returned value.

---

### Edge Cases

- A Variable with `WriteFullArrayOnly` set but a scalar (non-array) value:
  IndexRange writes to scalars are already rejected for unrelated reasons
  (a scalar has no indexable range); `WriteFullArrayOnly` enforcement must
  not change the error returned in that already-covered case.
- A Write to a non-Value attribute (e.g. `Description`) with an IndexRange:
  already rejected independently of `WriteFullArrayOnly` (IndexRange is
  only meaningful for the `Value` attribute); this feature must not change
  that.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST reject a client Write to a Variable's `Value`
  attribute that specifies an IndexRange, when that Variable's
  `AccessLevelEx` attribute has the `WriteFullArrayOnly` flag set.
- **FR-002**: The server MUST continue to accept a full-value (no
  IndexRange) client Write to a Variable with `WriteFullArrayOnly` set.
- **FR-003**: The server MUST continue to accept IndexRange client Writes
  to Variables that do NOT have `WriteFullArrayOnly` set (no regression).
- **FR-004**: The `WriteFullArrayOnly` enforcement MUST apply only to
  client Write-service requests, not to a server's own internal
  programmatic updates of a Variable's value.
- **FR-005**: The server MUST store and return, on a subsequent Read, the
  exact StatusCode and Source/Server Timestamps supplied in a client Write
  (in addition to the value itself), for at least one non-Good StatusCode.
- **FR-006**: The server MUST allow the `NonVolatile` and `Constant` flags
  to be set on a Variable's `AccessLevelEx` attribute and returned
  correctly on a subsequent Read.

### Key Entities

- **AccessLevelEx**: An extended-access-rights bitmask attribute on a
  Variable Node, carrying (among other bits) `WriteFullArrayOnly`,
  `NonVolatile`, and `Constant`.
- **WriteValue**: A single Write-service request item, carrying a NodeId,
  AttributeId, optional IndexRange, and a DataValue (value + StatusCode +
  timestamps).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An IndexRange Write to a Variable with `WriteFullArrayOnly`
  set is rejected in 100% of cases, with the Variable's stored value
  unchanged.
- **SC-002**: A full-value Write to the same Variable succeeds in 100% of
  cases.
- **SC-003**: A Write carrying a non-Good StatusCode and explicit
  timestamps is followed by a Read that returns identical value, status,
  and timestamps in 100% of test runs.
- **SC-004**: A Read of `AccessLevelEx` after setting `NonVolatile` and
  `Constant` returns both flags correctly in 100% of test runs.
- **SC-005**: All three conformance units (2820, 2936, 4237) are marked
  `Implemented` in the project's conformance evidence ledger with
  file:line and test-name citations.

## Assumptions

- "Reject" for an unsupported IndexRange Write means the Write-service
  result for that item is a "write not supported" style failure status,
  consistent with how this server already rejects other IndexRange writes
  it cannot honor (e.g. IndexRange writes to non-Value attributes).
- The three CUs are grounded directly in the OPC UA Foundation's official
  conformance-unit descriptions (fetched from the local normalized profile
  snapshot) and in OPC-10000 Part 3 (Address Space Model) §8.58 /
  Part 4 (Services) §5.11.4, per this repository's established
  spec-grounding practice.
- This is a completion of the pre-existing "Attribute Write remaining gaps"
  TODO.md backlog entry; CU 3560 (Address Space Interfaces), the fourth
  item originally tracked alongside these three, was already closed as a
  byproduct of feature 097 and is out of scope here.
