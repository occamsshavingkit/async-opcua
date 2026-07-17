//! Data Access conformance-unit instantiation helpers: `TwoStateDiscreteType`,
//! `MultiStateDiscreteType`, `MultiStateValueDiscreteType` (OPC-10000-8 §5.3.3.2-5.3.3.4), and the
//! `ArrayItemType` family -- `YArrayItemType`, `XYArrayItemType`, `ImageItemType`, `CubeItemType`,
//! `NDimensionArrayItemType` (OPC-10000-8 §5.3.4.2-5.3.4.6).
//!
//! These are SDK-*capability* helpers: closing the corresponding conformance units means
//! demonstrating this SDK can correctly instantiate and expose each structure (with a working,
//! tested example), matching this project's existing precedent (see `base_info.rs`) for similar
//! "supports VariableType X" CUs. `DiscreteItemType` itself (CU 2426) is abstract per
//! OPC-10000-8 §5.3.3.1 ("no instances of this type can exist") and closes as a byproduct of any
//! of its concrete subtypes below.

use crate::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AxisInformation, AxisScaleEnumeration, DataTypeId, EUInformation, EnumValueType,
    ExtensionObject, LocalizedText, NodeId, Range, VariableTypeId, Variant,
};

/// Creates a `TwoStateDiscreteType` Variable (OPC-10000-8 §5.3.3.2), also closing CU 2426
/// (`DiscreteItemType` is abstract; any concrete subtype instance satisfies it).
pub fn create_two_state_discrete_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    value: bool,
    true_state: LocalizedText,
    false_state: LocalizedText,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_TwoStateDiscrete"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(DataTypeId::Boolean)
        .has_type_definition(VariableTypeId::TwoStateDiscreteType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    property(address_space, ns, name, "TrueState", &var_id, true_state);
    property(address_space, ns, name, "FalseState", &var_id, false_state);

    var_id
}

/// Creates a `MultiStateDiscreteType` Variable (OPC-10000-8 §5.3.3.3). `enum_strings` must be a
/// `Variant::Array` of `LocalizedText`, one entry per sequential numeric value (0, 1, 2, ...).
pub fn create_multi_state_discrete_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    value: u32,
    enum_strings: Variant,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_MultiStateDiscrete"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(DataTypeId::UInt32)
        .has_type_definition(VariableTypeId::MultiStateDiscreteType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    let strings_id = NodeId::new(ns, format!("{name}_EnumStrings"));
    VariableBuilder::new(&strings_id, "EnumStrings", "EnumStrings")
        .data_type(DataTypeId::LocalizedText)
        .value_rank(1)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(enum_strings)
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    var_id
}

/// A `MultiStateValueDiscreteType` instance and its `ValueAsText` property (kept in sync via
/// [`update_multi_state_value_discrete`]).
#[derive(Debug, Clone)]
pub struct MultiStateValueDiscreteHandle {
    /// NodeId of the Variable's Value.
    pub value_id: NodeId,
    /// NodeId of the `ValueAsText` property.
    pub value_as_text_id: NodeId,
}

