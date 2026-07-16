# Feature Specification: Base Info Conformance Completion

**Feature Branch**: `097-base-info-completion`
**Created**: 2026-07-16
**Status**: Draft
**Input**: User description: "Base Info conformance completion: instantiate OrderedListType/IOrderedObjectType, SelectionListType, OptionSetType, ValueAsText, ReferenceDescriptionVariableType/HasReferenceDescription, CurrencyUnit property, and EstimatedReturnTime"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Ordered lists are addressable and interface-conformant (Priority: P1)

An application built on this SDK needs to expose a sequence of Objects
where order matters (e.g. process steps, a prioritized alarm list) in a
way any standard OPC UA client can discover and correctly order, without
relying on Browse response ordering (which clients may reorder, e.g.
alphabetically).

**Why this priority**: Largest structural lift in this cluster — closing
it also closes the unrelated "Address Space Interfaces" gap (CU 3560) as
a direct byproduct, since `OrderedListType` instances are required to
attach a `HasInterface` reference to `IOrderedObjectType` on each ordered
child.

**Independent Test**: Instantiate an `OrderedListType` Object with at
least 3 ordered child Objects; browse the `HasOrderedComponent`
references and confirm they resolve in the intended order; confirm each
child exposes `IOrderedObjectType`'s `NumberInList` property with unique,
monotonically meaningful values; confirm each child has a `HasInterface`
reference to `IOrderedObjectType`.

**Acceptance Scenarios**:

1. **Given** an `OrderedListType` instance with 3 child Objects added in a
   specific order, **When** a client reads each child's `NumberInList`
   property, **Then** the values are unique and reflect that order.
2. **Given** the same instance, **When** a client browses for
   `HasInterface` references from a child Object, **Then** it resolves to
   `IOrderedObjectType`.

---

### User Story 2 - Selection lists advertise valid values (Priority: P2)

An application exposes a Variable whose valid values come from a
dynamic set (e.g. available recipe names, connected device IDs) so a
client can discover what values are currently acceptable before writing,
rather than guessing and handling `Bad` write statuses.

**Why this priority**: Self-contained, single-VariableType instantiation;
independent of every other story here.

**Independent Test**: Instantiate a `SelectionListType` Variable with a
`Selections` array of 3 values, `SelectionDescriptions` describing each,
and `RestrictToList = true`; confirm all three properties read back
correctly and that the Selections/SelectionDescriptions arrays are the
same DataType/length.

**Acceptance Scenarios**:

1. **Given** a `SelectionListType` Variable with `RestrictToList = true`
   and 3 entries in `Selections`, **When** a client reads `Selections`,
   `SelectionDescriptions`, and `RestrictToList`, **Then** all three
   values match the configured set.

---

### User Story 3 - Option sets expose per-bit human-readable meaning (Priority: P3)

An application exposes a bitmask Variable (e.g. device status flags)
where a client needs to know what each individual bit means without
hardcoding a lookup table.

**Why this priority**: Self-contained, single-VariableType instantiation.

**Independent Test**: Instantiate an `OptionSetType` Variable with an
integer bitmask Value and an `OptionSetValues` array describing each bit;
confirm the array length matches the bit width and each entry's text is
readable.

**Acceptance Scenarios**:

1. **Given** an `OptionSetType` Variable with a configured bitmask value,
   **When** a client reads `OptionSetValues`, **Then** it gets one
   human-readable (or explicitly empty) `LocalizedText` per bit position.

---

### User Story 4 - Enumerated variables expose a human-readable text value (Priority: P4)

A client displaying an enumerated DataVariable's value to an operator
wants to subscribe to a ready-made text representation instead of
maintaining its own enum-value-to-text lookup.

**Why this priority**: Touches the most commonly-instantiated kind of
node (any enumerated DataVariable), but is otherwise a self-contained,
narrowly-scoped property.

**Independent Test**: Instantiate a DataVariable with an Enumeration
DataType and a `ValueAsText` property; write different valid enum values
to it and confirm `ValueAsText` updates to the matching localized text
each time.

**Acceptance Scenarios**:

1. **Given** an enumerated DataVariable with `ValueAsText` wired,
   **When** its Value changes to a different valid enum member, **Then**
   `ValueAsText` reads back the corresponding localized text.

---

### User Story 5 - References carry documented metadata (Priority: P5)

An engineer browsing an unusual or non-obvious Reference in the address
space (e.g. a custom cross-reference this SDK's application layer adds)
wants to discover *why* that reference exists without needing external
documentation.

**Why this priority**: The most specification-dense story (Part 23, a
companion part not covered by earlier grounding in this project); scoped
last given its narrower practical audience.

**Independent Test**: Attach a `ReferenceDescriptionVariableType`
instance via `HasReferenceDescription` to an existing Reference already
present in this server's address space; confirm its Value (a
`ReferenceDescriptionDataType`) accurately reports that reference's
SourceNode/ReferenceType/IsForward/TargetNode.

**Acceptance Scenarios**:

1. **Given** a Reference in the address space with an attached
   `ReferenceDescriptionVariableType` instance, **When** a client reads
   that instance's Value, **Then** the `SourceNode`/`ReferenceType`/
   `IsForward`/`TargetNode` fields match the actual described Reference.

---

### User Story 6 - Currency-valued variables self-describe their unit (Priority: P6)

An application exposing a monetary value (e.g. a price, a cost total)
wants a client to know which currency and how many decimal places apply,
without an out-of-band data dictionary.

**Why this priority**: Self-contained, single-property instantiation.

