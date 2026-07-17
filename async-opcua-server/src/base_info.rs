//! Base Info conformance-unit instantiation helpers: `OrderedListType`/`IOrderedObjectType`
//! (OPC-10000-5 §6.10/§6.11), `SelectionListType` (§7.18), `OptionSetType` (§7.17), `ValueAsText`
//! (OPC-10000-3, DataVariable InstanceDeclarations), `ReferenceDescriptionVariableType`/
//! `HasReferenceDescription` (OPC-10000-23 §5), and the `CurrencyUnit` property (OPC-10000-5
//! §12.2.12.2).
//!
//! These are SDK-*capability* helpers: closing the corresponding conformance units means
//! demonstrating this SDK can correctly instantiate and expose each structure (with a working,
//! tested example), matching this project's existing precedent for similar "supports
//! VariableType X" CUs.

use crate::address_space::{AddressSpace, ObjectBuilder, VariableBuilder};
use opcua_types::{
    CurrencyUnitType, DataTypeId, LocalizedText, NodeId, ObjectTypeId,
    ReferenceDescriptionDataType, ReferenceTypeId, VariableTypeId, Variant,
};

/// Creates an `OrderedListType` Object (OPC-10000-5 §6.10). Use [`add_ordered_object`] to
/// populate it with ordered children.
pub fn create_ordered_list_in_address_space(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
) -> NodeId {
    let list_id = NodeId::new(ns, format!("{name}_OrderedList"));
    ObjectBuilder::new(&list_id, name, name)
        .has_type_definition(ObjectTypeId::OrderedListType)
        .component_of(parent_id)
        .insert(address_space);
    list_id
}

/// Adds an ordered `<OrderedObject>` child to a list created by
/// [`create_ordered_list_in_address_space`]. The child is attached via `HasOrderedComponent`
/// (so browse order reflects list order), implements `IOrderedObjectType` via `HasInterface`
/// (OPC-10000-5 §6.11 — this also satisfies the "Address Space Interfaces" conformance unit),
/// and exposes a mandatory `NumberInList` property.
pub fn add_ordered_object(
    address_space: &AddressSpace,
    ns: u16,
    list_id: &NodeId,
    name: &str,
    number_in_list: i64,
) -> NodeId {
    let object_id = NodeId::new(ns, format!("{name}_OrderedObject"));
    ObjectBuilder::new(&object_id, name, name)
        .has_type_definition(ObjectTypeId::BaseObjectType)
        .insert(address_space);

    address_space.insert_reference(list_id, &object_id, ReferenceTypeId::HasOrderedComponent);
    address_space.insert_reference(
        &object_id,
        &NodeId::from(ObjectTypeId::IOrderedObjectType),
        ReferenceTypeId::HasInterface,
    );

    let number_in_list_id = NodeId::new(ns, format!("{name}_NumberInList"));
    VariableBuilder::new(&number_in_list_id, "NumberInList", "NumberInList")
        .data_type(DataTypeId::Int64)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(number_in_list)
        .writable()
        .property_of(object_id.clone())
        .insert(address_space);

    object_id
}

/// Creates a `SelectionListType` Variable (OPC-10000-5 §7.18). `selections` must be a
/// `Variant::Array` of `data_type`; `selection_descriptions`, if provided, must be a
/// `Variant::Array` of `LocalizedText` with the same length as `selections`.
#[allow(clippy::too_many_arguments)]
pub fn create_selection_list_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    initial_value: Variant,
    selections: Variant,
    selection_descriptions: Option<Variant>,
    restrict_to_list: Option<bool>,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_SelectionList"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(data_type.clone())
        .has_type_definition(VariableTypeId::SelectionListType)
        .value(initial_value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    let selections_id = NodeId::new(ns, format!("{name}_Selections"));
    VariableBuilder::new(&selections_id, "Selections", "Selections")
        .data_type(data_type)
        .value_rank(1)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(selections)
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    if let Some(descriptions) = selection_descriptions {
        let descriptions_id = NodeId::new(ns, format!("{name}_SelectionDescriptions"));
        VariableBuilder::new(
            &descriptions_id,
            "SelectionDescriptions",
            "SelectionDescriptions",
        )
        .data_type(DataTypeId::LocalizedText)
        .value_rank(1)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(descriptions)
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);
    }

    if let Some(restrict) = restrict_to_list {
        let restrict_id = NodeId::new(ns, format!("{name}_RestrictToList"));
        VariableBuilder::new(&restrict_id, "RestrictToList", "RestrictToList")
            .data_type(DataTypeId::Boolean)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(restrict)
            .writable()
            .property_of(var_id.clone())
            .insert(address_space);
    }

    var_id
}

/// Creates an `OptionSetType` Variable (OPC-10000-5 §7.17). `option_set_values` must be a
/// `Variant::Array` of `LocalizedText`, one entry per bit position (least-significant-bit
/// first). `bit_mask`, if provided, must be a `Variant::Array` of `Boolean` with the same
/// bit-position ordering.
#[allow(clippy::too_many_arguments)]
pub fn create_option_set_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    value: Variant,
    option_set_values: Variant,
    bit_mask: Option<Variant>,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_OptionSet"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(data_type)
        .has_type_definition(VariableTypeId::OptionSetType)
        .value(value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    let values_id = NodeId::new(ns, format!("{name}_OptionSetValues"));
    VariableBuilder::new(&values_id, "OptionSetValues", "OptionSetValues")
        .data_type(DataTypeId::LocalizedText)
        .value_rank(1)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(option_set_values)
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    if let Some(bit_mask) = bit_mask {
        let bit_mask_id = NodeId::new(ns, format!("{name}_BitMask"));
        VariableBuilder::new(&bit_mask_id, "BitMask", "BitMask")
            .data_type(DataTypeId::Boolean)
            .value_rank(1)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(bit_mask)
            .writable()
            .property_of(var_id.clone())
            .insert(address_space);
    }

    var_id
}

