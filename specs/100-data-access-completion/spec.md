# Feature Specification: Data Access Conformance Completion

**Feature Branch**: `100-data-access-completion`
**Created**: 2026-07-17
**Status**: Draft
**Input**: User description: "Data Access conformance completion: TwoStateDiscreteType, MultiStateDiscreteType, MultiStateValueDiscreteType, and the ArrayItemType family (YArrayItemType, XYArrayItemType, ImageItemType, CubeItemType, NDimensionArrayItemType) -- CUs 2361, 2426, 2831, 2988, 3323-3327."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Discrete-state DataAccess Variables (Priority: P1)

A server operator wants to model a piece of equipment that has a small,
named set of states — a two-position valve (open/closed), a multi-position
selector switch (open/close/in-transit), or an operating mode whose numeric
codes have gaps (e.g. 1, 4, 8) — using the standard OPC UA Data Access
VariableTypes clients already know how to interpret, instead of a bare
untyped Variable.

**Why this priority**: Covers the three DiscreteItemType subtypes
(TwoStateDiscreteType, MultiStateDiscreteType, MultiStateValueDiscreteType)
— the most commonly used and simplest Data Access types, and the
prerequisite for closing the abstract DiscreteItemType CU as a byproduct.

**Independent Test**: Instantiate one Variable of each of the three
subtypes with their spec-mandated Properties; read back the Properties and
the Value; for MultiStateValueDiscreteType, write a new value and confirm
`ValueAsText` updates to match (or reads back empty for an unmatched
value).

**Acceptance Scenarios**:

1. **Given** a `TwoStateDiscreteType` Variable configured with `TrueState`
   and `FalseState` text, **When** a client reads the Variable and its
   Properties, **Then** it sees the current boolean Value and both state
   texts.
2. **Given** a `MultiStateDiscreteType` Variable configured with a list of
   state names, **When** a client reads the Variable, **Then** it sees the
   current numeric Value and the full ordered list of state names via
   `EnumStrings`.
3. **Given** a `MultiStateValueDiscreteType` Variable configured with
   non-contiguous numeric states (e.g. 1, 4, 8), **When** a client reads
   the Variable, **Then** it sees the current Value, the `EnumValues`
   table, and a `ValueAsText` property that reflects the current Value's
   display text.
4. **Given** the same Variable, **When** its Value is changed to a number
   with no matching entry in `EnumValues`, **Then** `ValueAsText` reflects
   "no text available" rather than stale or incorrect text.

---

### User Story 2 - Array-shaped DataAccess Variables (Priority: P2)

A server operator wants to expose data whose *shape itself* is meaningful
— a spectrum, a list of labeled peaks, an image, a 3-D particle
distribution, or an arbitrary N-dimensional dataset — using the standard
Data Access ArrayItem VariableTypes, so generic OPC UA clients can render
them correctly (with axis, scale, and unit information) without
vendor-specific interpretation.

**Why this priority**: Covers the five ArrayItemType subtypes. Lower
priority than User Story 1 because these are less commonly needed in a
typical server, but they close out the remainder of the Data Access
conformance backlog.

**Independent Test**: Instantiate one Variable of each of the five
ArrayItem subtypes with their spec-mandated Properties (including the
correct number of axis-definition Properties per subtype: one for
YArrayItemType/XYArrayItemType, two for ImageItemType, three for
CubeItemType, and one per dimension for NDimensionArrayItemType); read
back the Value and Properties and confirm they match what was configured.

**Acceptance Scenarios**:

1. **Given** a `YArrayItemType` Variable, **When** a client reads it,
   **Then** it sees the numeric spectrum Value plus `EURange`,
   `EngineeringUnits`, `Title`, `AxisScaleType`, and `XAxisDefinition`.
2. **Given** an `XYArrayItemType` Variable, **When** a client reads it,
   **Then** it sees an array of (position, intensity) pairs plus the same
   base Properties and `XAxisDefinition`.
