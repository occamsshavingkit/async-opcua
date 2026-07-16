# Phase 0 Research: Base Info Conformance Completion

Grounded against the local OPC-10000-3 (Address Space Model) and
OPC-10000-5 (Information Model) v1.05.06 PDFs, except CU 3996 (see its
own section below).

## Real finding during implementation: this address space cannot guarantee
Browse-order for multiple same-type references from one source

`async-opcua-nodes::References` stores references as `by_source: HashMap<NodeId,
HashSet<Reference>>` — a `HashSet`, not an order-preserving collection.
Browsing `HasOrderedComponent` from an `OrderedListType` instance can
therefore return its `<OrderedObject>` children in an arbitrary
(hash-based) order, not insertion order. This is not a bug to fix for
this feature: OPC-10000-5 §6.11 itself anticipates exactly this ("not all
Clients consider the order returned by the Browse Service ... each
`<OrderedObject>` shall implement the IOrderedObjectType Interface,
providing the order by a specific Property") — `NumberInList` is the
spec's own answer to unreliable Browse order, not a redundant convenience.
The implementation and its test rely on `NumberInList` as the
authoritative order signal; the test only asserts Browse returns the
correct *set* of children, not a particular order.

## CU 2512 — OrderedListType / IOrderedObjectType (Part 5 §6.10/§6.11)

`OrderedListType` (subtype of `BaseObjectType`) has `<OrderedObject>`
children attached via `HasOrderedComponent` (Optional-Placeholder,
0..*), an Optional `NodeVersion` String property, and generates
`GeneralModelChangeEventType`. Each `<OrderedObject>` **shall** implement
`IOrderedObjectType` (an abstract Interface subtype of
`BaseInterfaceType`) via `HasInterface`, which mandates a `NumberInList`
property (`Number`, i.e. any numeric DataType). Order is defined by the
`HasOrderedComponent` reference order, not by `NumberInList` sort order
directly, though the two must stay consistent; no two children may share
a `NumberInList` value.

**Byproduct**: attaching `HasInterface` to `IOrderedObjectType` on each
`<OrderedObject>` is exactly what CU 3560 (Address Space Interfaces)
requires ("no server code creates `HasInterface` refs") — closes both
CUs in one implementation.

## CU 2711 — SelectionListType (Part 5 §7.18)

Subtype of `BaseDataVariableType`. Mandatory `Selections` property
(array, same DataType as the instance's own Value DataType — not fixed
to `BaseDataType` at the instance level). Optional
`SelectionDescriptions` (`LocalizedText[]`, same length as `Selections`)
and `RestrictToList` (`Boolean`, default false meaning "not restricted").
`Selections` may legitimately be empty (no currently-valid values).

## CU 3127 — OptionSetType (Part 5 §7.17)

Subtype of `BaseDataVariableType`, `ValueRank = Scalar`, `DataType` must
be capable of representing a bitmask (numeric integer or `ByteString`,
e.g. `BitFieldMaskDataType`). Mandatory `OptionSetValues`
(`LocalizedText[]`, one entry per bit position, least-significant-bit
first; empty `LocalizedText` for a bit with no defined meaning). Optional
`BitMask` (`Boolean[]`, same bit-position ordering, for per-bit
subscription).

## CU 2969 — ValueAsText (Part 3 §5.x, DataVariable InstanceDeclarations)

Optional `LocalizedText` property on any DataVariable "with a finite set
of LocalizedTexts associated with its value" (the spec's own example:
"any DataVariables having an Enumeration DataType"). Provides the
localized text for the Variable's *current* Value, so clients displaying
text can subscribe to this Property instead of maintaining their own
enum-to-text lookup. Must be kept in sync with Value.

## CU 3996 — ReferenceDescriptionVariableType / HasReferenceDescription
(OPC-10000-23, Common Reference Types — NOT Part 3/5)

Not found in this project's locally cached Part 3/5 v1.05.06 PDFs;
confirmed via the OPC Foundation's online reference
(reference.opcfoundation.org/Core/Part23/v105/docs/5) that this concept
lives in Part 23, a companion part not previously cached locally. The
corresponding generated NodeIds (`ReferenceDescriptionDataType`=32659,
`ReferenceListEntryDataType`=32660, `HasReferenceDescription`=32679, ...)
already exist in this project's imported 1.05 nodeset, confirming the
feature is genuinely available, just undocumented locally.

- `ReferenceDescriptionDataType` (mandatory fields): `SourceNode` (NodeId),
  `ReferenceType` (NodeId), `IsForward` (Boolean), `TargetNode`
  (ExpandedNodeId).
- `ReferenceListEntryDataType`: same as above minus `SourceNode` (used
  for refinement-path entries, to avoid repeating the source).
- `ReferenceDescriptionVariableType`: subtype of `BaseDataVariableType`,
  Value is a scalar `ReferenceDescriptionDataType`; optional
  `ReferenceRefinement` property (`ReferenceListEntryDataType[]`).
- `HasReferenceDescription`: concrete (non-abstract) subtype of
  `HasChild`, asymmetric, `InverseName` = "ReferenceDescriptionOf".
  Source = the node associated with the described Reference (its
  SourceNode and/or TargetNode); Target = the
  `ReferenceDescriptionVariableType` instance.

## CU 5240 — CurrencyUnit Property (Part 5 §12.2.12.2)

`CurrencyUnitType` is a *Structure DataType* (not a VariableType), already
generated in this project (`async-opcua-types::CurrencyUnitType`) with
fields `numeric_code: i16`, `exponent: i8`, `alphabetic_code: UAString`,
`currency: LocalizedText` — an exact match for ISO 4217 (numeric code,
decimal exponent, alphabetic code e.g. "USD", display name). The CU
requires a `CurrencyUnit` *Property* of this DataType attached to
DataVariables representing currency, not a new structure to invent.

## CU 3198 — EstimatedReturnTime (Part 5, ServerType, Table ~"ServerType
definition")

Optional `DateTime` Property directly on `ServerType`/the `Server`
Object. "Indicates the time at which the Server is expected to have a
ServerStatus.State of RUNNING_0. A Client that observes a shutdown or a
ServiceLevel of 0 should either wait until after this time to attempt to
reconnect... or enter into slow retry logic."

This server already has a real, matching mechanism:
`ServerStatusWrapper::schedule_shutdown(reason: LocalizedText, deadline:
Instant)` in `server_status.rs`, which drives
`ServerStatusDataType.seconds_till_shutdown`/`.shutdown_reason`. Extending
this (rather than inventing a second, parallel shutdown-scheduling
concept) with an optional estimated-return `DateTime` is the correct,
minimal way to close this CU — matches Constitution Principle II ("root
causes, not symptoms" / reuse existing mechanisms).

## Alternatives considered

- For CU 2969 (ValueAsText): computing it generically for *every*
  enumerated DataVariable server-wide (a cross-cutting hook in the
  attribute-read path) was considered, but rejected as unnecessary scope
  — the CU requires the SDK to *support* exposing this property when an
  application wants it, not that every enum variable gets one
  automatically; a demonstrated, tested instantiation helper closes the
  CU per this project's existing "supports VariableType X" precedent
  (see spec.md Assumptions).
- For CU 3198: a brand-new `EstimatedReturnTime`-specific scheduling
  API, independent of `schedule_shutdown` — rejected per Constitution
  Principle II; the existing mechanism already models "the server knows
  it's going away and roughly when," which is exactly what this Property
  reports.
