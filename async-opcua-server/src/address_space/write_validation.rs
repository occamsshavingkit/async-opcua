use std::collections::HashSet;

use crate::node_manager::{ParsedWriteValue, RequestContext};
use opcua_nodes::{NodeBase, TypeTree};
use opcua_types::{
    AccessLevelExType, AttributeId, BrowseDirection, DataEncoding, DataTypeDefinition, DataTypeId,
    NodeId, NumericRange, QualifiedName, Range, ReferenceTypeId, StatusCode, TimestampsToReturn,
    Variant, VariantScalarTypeId, VariantTypeId,
};

use super::{
    utils::{
        is_writable, remember_localized_text_attribute_value,
        validate_localized_text_attribute_write_locale,
    },
    AddressSpace, HasNodeId, NodeType, Variable,
};

/// Validate `value`, verifying that it can be written as the value of
/// `variable`.
pub fn validate_value_to_write(
    variable: &Variable,
    value: &Variant,
    type_tree: &dyn TypeTree,
    has_index_range: bool,
) -> Result<(), StatusCode> {
    let node_data_type = variable.data_type();

    validate_value_data_type_to_write(
        &node_data_type,
        variable.value_rank(),
        value,
        type_tree,
        has_index_range,
    )
}

// Part 4 §5.11.4 / Part 3 §5.6: the written value's array-ness must match the node's ValueRank.
// Skipped when an index range is present, since that writes a sub-section (e.g. a scalar element
// of an array) rather than replacing the whole value.
fn validate_value_rank_to_write(
    value_rank: i32,
    is_array: bool,
    has_index_range: bool,
) -> Result<(), StatusCode> {
    if has_index_range {
        return Ok(());
    }
    let ok = match value_rank {
        // ScalarOrOneDimension / Any: either a scalar or an array is acceptable.
        -3 | -2 => true,
        // Scalar: the value must not be an array.
        -1 => !is_array,
        // 0 (OneOrMoreDimensions), 1, 2, ...: an array is required.
        _ => is_array,
    };
    if ok {
        Ok(())
    } else {
        Err(StatusCode::BadTypeMismatch)
    }
}

fn validate_value_data_type_to_write(
    node_data_type: &NodeId,
    value_rank: i32,
    value: &Variant,
    type_tree: &dyn TypeTree,
    has_index_range: bool,
) -> Result<(), StatusCode> {
    if matches!(value, Variant::Empty) {
        return Ok(());
    }

    if let Some(value_data_type) = value.data_type() {
        let Some(data_type) = value_data_type.try_resolve(type_tree.namespaces()) else {
            return Err(StatusCode::BadTypeMismatch);
        };
        // Value is scalar, check if the data type matches
        let data_type_matches = type_tree.is_subtype_of(&data_type, node_data_type);

        if !data_type_matches {
            if value.is_array() {
                return Err(StatusCode::BadTypeMismatch);
            }
            // Check if the value to write is a byte string and the receiving node type a byte array.
            // This code is a mess just for some weird edge case in the spec that a write from
            // a byte string to a byte array should succeed
            match value {
                Variant::ByteString(_) => {
                    if node_data_type
                        .as_data_type_id()
                        .is_ok_and(|data_type| data_type == DataTypeId::Byte)
                    {
                        match value_rank {
                            -2 | -3 | 1 => Ok(()),
                            _ => Err(StatusCode::BadTypeMismatch),
                        }
                    } else {
                        Err(StatusCode::BadTypeMismatch)
                    }
                }
                // Part 4 §5.11.4: a scalar value whose data type is neither the node's
                // data type nor a subtype of it is a type mismatch.
                _ => Err(StatusCode::BadTypeMismatch),
            }
        } else {
            // Data type matches; the value's array-ness must also match the node's ValueRank.
            validate_value_rank_to_write(value_rank, value.is_array(), has_index_range)
        }
    } else {
        Err(StatusCode::BadTypeMismatch)
    }
}