/// Creates a `MultiStateValueDiscreteType` Variable (OPC-10000-8 §5.3.3.4). Unlike
/// `MultiStateDiscreteType`, `enum_values` need not be zero-based or contiguous.
pub fn create_multi_state_value_discrete_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    value: i64,
    enum_values: &[EnumValueType],
) -> MultiStateValueDiscreteHandle {
    let value_id = NodeId::new(ns, format!("{name}_MultiStateValueDiscrete"));
    VariableBuilder::new(&value_id, name, name)
        .data_type(DataTypeId::Int64)
        .has_type_definition(VariableTypeId::MultiStateValueDiscreteType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    let values_id = NodeId::new(ns, format!("{name}_EnumValues"));
    VariableBuilder::new(&values_id, "EnumValues", "EnumValues")
        .data_type(DataTypeId::EnumValueType)
        .value_rank(1)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(Variant::from(
            enum_values
                .iter()
                .cloned()
                .map(ExtensionObject::from_message)
                .collect::<Vec<_>>(),
        ))
        .writable()
        .property_of(value_id.clone())
        .insert(address_space);

    let value_as_text_id = NodeId::new(ns, format!("{name}_ValueAsText"));
    VariableBuilder::new(&value_as_text_id, "ValueAsText", "ValueAsText")
        .data_type(DataTypeId::LocalizedText)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(multi_state_value_text(enum_values, value))
        .writable()
        .property_of(value_id.clone())
        .insert(address_space);

    MultiStateValueDiscreteHandle {
        value_id,
        value_as_text_id,
    }
}

/// Writes a new value to a Variable created by [`create_multi_state_value_discrete_variable`],
/// recomputing `ValueAsText` from `enum_values` (null if `new_value` has no matching entry, per
/// OPC-10000-8 §5.3.3.4).
pub fn update_multi_state_value_discrete(
    address_space: &AddressSpace,
    handle: &MultiStateValueDiscreteHandle,
    enum_values: &[EnumValueType],
    new_value: i64,
) {
    set_variable_value(address_space, &handle.value_id, Variant::from(new_value));
    set_variable_value(
        address_space,
        &handle.value_as_text_id,
        multi_state_value_text(enum_values, new_value),
    );
}

fn multi_state_value_text(enum_values: &[EnumValueType], value: i64) -> Variant {
    enum_values
        .iter()
        .find(|entry| entry.value == value)
        .map(|entry| Variant::from(entry.display_name.clone()))
        .unwrap_or(Variant::Empty)
}

/// The four Properties every `ArrayItemType` subtype mandates (OPC-10000-8 §5.3.4.1):
/// `EURange`, `EngineeringUnits`, `Title`, `AxisScaleType`.
#[derive(Debug, Clone)]
pub struct ArrayItemBaseProperties {
    /// Value range likely to be obtained in normal operation.
    pub eu_range: Range,
    /// Engineering units of the ArrayItem's Value.
    pub engineering_units: EUInformation,
    /// User-readable title of the ArrayItem's Value.
    pub title: LocalizedText,
    /// Scale to use for the axis where the Value is displayed.
    pub axis_scale_type: AxisScaleEnumeration,
}

fn attach_array_item_base_properties(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    var_id: &NodeId,
    base: &ArrayItemBaseProperties,
) {
    let eu_range_id = NodeId::new(ns, format!("{name}_EURange"));
    VariableBuilder::new(&eu_range_id, "EURange", "EURange")
        .data_type(DataTypeId::Range)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(ExtensionObject::from_message(base.eu_range.clone()))
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    let eu_id = NodeId::new(ns, format!("{name}_EngineeringUnits"));
    VariableBuilder::new(&eu_id, "EngineeringUnits", "EngineeringUnits")
        .data_type(DataTypeId::EUInformation)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(ExtensionObject::from_message(
            base.engineering_units.clone(),
        ))
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    let title_id = NodeId::new(ns, format!("{name}_Title"));
    VariableBuilder::new(&title_id, "Title", "Title")
        .data_type(DataTypeId::LocalizedText)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(base.title.clone())
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    let axis_scale_id = NodeId::new(ns, format!("{name}_AxisScaleType"));
    VariableBuilder::new(&axis_scale_id, "AxisScaleType", "AxisScaleType")
        .data_type(DataTypeId::AxisScaleEnumeration)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(base.axis_scale_type as i32)
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);
}

fn axis_definition_property(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    property_name: &str,
    var_id: &NodeId,
    axis: AxisInformation,
) {
    let axis_id = NodeId::new(ns, format!("{name}_{property_name}"));
    VariableBuilder::new(&axis_id, property_name, property_name)
        .data_type(DataTypeId::AxisInformation)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(ExtensionObject::from_message(axis))
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);
}

/// Creates a `YArrayItemType` Variable (OPC-10000-8 §5.3.4.2): a 1-D array representing spectra
/// or distributions with constant X-axis intervals. `value` must be a `Variant::Array` of one of
/// the numeric types the spec restricts this VariableType to (SByte, Int16, Int32, Int64, Float,
/// Double, ComplexNumberType, DoubleComplexNumberType).
#[allow(clippy::too_many_arguments)]
pub fn create_y_array_item_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    value: Variant,
    base: ArrayItemBaseProperties,
    x_axis_definition: AxisInformation,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_YArrayItem"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(data_type)
        .value_rank(1)
        .array_dimensions(&[0])
        .has_type_definition(VariableTypeId::YArrayItemType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    attach_array_item_base_properties(address_space, ns, name, &var_id, &base);
    axis_definition_property(
        address_space,
        ns,
        name,
        "XAxisDefinition",
        &var_id,
        x_axis_definition,
    );

    var_id
}