3. **Given** an `ImageItemType` Variable, **When** a client reads it,
   **Then** it sees a 2-D matrix Value plus both `XAxisDefinition` and
   `YAxisDefinition`.
4. **Given** a `CubeItemType` Variable, **When** a client reads it,
   **Then** it sees a 3-D Value plus `XAxisDefinition`, `YAxisDefinition`,
   and `ZAxisDefinition`.
5. **Given** an `NDimensionArrayItemType` Variable configured with N
   dimensions, **When** a client reads it, **Then** it sees the Value plus
   exactly N `AxisDefinition` entries, one per dimension.

---

### Edge Cases

- `DiscreteItemType` itself is abstract per its specification (no
  instances of the abstract type can exist) — the corresponding
  conformance unit is satisfied by having any concrete subtype instance
  (covered by User Story 1), not by attempting to instantiate the abstract
  type directly.
- A `MultiStateValueDiscreteType` Value with no matching `EnumValues`
  entry: `ValueAsText` must not show stale text from a previous value.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The SDK MUST provide a way to instantiate a
  `TwoStateDiscreteType` Variable with its mandatory `TrueState` and
  `FalseState` Properties.
- **FR-002**: The SDK MUST provide a way to instantiate a
  `MultiStateDiscreteType` Variable with its mandatory `EnumStrings`
  Property.
- **FR-003**: The SDK MUST provide a way to instantiate a
  `MultiStateValueDiscreteType` Variable with its mandatory `EnumValues`
  and `ValueAsText` Properties, and a way to update its Value such that
  `ValueAsText` is recomputed to match (or reflects no match).
- **FR-004**: The SDK MUST provide a way to instantiate each of
  `YArrayItemType`, `XYArrayItemType`, `ImageItemType`, `CubeItemType`, and
  `NDimensionArrayItemType`, each with its full set of mandatory
  Properties (the shared base set — `EURange`, `EngineeringUnits`,
  `Title`, `AxisScaleType` — plus the subtype-specific axis-definition
  Properties).
- **FR-005**: Every instantiated Variable's Value and Properties MUST be
  correctly readable by a client via the standard Read service.

### Key Entities

- **DiscreteItemType family**: TwoStateDiscreteType, MultiStateDiscreteType,
  MultiStateValueDiscreteType — Variables whose value is one of a small,
  named set of states.
- **ArrayItemType family**: YArrayItemType, XYArrayItemType, ImageItemType,
  CubeItemType, NDimensionArrayItemType — Variables whose value is a
  shaped array (1-D through N-D) with axis/scale/unit metadata.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All three DiscreteItemType-family CUs (2361, 2831, 2988) plus
  the abstract-type CU (2426) are marked `Implemented` in the project's
  conformance evidence ledger with file:line and test-name citations.
- **SC-002**: All five ArrayItemType-family CUs (3323-3327) are marked
  `Implemented` with the same evidence standard.
- **SC-003**: A read of any instantiated Variable of these eight types
  returns its Value and all mandatory Properties correctly in 100% of test
  runs.

## Assumptions

- CUs 2474 ("MultiStateDictionaryEntryDiscreteBaseType or a subtype") and
  2776 (its optional `ValueAsDictionaryEntries` Property) are explicitly
  OUT OF SCOPE for this feature. Investigation found this type is present
  in the generated core nodeset (from the current `Opc.Ua.NodeSet2.xml`
  schema snapshot) but is NOT documented in either the locally cached
  OPC-10000-8 v1.05.07 PDF or the OPC Foundation's public reference
  documentation site as of this writing — it appears to be a schema-only
  addition ahead of the corresponding spec-document text being published.
  Per this project's "Correctness Over Completion" principle, implementing
  against inferred-from-generated-code semantics rather than a verifiable
  specification text is deferred until authoritative documentation is
  available, rather than guessed at.
- Each VariableType is closed via a real, working SDK-capability example
  (an instantiation helper plus a test proving Value and Properties read
  back correctly), matching this project's established precedent for
  "supports VariableType X" conformance units (see feature 097, Base Info
  completion).