pub(super) fn validate_attribute_value_to_write(
    attribute_id: AttributeId,
    value: &Variant,
) -> Result<(), StatusCode> {
    let valid = match attribute_id {
        AttributeId::Value => true,
        AttributeId::NodeId | AttributeId::DataType => {
            is_scalar_attribute_value(value, VariantScalarTypeId::NodeId)
        }
        AttributeId::NodeClass | AttributeId::ValueRank => {
            is_scalar_attribute_value(value, VariantScalarTypeId::Int32)
        }
        AttributeId::BrowseName => {
            is_scalar_attribute_value(value, VariantScalarTypeId::QualifiedName)
        }
        AttributeId::DisplayName | AttributeId::Description | AttributeId::InverseName => {
            is_scalar_attribute_value(value, VariantScalarTypeId::LocalizedText)
        }
        AttributeId::WriteMask | AttributeId::UserWriteMask | AttributeId::AccessLevelEx => {
            is_scalar_attribute_value(value, VariantScalarTypeId::UInt32)
        }
        AttributeId::IsAbstract
        | AttributeId::Symmetric
        | AttributeId::ContainsNoLoops
        | AttributeId::Historizing
        | AttributeId::Executable
        | AttributeId::UserExecutable => {
            is_scalar_attribute_value(value, VariantScalarTypeId::Boolean)
        }
        AttributeId::EventNotifier | AttributeId::AccessLevel | AttributeId::UserAccessLevel => {
            is_scalar_attribute_value(value, VariantScalarTypeId::Byte)
        }
        AttributeId::ArrayDimensions => {
            is_array_attribute_value(value, VariantScalarTypeId::UInt32)
        }
        AttributeId::MinimumSamplingInterval => {
            is_scalar_attribute_value(value, VariantScalarTypeId::Double)
        }
        AttributeId::DataTypeDefinition => {
            value.is_empty()
                || is_scalar_attribute_value(value, VariantScalarTypeId::ExtensionObject)
        }
        AttributeId::RolePermissions | AttributeId::UserRolePermissions => {
            is_array_attribute_value(value, VariantScalarTypeId::ExtensionObject)
        }
        AttributeId::AccessRestrictions => {
            is_scalar_attribute_value(value, VariantScalarTypeId::UInt16)
        }
    };

    if valid {
        Ok(())
    } else {
        Err(StatusCode::BadTypeMismatch)
    }
}

fn is_scalar_attribute_value(value: &Variant, expected: VariantScalarTypeId) -> bool {
    value.type_id() == VariantTypeId::Scalar(expected)
}

fn is_array_attribute_value(value: &Variant, expected: VariantScalarTypeId) -> bool {
    matches!(value.type_id(), VariantTypeId::Array(actual, _) if actual == expected)
}

/// Validate that the user given by `context` can write to the attribute given
/// by `node_to_write` on `node`.
pub fn validate_node_write(
    node: &NodeType,
    context: &RequestContext,
    node_to_write: &ParsedWriteValue,
    type_tree: &dyn TypeTree,
) -> Result<(), StatusCode> {
    validate_node_write_inner(None, node, context, node_to_write, type_tree)
}

pub(crate) fn validate_node_write_in_address_space(
    address_space: &AddressSpace,
    node: &NodeType,
    context: &RequestContext,
    node_to_write: &ParsedWriteValue,
    type_tree: &dyn TypeTree,
) -> Result<(), StatusCode> {
    validate_node_write_inner(Some(address_space), node, context, node_to_write, type_tree)
}

fn validate_node_write_inner(
    address_space: Option<&AddressSpace>,
    node: &NodeType,
    context: &RequestContext,
    node_to_write: &ParsedWriteValue,
    type_tree: &dyn TypeTree,
) -> Result<(), StatusCode> {
    is_writable(context, node, node_to_write.attribute_id)?;

    if node_to_write.attribute_id != AttributeId::Value && node_to_write.index_range.has_range() {
        return Err(StatusCode::BadWriteNotSupported);
    }

    let Some(value) = node_to_write.value.value.as_ref() else {
        return Err(StatusCode::BadTypeMismatch);
    };

    validate_attribute_value_to_write(node_to_write.attribute_id, value)?;
    validate_localized_text_attribute_write_locale(context, node_to_write.attribute_id, value)?;

    if node_to_write.attribute_id == AttributeId::Value {
        match node {
            NodeType::Variable(var) => {
                let has_index_range = node_to_write.index_range.has_range();
                // Part 3 §8.58 Table 42: WriteFullArrayOnly=1 means Write of
                // IndexRange is NOT supported. Part 4 §5.11.4 Table 53: a Server
                // shall return Bad_WriteNotSupported in that case.
                if has_index_range
                    && var.access_level_ex() & AccessLevelExType::WriteFullArrayOnly.bits() as u32
                        != 0
                {
                    return Err(StatusCode::BadWriteNotSupported);
                }
                if let Some(address_space) = address_space {
                    if !validate_modeled_enum_value_to_write(
                        address_space,
                        var,
                        value,
                        has_index_range,
                    )? {
                        validate_value_to_write(var, value, type_tree, has_index_range)?;
                    }
                    validate_value_against_eurange(address_space, var, value, type_tree)?;
                } else {
                    validate_value_to_write(var, value, type_tree, has_index_range)?;
                }
            }
            NodeType::VariableType(var_type) => validate_value_data_type_to_write(
                var_type.data_type(),
                var_type.value_rank(),
                value,
                type_tree,
                node_to_write.index_range.has_range(),
            )?,
            _ => {}
        }
    }

    remember_localized_text_attribute_value(
        &context.info,
        node.node_id(),
        node_to_write.attribute_id,
        value,
    );

    Ok(())
}

