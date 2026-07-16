# Data Model: Base Info Conformance Completion

No new persistent storage. All entities below are address-space node
structures created by new helper functions in `base_info.rs`, using
existing generated types.

## OrderedListType instance (US1)

| Node | Type | Notes |
|---|---|---|
| List Object | `ObjectTypeId::OrderedListType` | Parent; optional `NodeVersion` String property |
| `<OrderedObject>` children (N) | `BaseObjectType`, `HasOrderedComponent` from the list | Order = reference order |
| Each child's `NumberInList` | Property, numeric, unique per list | Mandatory per `IOrderedObjectType` |
| Each child → `IOrderedObjectType` | `HasInterface` reference | Closes CU 3560 as a byproduct |

## SelectionListType instance (US2)

| Property | Type | Modelling |
|---|---|---|
| `Selections` | same DataType as the instance's Value | Mandatory |
| `SelectionDescriptions` | `LocalizedText[]`, same length as `Selections` | Optional |
| `RestrictToList` | `Boolean` | Optional |

## OptionSetType instance (US3)

| Property | Type | Modelling |
|---|---|---|
| Value | numeric or `ByteString` bitmask | — |
| `OptionSetValues` | `LocalizedText[]`, one per bit, LSB first | Mandatory |
| `BitMask` | `Boolean[]`, same bit ordering | Optional |

## ValueAsText attachment (US4)

| Property | Type | Notes |
|---|---|---|
| `ValueAsText` | `LocalizedText` | Recomputed whenever the enumerated Variable's Value changes; sourced from the DataType's `EnumValues`/`EnumStrings` definition |

## ReferenceDescriptionVariableType instance (US5)

| Node | Type | Notes |
|---|---|---|
| Value | `ReferenceDescriptionDataType` (`source_node`/`reference_type`/`is_forward`/`target_node`) | Scalar |
| `ReferenceRefinement` | `ReferenceListEntryDataType[]` | Optional |
| Attachment | `HasReferenceDescription` from the described Reference's source and/or target node | — |

## CurrencyUnit property (US6)

| Field | Type |
|---|---|
| `numeric_code` | `i16` (ISO 4217 numeric) |
| `exponent` | `i8` (decimal places) |
| `alphabetic_code` | `UAString` (e.g. "USD") |
| `currency` | `LocalizedText` (display name) |

Attached as a `CurrencyUnit` property (`CurrencyUnitType`-valued) on a
DataVariable representing a monetary amount.

## EstimatedReturnTime (US7)

Extends `ServerStatusWrapper`: a new optional `DateTime` field set
alongside `schedule_shutdown`'s existing `reason`/`deadline`, exposed at
`VariableId::Server_EstimatedReturnTime`, null when no shutdown is
scheduled. No new struct — a field addition to the existing
`ShutdownTarget`/read path.