/// Creates an `XYArrayItemType` Variable (OPC-10000-8 §5.3.4.3): a 1-D array of `XVType` values
/// (e.g. a list of peaks, where `x` is position and `value` is intensity).
pub fn create_xy_array_item_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    value: Variant,
    base: ArrayItemBaseProperties,
    x_axis_definition: AxisInformation,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_XYArrayItem"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(DataTypeId::XVType)
        .value_rank(1)
        .has_type_definition(VariableTypeId::XYArrayItemType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    attach_array_item_base_properties(address_space, ns, name, &var_id, &base);
    axis_definition_property(
        address_space,
        ns,
        name,
        "XAxisDefinition",
        &var_id,
        x_axis_definition,
    );

    var_id
}

/// Creates an `ImageItemType` Variable (OPC-10000-8 §5.3.4.4): a 2-D matrix (column = X, row =
/// Y) such as an image, where the value is pixel intensity.
#[allow(clippy::too_many_arguments)]
pub fn create_image_item_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    value: Variant,
    columns: u32,
    rows: u32,
    base: ArrayItemBaseProperties,
    x_axis_definition: AxisInformation,
    y_axis_definition: AxisInformation,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_ImageItem"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(data_type)
        .value_rank(2)
        .array_dimensions(&[columns, rows])
        .has_type_definition(VariableTypeId::ImageItemType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    attach_array_item_base_properties(address_space, ns, name, &var_id, &base);
    axis_definition_property(
        address_space,
        ns,
        name,
        "XAxisDefinition",
        &var_id,
        x_axis_definition,
    );
    axis_definition_property(
        address_space,
        ns,
        name,
        "YAxisDefinition",
        &var_id,
        y_axis_definition,
    );

    var_id
}

/// Creates a `CubeItemType` Variable (OPC-10000-8 §5.3.4.5): a 3-D cube (column = X, row = Y,
/// depth = Z) such as a spatial particle distribution.
#[allow(clippy::too_many_arguments)]
pub fn create_cube_item_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    value: Variant,
    dimensions: [u32; 3],
    base: ArrayItemBaseProperties,
    x_axis_definition: AxisInformation,
    y_axis_definition: AxisInformation,
    z_axis_definition: AxisInformation,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_CubeItem"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(data_type)
        .value_rank(3)
        .array_dimensions(&dimensions)
        .has_type_definition(VariableTypeId::CubeItemType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    attach_array_item_base_properties(address_space, ns, name, &var_id, &base);
    axis_definition_property(
        address_space,
        ns,
        name,
        "XAxisDefinition",
        &var_id,
        x_axis_definition,
    );
    axis_definition_property(
        address_space,
        ns,
        name,
        "YAxisDefinition",
        &var_id,
        y_axis_definition,
    );
    axis_definition_property(
        address_space,
        ns,
        name,
        "ZAxisDefinition",
        &var_id,
        z_axis_definition,
    );

    var_id
}

/// Creates an `NDimensionArrayItemType` Variable (OPC-10000-8 §5.3.4.6): a generic
/// multi-dimensional ArrayItem, with one `AxisInformation` entry per dimension.
#[allow(clippy::too_many_arguments)]
pub fn create_nd_dimension_array_item_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    value: Variant,
    dimensions: &[u32],
    base: ArrayItemBaseProperties,
    axis_definitions: Vec<AxisInformation>,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_NDimensionArrayItem"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(data_type)
        .value_rank(0)
        .array_dimensions(dimensions)
        .has_type_definition(VariableTypeId::NDimensionArrayItemType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    attach_array_item_base_properties(address_space, ns, name, &var_id, &base);

    let axis_id = NodeId::new(ns, format!("{name}_AxisDefinition"));
    VariableBuilder::new(&axis_id, "AxisDefinition", "AxisDefinition")
        .data_type(DataTypeId::AxisInformation)
        .value_rank(1)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(Variant::from(
            axis_definitions
                .into_iter()
                .map(ExtensionObject::from_message)
                .collect::<Vec<_>>(),
        ))
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    var_id
}

fn property(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    property_name: &str,
    parent_id: &NodeId,
    value: LocalizedText,
) {
    let property_id = NodeId::new(ns, format!("{name}_{property_name}"));
    VariableBuilder::new(&property_id, property_name, property_name)
        .data_type(DataTypeId::LocalizedText)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(value)
        .writable()
        .property_of(parent_id.clone())
        .insert(address_space);
}

fn set_variable_value(address_space: &AddressSpace, node_id: &NodeId, value: Variant) {
    if let Some(mut node) = address_space.find_mut(node_id) {
        if let opcua_nodes::NodeType::Variable(ref mut var) = &mut *node {
            let _ = var.set_value(&opcua_types::NumericRange::None, value);
        }
    }
}