fn validate_modeled_enum_value_to_write(
    address_space: &AddressSpace,
    variable: &Variable,
    value: &Variant,
    has_index_range: bool,
) -> Result<bool, StatusCode> {
    let Some(valid_values) = modeled_enum_value_set(address_space, variable) else {
        return Ok(false);
    };

    let Some(values_are_valid) = integer_values_are_in_enum_set(value, &valid_values) else {
        return Ok(false);
    };

    validate_value_rank_to_write(variable.value_rank(), value.is_array(), has_index_range)?;

    if values_are_valid {
        Ok(true)
    } else {
        Err(StatusCode::BadOutOfRange)
    }
}

fn modeled_enum_value_set(
    address_space: &AddressSpace,
    variable: &Variable,
) -> Option<HashSet<i64>> {
    let data_type = variable.data_type();
    let data_type_node = address_space.find(&data_type)?;
    let NodeType::DataType(data_type) = &*data_type_node else {
        return None;
    };
    let Some(DataTypeDefinition::Enum(enum_definition)) = data_type.data_type_definition() else {
        return None;
    };
    let fields = enum_definition.fields.as_ref()?;
    if fields.is_empty() {
        return None;
    }

    Some(fields.iter().map(|field| field.value).collect())
}

fn integer_values_are_in_enum_set(value: &Variant, valid_values: &HashSet<i64>) -> Option<bool> {
    match value {
        Variant::Array(array) => {
            if array.values.is_empty() {
                return None;
            }

            let mut values_are_valid = true;
            for element in &array.values {
                match scalar_integer_value_as_i64(element) {
                    Ok(Some(value)) => values_are_valid &= valid_values.contains(&value),
                    Ok(None) => return None,
                    Err(()) => return Some(false),
                };
            }
            Some(values_are_valid)
        }
        _ => match scalar_integer_value_as_i64(value) {
            Ok(Some(value)) => Some(valid_values.contains(&value)),
            Ok(None) => None,
            Err(()) => Some(false),
        },
    }
}

fn scalar_integer_value_as_i64(value: &Variant) -> Result<Option<i64>, ()> {
    match value {
        Variant::SByte(value) => Ok(Some(i64::from(*value))),
        Variant::Byte(value) => Ok(Some(i64::from(*value))),
        Variant::Int16(value) => Ok(Some(i64::from(*value))),
        Variant::UInt16(value) => Ok(Some(i64::from(*value))),
        Variant::Int32(value) => Ok(Some(i64::from(*value))),
        Variant::UInt32(value) => Ok(Some(i64::from(*value))),
        Variant::Int64(value) => Ok(Some(*value)),
        Variant::UInt64(value) if *value <= i64::MAX as u64 => Ok(Some(*value as i64)),
        Variant::UInt64(_) => Err(()),
        _ => Ok(None),
    }
}

fn validate_value_against_eurange(
    address_space: &AddressSpace,
    variable: &Variable,
    value: &Variant,
    type_tree: &dyn TypeTree,
) -> Result<(), StatusCode> {
    let Some((low, high)) = read_variable_eurange(address_space, variable.node_id(), type_tree)
    else {
        return Ok(());
    };

    if numeric_values_are_in_range(value, low, high) {
        Ok(())
    } else {
        Err(StatusCode::BadOutOfRange)
    }
}

fn read_variable_eurange(
    address_space: &AddressSpace,
    source_node_id: &NodeId,
    type_tree: &dyn TypeTree,
) -> Option<(f64, f64)> {
    let eurange_node = address_space.find_node_by_browse_name(
        source_node_id,
        Some((ReferenceTypeId::HasProperty, false)),
        type_tree,
        BrowseDirection::Forward,
        QualifiedName::from("EURange"),
    )?;

    let value = eurange_node
        .as_node()
        .get_attribute(
            TimestampsToReturn::Neither,
            AttributeId::Value,
            &NumericRange::None,
            &DataEncoding::Binary,
        )
        .and_then(|data_value| data_value.value)?;

    match value {
        Variant::ExtensionObject(eurange) => eurange.inner_as::<Range>().and_then(|range| {
            if range.low <= range.high {
                Some((range.low, range.high))
            } else {
                None
            }
        }),
        _ => None,
    }
}

fn numeric_values_are_in_range(value: &Variant, low: f64, high: f64) -> bool {
    match value {
        Variant::Array(array) => array
            .values
            .iter()
            .all(|element| scalar_numeric_value_is_in_range(element, low, high)),
        _ => scalar_numeric_value_is_in_range(value, low, high),
    }
}

fn scalar_numeric_value_is_in_range(value: &Variant, low: f64, high: f64) -> bool {
    match value.as_f64() {
        Some(value) => low <= value && value <= high,
        None => true,
    }
}