/// An enumerated DataVariable with a `ValueAsText` property (OPC-10000-3, DataVariable
/// InstanceDeclarations) kept in sync with its current Value via [`update_enum_value`].
#[derive(Debug, Clone)]
pub struct EnumVariableWithValueAsText {
    /// NodeId of the enumerated DataVariable's Value.
    pub value_id: NodeId,
    /// NodeId of the `ValueAsText` property.
    pub value_as_text_id: NodeId,
}

/// Creates an enumerated DataVariable with a `ValueAsText` property, initialized from
/// `enum_values` (a table of numeric value -> localized text) and `initial_value`.
pub fn create_enum_variable_with_value_as_text(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    data_type: NodeId,
    enum_values: &[(i64, LocalizedText)],
    initial_value: i64,
) -> EnumVariableWithValueAsText {
    let value_id = NodeId::new(ns, format!("{name}_Value"));
    VariableBuilder::new(&value_id, name, name)
        .data_type(data_type)
        .value(initial_value)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    let value_as_text_id = NodeId::new(ns, format!("{name}_ValueAsText"));
    let initial_text = enum_text(enum_values, initial_value);
    VariableBuilder::new(&value_as_text_id, "ValueAsText", "ValueAsText")
        .data_type(DataTypeId::LocalizedText)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(initial_text)
        .writable()
        .property_of(value_id.clone())
        .insert(address_space);

    EnumVariableWithValueAsText {
        value_id,
        value_as_text_id,
    }
}

/// Writes a new enum value to a Variable created by [`create_enum_variable_with_value_as_text`],
/// recomputing `ValueAsText` from `enum_values`. `ValueAsText` reads back null if `new_value`
/// has no matching entry.
pub fn update_enum_value(
    address_space: &AddressSpace,
    handle: &EnumVariableWithValueAsText,
    enum_values: &[(i64, LocalizedText)],
    new_value: i64,
) {
    set_variable_value(address_space, &handle.value_id, Variant::from(new_value));
    let text = enum_text(enum_values, new_value);
    set_variable_value(address_space, &handle.value_as_text_id, Variant::from(text));
}

fn enum_text(enum_values: &[(i64, LocalizedText)], value: i64) -> LocalizedText {
    enum_values
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, text)| text.clone())
        .unwrap_or_else(LocalizedText::null)
}

/// Attaches a `ReferenceDescriptionVariableType` instance (OPC-10000-23 §5) via
/// `HasReferenceDescription` from `described_source`, documenting the Reference
/// `described_source --described_reference_type--> described_target` (or its inverse, per
/// `is_forward`).
#[allow(clippy::too_many_arguments)]
pub fn attach_reference_description(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    described_source: &NodeId,
    described_reference_type: NodeId,
    is_forward: bool,
    described_target: opcua_types::ExpandedNodeId,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_ReferenceDescription"));
    let value = ReferenceDescriptionDataType {
        source_node: described_source.clone(),
        reference_type: described_reference_type,
        is_forward,
        target_node: described_target,
    };
    VariableBuilder::new(&var_id, name, name)
        .data_type(DataTypeId::ReferenceDescriptionDataType)
        .has_type_definition(VariableTypeId::ReferenceDescriptionVariableType)
        .value(opcua_types::ExtensionObject::from_message(value))
        .writable()
        .insert(address_space);

    address_space.insert_reference(
        described_source,
        &var_id,
        ReferenceTypeId::HasReferenceDescription,
    );

    var_id
}

/// Attaches a `CurrencyUnit` property (OPC-10000-5 §12.2.12.2) to a monetary DataVariable.
pub fn create_currency_variable(
    address_space: &AddressSpace,
    ns: u16,
    name: &str,
    parent_id: NodeId,
    amount: f64,
    currency: CurrencyUnitType,
) -> NodeId {
    let var_id = NodeId::new(ns, format!("{name}_Amount"));
    VariableBuilder::new(&var_id, name, name)
        .data_type(DataTypeId::Double)
        .value(amount)
        .writable()
        .component_of(parent_id)
        .insert(address_space);

    let currency_id = NodeId::new(ns, format!("{name}_CurrencyUnit"));
    VariableBuilder::new(&currency_id, "CurrencyUnit", "CurrencyUnit")
        .data_type(DataTypeId::CurrencyUnitType)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(opcua_types::ExtensionObject::from_message(currency))
        .writable()
        .property_of(var_id.clone())
        .insert(address_space);

    var_id
}

fn set_variable_value(address_space: &AddressSpace, node_id: &NodeId, value: Variant) {
    if let Some(mut node) = address_space.find_mut(node_id) {
        if let opcua_nodes::NodeType::Variable(ref mut var) = &mut *node {
            let _ = var.set_value(&opcua_types::NumericRange::None, value);
        }
    }
}
