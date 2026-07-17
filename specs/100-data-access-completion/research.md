# Research: Data Access Conformance Completion

## Official CU descriptions (from the OPC UA Foundation normalized profile snapshot)

- 2361 "Data Access TwoState": Support TwoStateDiscreteType Variables with corresponding Properties.
- 2426 "Data Access DiscreteItemType": Support a subtype of DiscreteItemType.
- 2831 "Data Access MultiStateValueDiscrete": Support MultiStateValueDiscreteType Variables with corresponding Properties.
- 2988 "Data Access MultiState": Support MultiStateDiscreteType Variables with corresponding Properties.
- 3323-3327 "Data Access {Y,XY,Image,Cube,NDimension}Array{Item}Type": Provide Variables of the respective type or a subtype.

## OPC-10000-8 (Data Access) v1.05.07 grounding

### DiscreteItemType family (§5.3.3)

- **DiscreteItemType** (§5.3.3.1): `IsAbstract = True`. "No instances of this
  type can exist." → CU 2426 is satisfied by any concrete subtype
  instance; no separate instantiation is possible or required.
- **TwoStateDiscreteType** (§5.3.3.2): DataType `Boolean`. Mandatory
  Properties: `TrueState` (LocalizedText), `FalseState` (LocalizedText).
- **MultiStateDiscreteType** (§5.3.3.3): DataType `UInteger`. Mandatory
  Property: `EnumStrings` (LocalizedText[]) — "a string lookup table
  corresponding to sequential numeric values (0, 1, 2, etc.)".
- **MultiStateValueDiscreteType** (§5.3.3.4): DataType `Number`
  (restricted in practice to integer types up to 64 bits). Mandatory
  Properties: `EnumValues` (EnumValueType[]) and `ValueAsText`
  (LocalizedText) — "If the Value is scalar, the ValueAsText Property
  provides the localized text representation... If the Value is not
  scalar then ValueAsText should be Null." Unlike MultiStateDiscreteType,
  the numeric codes need not be contiguous or zero-based (e.g. 1, 4, 8).

### ArrayItemType family (§5.3.4)

- **ArrayItemType** (§5.3.4.1): `IsAbstract = True`, `ValueRank = 0`
  (OneOrMoreDimensions). Mandatory Properties shared by every subtype:
  `EURange` (Range), `EngineeringUnits` (EUInformation), `Title`
  (LocalizedText), `AxisScaleType` (AxisScaleEnumeration).
  `InstrumentRange` is Optional (not implemented here, matching the
  spec's own optionality).
- **YArrayItemType** (§5.3.4.2): `ValueRank = 1`, `ArrayDimensions = {0}`
  (unknown size). Adds mandatory `XAxisDefinition` (AxisInformation).
  DataType restricted to SByte/Int16/Int32/Int64/Float/Double/
  ComplexNumberType/DoubleComplexNumberType.
- **XYArrayItemType** (§5.3.4.3): `ValueRank = 1`, DataType fixed to
  `XVType`. Adds mandatory `XAxisDefinition`.
- **ImageItemType** (§5.3.4.4): `ValueRank = 2`. Adds mandatory
  `XAxisDefinition` and `YAxisDefinition`. "The ArrayDimensions Attribute
  ... shall use the first entry ([0]) to define the number of columns and
  the second entry ([1]) to define the number of rows."
- **CubeItemType** (§5.3.4.5): `ValueRank = 3`. Adds `XAxisDefinition`,
  `YAxisDefinition`, `ZAxisDefinition`.
- **NDimensionArrayItemType** (§5.3.4.6): `ValueRank = 0`
  (OneOrMoreDimensions, generic). Adds mandatory `AxisDefinition`
  (`AxisInformation[]`) — one entry per dimension, "holds the information
  about the EngineeringUnits and Range for all axis."

## CU 2474 / 2776 — explicitly deferred

`MultiStateDictionaryEntryDiscreteBaseType` (CU 2474) and its optional
`ValueAsDictionaryEntries` Property (CU 2776) were investigated but NOT
implemented:

- Not present anywhere in the locally cached OPC-10000-8 v1.05.07 PDF text
  (searched exhaustively — the §5.3.3 subsection list stops at
  MultiStateValueDiscreteType).
- Not present on the OPC Foundation's public reference documentation site
  (`reference.opcfoundation.org`) either, confirmed by direct query.
- IS present in the generated core nodeset
  (`async-opcua-core-namespace/src/generated/nodeset_51.rs`, NodeId
  `ns=0;i=19077`, generated from the current `schemas/1.05/Opc.Ua.NodeSet2.xml`
  snapshot this project's codegen consumes) with two child Properties —
  `EnumDictionaryEntries` (`ns=0;i=19082`, 2-D array) and
  `ValueAsDictionaryEntries` (`ns=0;i=19083`, 1-D array, ModellingRule
  Optional).

This is a genuine "schema ahead of published spec text" situation: the
type is real (it ships in the standard nodeset XML this project imports)
but its semantics are not documented anywhere this project can verify
against. Per the "Correctness Over Completion" constitution principle,
guessing at an implementation from NodeId cross-references alone (without
being able to confirm, e.g., the exact expected structure of
`EnumDictionaryEntries`'s entries) is not acceptable. This is documented
as an explicit, reasoned deferral rather than a silent gap.

## Real implementation finding: `Variant::Array.dimensions` vs. node `ArrayDimensions`

`Variable::is_valid()` (`async-opcua-nodes/src/variable.rs`) validates a
Variable's stored Value against its `ValueRank`/`ArrayDimensions`
differently depending on `ValueRank`:

- For `ValueRank >= 1` (YArrayItemType, XYArrayItemType, ImageItemType,
  CubeItemType — all have a fixed, non-zero ValueRank), only the node's
  `ArrayDimensions` *attribute* length is checked against `ValueRank`. A
  flat `Variant::Array` (with `dimensions: None`) is valid regardless of
  its own shape.
- For `ValueRank == 0` (NDimensionArrayItemType's "OneOrMoreDimensions"),
  the check instead compares `ArrayDimensions.len()` against the *Variant
  value's own* `dimensions` field length (defaulting to 1 if the Variant
  has none) — meaning a flat array Value with a multi-entry
  `ArrayDimensions` attribute is rejected as invalid.

**Decision**: `create_nd_dimension_array_item_variable`'s test constructs
its Value via `Array::new_multi(scalar, values, dimensions)` (setting the
Variant's own multi-dimensional shape) rather than the flat
`Array::new(scalar, values)` used for the other four ArrayItem subtypes.
This is purely a test/example-construction detail — the production helper
function itself accepts a pre-built `Variant`, so it imposes no
constraint the caller couldn't already have satisfied correctly.
