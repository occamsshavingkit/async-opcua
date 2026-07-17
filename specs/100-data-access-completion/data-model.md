# Data Model: Data Access Conformance Completion

No new entities are introduced. This feature adds instantiation helpers
against existing generated types:

## `ArrayItemBaseProperties` (new, `data_access.rs`)

The four Properties every `ArrayItemType` subtype mandates (OPC-10000-8
§5.3.4.1), bundled to avoid repeating the same four parameters across five
near-identical functions:

```rust
pub struct ArrayItemBaseProperties {
    pub eu_range: Range,
    pub engineering_units: EUInformation,
    pub title: LocalizedText,
    pub axis_scale_type: AxisScaleEnumeration,
}
```

## `MultiStateValueDiscreteHandle` (new, `data_access.rs`)

```rust
pub struct MultiStateValueDiscreteHandle {
    pub value_id: NodeId,
    pub value_as_text_id: NodeId,
}
```

Returned by `create_multi_state_value_discrete_variable`; passed to
`update_multi_state_value_discrete` to keep `ValueAsText` in sync on
subsequent writes, mirroring `base_info.rs`'s
`EnumVariableWithValueAsText`/`update_enum_value` pair.

## Existing generated types used (no changes)

- `opcua_types::Range` — `{ low: f64, high: f64 }`
- `opcua_types::EUInformation` — `{ namespace_uri, unit_id, display_name, description }`
- `opcua_types::AxisInformation` — `{ engineering_units, eu_range, title, axis_scale_type, axis_steps }`
- `opcua_types::AxisScaleEnumeration` — `Linear | Log | Ln`
- `opcua_types::EnumValueType` — `{ value: i64, display_name, description }`
- `opcua_types::XVType` — `{ x: f64, value: f32 }`

## Instantiation function signatures (`data_access.rs`)

```rust
create_two_state_discrete_variable(address_space, ns, name, parent_id, value: bool, true_state: LocalizedText, false_state: LocalizedText) -> NodeId
create_multi_state_discrete_variable(address_space, ns, name, parent_id, value: u32, enum_strings: Variant) -> NodeId
create_multi_state_value_discrete_variable(address_space, ns, name, parent_id, value: i64, enum_values: &[EnumValueType]) -> MultiStateValueDiscreteHandle
update_multi_state_value_discrete(address_space, handle, enum_values, new_value: i64)

create_y_array_item_variable(address_space, ns, name, parent_id, data_type, value: Variant, base: ArrayItemBaseProperties, x_axis_definition: AxisInformation) -> NodeId
create_xy_array_item_variable(address_space, ns, name, parent_id, value: Variant, base, x_axis_definition) -> NodeId
create_image_item_variable(address_space, ns, name, parent_id, data_type, value: Variant, columns, rows, base, x_axis_definition, y_axis_definition) -> NodeId
create_cube_item_variable(address_space, ns, name, parent_id, data_type, value: Variant, dimensions: [u32; 3], base, x_axis_definition, y_axis_definition, z_axis_definition) -> NodeId
create_nd_dimension_array_item_variable(address_space, ns, name, parent_id, data_type, value: Variant, dimensions: &[u32], base, axis_definitions: Vec<AxisInformation>) -> NodeId
```
