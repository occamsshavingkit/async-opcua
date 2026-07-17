# Data Model: Address Space Write Enforcement Completion

No new entities are introduced. This feature adds a validation check and
tests against existing entities:

## AccessLevelExType (existing, `async-opcua-types`)

Bitmask attribute type, `Byte`/`OptionSet`-backed. Relevant bits for this
feature (Part 3 §8.58, Table 42):

| Bit | Name               | Meaning when set (1)                                   |
|-----|--------------------|----------------------------------------------------------|
| 10  | WriteFullArrayOnly | Write of IndexRange is NOT supported for this Variable   |
| 12  | NonVolatile        | Variable's value is non-volatile (survives restart)      |
| 13  | Constant           | Variable's value is constant (never changes)              |

## Variable (existing, `async-opcua-nodes`)

No field changes. Existing fields/methods used:

- `access_level_ex_extended: u32` (bits 8..31, stored shifted down by 8)
- `access_level_ex() -> u32` — full AccessLevelEx value (low byte mirrors
  `AccessLevel`, high bits from `access_level_ex_extended`)
- `set_access_level_ex(AccessLevelExType)` — builder-time setter
- `set_value_range(...)` — the underlying range-write primitive (untouched
  by this feature; the new check happens upstream of this call on the
  Write-service path only)

## ParsedWriteValue (existing, `async-opcua-server::node_manager::attributes`)

No field changes. Existing fields used:

- `attribute_id: AttributeId`
- `index_range: NumericRange` (checked via `.has_range()`)
- `value: DataValue` (value, status, source/server timestamps)

## Validation flow (new logic, no new types)

```text
validate_node_write_inner(...)
  └── AttributeId::Value, NodeType::Variable(var)
        has_index_range = node_to_write.index_range.has_range()
        ── NEW ── if has_index_range
                     && var.access_level_ex() & WriteFullArrayOnly.bits() != 0
                  → Err(BadWriteNotSupported)     [CU 2820]
        (existing enum / data-type / EURange validation continues as before)
```