**Independent Test**: Instantiate a DataVariable representing a monetary
amount with a `CurrencyUnit` property populated with a real ISO 4217
currency's numeric code, exponent, alphabetic code, and display name;
confirm all four fields read back correctly.

**Acceptance Scenarios**:

1. **Given** a currency-valued DataVariable with its `CurrencyUnit`
   property set, **When** a client reads that property, **Then** all
   four `CurrencyUnitType` fields match a real, internally-consistent
   ISO 4217 currency definition.

---

### User Story 7 - Clients can learn when a degraded server will recover (Priority: P7)

An operator or client application observing a server in a non-Running
state (e.g. during a planned, graceful restart this application
triggers) wants to know approximately when to expect it back, so its
reconnect logic can wait rather than hammering the server with retries.

**Why this priority**: Narrowest audience (only matters during a
graceful-shutdown/restart flow this server already partially models);
last since it depends on understanding the existing shutdown-scheduling
mechanism first.

**Independent Test**: Trigger this server's existing graceful-shutdown
scheduling with a known estimated return time; confirm a client reading
`Server.EstimatedReturnTime` gets that value; confirm it is absent/null
when no shutdown is scheduled.

**Acceptance Scenarios**:

1. **Given** a graceful shutdown scheduled with an estimated return
   time, **When** a client reads `Server.EstimatedReturnTime`, **Then**
   it matches the scheduled value.
2. **Given** no shutdown scheduled, **When** a client reads
   `Server.EstimatedReturnTime`, **Then** it reads null.

---

### Edge Cases

- An `OrderedListType`'s child list must support being empty (no ordered
  children yet) without error.
- A `SelectionListType`'s `Selections` array must support being empty
  (no valid values currently) without error, since the spec explicitly
  allows this to represent "no values currently writable."
- Writing a `SelectionListType`-backed Variable to a value NOT in
  `Selections` while `RestrictToList = true` is out of scope for this
  feature (this feature closes the CU's own scope: exposing the
  properties correctly, not implementing write-time restriction
  enforcement) — noted as a follow-up, not blocking closure.
- `ValueAsText` must not be present (or must read null) for a numeric
  enum value that has no matching entry in the DataType's `EnumValues`/
  `EnumStrings` definition.
- `EstimatedReturnTime` must read back null/absent both before any
  shutdown is scheduled and after the server has fully returned to
  `Running`.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The SDK MUST support instantiating an `OrderedListType`
  Object with an arbitrary number of ordered child Objects, each exposing
  `IOrderedObjectType` (via `HasInterface`) and a unique `NumberInList`.
- **FR-002**: The SDK MUST support instantiating a `SelectionListType`
  Variable with `Selections`, optional `SelectionDescriptions`, and
  optional `RestrictToList`.
- **FR-003**: The SDK MUST support instantiating an `OptionSetType`
  Variable with `OptionSetValues` and optional `BitMask`.
- **FR-004**: The SDK MUST support attaching a `ValueAsText` property to
  an enumerated DataVariable, kept in sync with that Variable's current
  Value.
- **FR-005**: The SDK MUST support instantiating a
  `ReferenceDescriptionVariableType` Variable, attached via
  `HasReferenceDescription`, whose Value accurately describes an existing
  Reference in the address space.
- **FR-006**: The SDK MUST support attaching a `CurrencyUnit` property
  (of `CurrencyUnitType`) to a DataVariable representing a monetary
  value.
- **FR-007**: The server MUST expose `Server.EstimatedReturnTime`,
  settable when a graceful shutdown with a known return time is
  scheduled, and null otherwise.

### Key Entities

- **OrderedListType instance**: An Object exposing an ordered sequence of
  child Objects via `HasOrderedComponent`, each implementing
  `IOrderedObjectType`.
- **SelectionListType instance**: A Variable whose valid-value set is
  discoverable via its `Selections` property.
- **OptionSetType instance**: A Variable representing a bitmask with
  per-bit human-readable descriptions.
- **ReferenceDescriptionVariableType instance**: A Variable documenting a
  specific Reference elsewhere in the address space.
- **CurrencyUnit property**: A `CurrencyUnitType`-valued property
  attached to a monetary DataVariable.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001** through **SC-007**: each of the seven user stories above has
  a passing automated test proving its Independent Test criterion.
- **SC-008**: All seven target conformance units (2512, 2711, 3127, 2969,
  3996, 5240, 3198) — plus the incidentally-closed 3560 (Address Space
  Interfaces) — move to `implemented` in the project's conformance
  evidence register, each with a file:line/test citation.

## Assumptions

- These are SDK-*capability* conformance units: closing them means
  demonstrating the SDK can correctly instantiate and expose each
  structure (with a working example and test), not that every
  application built on this SDK automatically gets one for free —
  matching this project's existing precedent for similar "supports
  VariableType X" CUs (e.g. DataAccess types, custom-codegen).
- `ReferenceDescriptionVariableType`/`HasReferenceDescription`/
  `ReferenceDescriptionDataType`/`ReferenceListEntryDataType` are defined
  in OPC 10000-23 (Common Reference Types), not Part 3 or Part 5 —
  confirmed via the OPC Foundation's online reference (not present in
  this project's locally cached Part 3/5 PDFs) since these NodeIds
  already exist in the generated 1.05 nodeset this project imports.
- `EstimatedReturnTime` is wired into the server's *existing*
  `schedule_shutdown` mechanism (`server_status.rs`) rather than a new,
  separate scheduling concept, since that's the server's only existing
  notion of "known future state change."
