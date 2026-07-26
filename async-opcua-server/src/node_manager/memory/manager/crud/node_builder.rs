use crate::{
    address_space::{AccessLevel, EventNotifier, NodeType},
    node_manager::AddNodeItem,
};
use opcua_nodes::{
    DataType, Method, NodeBase, Object, ObjectType, ReferenceType, Variable, VariableType, View,
};
use opcua_types::{
    AddNodeAttributes, AttributesMask, DataTypeId, LocalizedText, NodeClass, NodeId, StatusCode,
    Variant, WriteMask,
};

#[cfg(feature = "node-management")]
pub(super) fn build_node(item: &AddNodeItem, node_id: &NodeId) -> Result<NodeType, StatusCode> {
    match (item.node_class(), item.node_attributes()) {
        (NodeClass::Object, AddNodeAttributes::Object(attributes)) => {
            build_object(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (NodeClass::Variable, AddNodeAttributes::Variable(attributes)) => {
            build_variable(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (NodeClass::Method, AddNodeAttributes::Method(attributes)) => {
            build_method(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (NodeClass::ObjectType, AddNodeAttributes::ObjectType(attributes)) => {
            build_object_type(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (NodeClass::VariableType, AddNodeAttributes::VariableType(attributes)) => {
            build_variable_type(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (NodeClass::ReferenceType, AddNodeAttributes::ReferenceType(attributes)) => {
            build_reference_type(node_id, item.browse_name().clone(), attributes)
                .map(NodeType::from)
        }
        (NodeClass::DataType, AddNodeAttributes::DataType(attributes)) => {
            build_data_type(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (NodeClass::View, AddNodeAttributes::View(attributes)) => {
            build_view(node_id, item.browse_name().clone(), attributes).map(NodeType::from)
        }
        (
            NodeClass::Object
            | NodeClass::Variable
            | NodeClass::Method
            | NodeClass::ObjectType
            | NodeClass::VariableType
            | NodeClass::ReferenceType
            | NodeClass::DataType
            | NodeClass::View,
            _,
        ) => Err(StatusCode::BadNodeAttributesInvalid),
        _ => Err(StatusCode::BadNodeClassInvalid),
    }
}

#[cfg(feature = "node-management")]
pub(super) fn build_object(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::ObjectAttributes,
) -> Result<Object, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(attributes.specified_attributes, object_attributes_mask())?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);
    let event_notifier = if mask.contains(AttributesMask::EVENT_NOTIFIER) {
        EventNotifier::from_bits_truncate(attributes.event_notifier)
    } else {
        EventNotifier::empty()
    };

    let mut node = Object::new(node_id, browse_name, display_name, event_notifier);
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );
    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn build_variable(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::VariableAttributes,
) -> Result<Variable, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(attributes.specified_attributes, variable_attributes_mask())?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);
    let data_type = if mask.contains(AttributesMask::DATA_TYPE) {
        attributes.data_type.clone()
    } else {
        NodeId::from(DataTypeId::BaseDataType)
    };
    let value = if mask.contains(AttributesMask::VALUE) {
        attributes.value.clone()
    } else {
        Variant::Empty
    };

    let mut node = Variable::new_data_value(
        node_id,
        browse_name,
        display_name,
        data_type,
        None,
        None,
        value,
    );

    if mask.contains(AttributesMask::VALUE_RANK) {
        node.set_value_rank(attributes.value_rank);
    }
    if mask.contains(AttributesMask::HISTORIZING) {
        node.set_historizing(attributes.historizing);
    }
    if mask.contains(AttributesMask::ACCESS_LEVEL) {
        node.set_access_level(AccessLevel::from_bits_truncate(attributes.access_level));
    }
    if mask.contains(AttributesMask::USER_ACCESS_LEVEL) {
        node.set_user_access_level(AccessLevel::from_bits_truncate(
            attributes.user_access_level,
        ));
    }
    if mask.contains(AttributesMask::ARRAY_DIMENSIONS) {
        if let Some(array_dimensions) = attributes.array_dimensions.as_ref() {
            validate_array_dimensions(attributes.value_rank, array_dimensions)?;
            node.set_array_dimensions(array_dimensions);
        } else {
            return Err(StatusCode::BadNodeAttributesInvalid);
        }
    }
    if mask.contains(AttributesMask::MINIMUM_SAMPLING_INTERVAL) {
        node.set_minimum_sampling_interval(attributes.minimum_sampling_interval);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn build_method(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::MethodAttributes,
) -> Result<Method, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(attributes.specified_attributes, method_attributes_mask())?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);

    let mut node = Method::new(node_id, browse_name, display_name, false, false);
    if mask.contains(AttributesMask::EXECUTABLE) {
        node.set_executable(attributes.executable);
    }
    if mask.contains(AttributesMask::USER_EXECUTABLE) {
        node.set_user_executable(attributes.user_executable);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn build_object_type(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::ObjectTypeAttributes,
) -> Result<ObjectType, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(
        attributes.specified_attributes,
        object_type_attributes_mask(),
    )?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);

    let mut node = ObjectType::new(node_id, browse_name, display_name, false);
    if mask.contains(AttributesMask::IS_ABSTRACT) {
        node.set_is_abstract(attributes.is_abstract);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn build_variable_type(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::VariableTypeAttributes,
) -> Result<VariableType, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(
        attributes.specified_attributes,
        variable_type_attributes_mask(),
    )?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);
    let data_type = if mask.contains(AttributesMask::DATA_TYPE) {
        attributes.data_type.clone()
    } else {
        NodeId::from(DataTypeId::BaseDataType)
    };
    let value = if mask.contains(AttributesMask::VALUE) {
        attributes.value.clone()
    } else {
        Variant::Empty
    };

    let mut node = VariableType::new(node_id, browse_name, display_name, data_type, false, -1);

    if mask.contains(AttributesMask::VALUE) {
        node.set_value(value);
    }
    if mask.contains(AttributesMask::VALUE_RANK) {
        node.set_value_rank(attributes.value_rank);
    }
    if mask.contains(AttributesMask::ARRAY_DIMENSIONS) {
        if let Some(array_dimensions) = attributes.array_dimensions.as_ref() {
            validate_array_dimensions(attributes.value_rank, array_dimensions)?;
            node.set_array_dimensions(array_dimensions);
        } else {
            return Err(StatusCode::BadNodeAttributesInvalid);
        }
    }
    if mask.contains(AttributesMask::IS_ABSTRACT) {
        node.set_is_abstract(attributes.is_abstract);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn validate_array_dimensions(
    value_rank: i32,
    array_dimensions: &[u32],
) -> Result<(), StatusCode> {
    if value_rank >= 1 && array_dimensions.len() != value_rank as usize {
        Err(StatusCode::BadNodeAttributesInvalid)
    } else {
        Ok(())
    }
}

#[cfg(feature = "node-management")]
pub(super) fn build_reference_type(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::ReferenceTypeAttributes,
) -> Result<ReferenceType, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(
        attributes.specified_attributes,
        reference_type_attributes_mask(),
    )?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);

    let mut node = ReferenceType::new(node_id, browse_name, display_name, None, false, false);
    if mask.contains(AttributesMask::IS_ABSTRACT) {
        node.set_is_abstract(attributes.is_abstract);
    }
    if mask.contains(AttributesMask::SYMMETRIC) {
        node.set_symmetric(attributes.symmetric);
    }
    if mask.contains(AttributesMask::INVERSE_NAME) {
        node.set_inverse_name(attributes.inverse_name.clone());
    }
    // OPC 10000-3 §5.3.2: a symmetric ReferenceType must not define an
    // InverseName. Enforced via the node-level well-formedness invariant so the
    // rule has a single source of truth.
    if !node.symmetric_inverse_name_is_valid() {
        return Err(StatusCode::BadNodeAttributesInvalid);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn build_data_type(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::DataTypeAttributes,
) -> Result<DataType, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(attributes.specified_attributes, data_type_attributes_mask())?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);

    let mut node = DataType::new(node_id, browse_name, display_name, false);
    if mask.contains(AttributesMask::IS_ABSTRACT) {
        node.set_is_abstract(attributes.is_abstract);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn build_view(
    node_id: &NodeId,
    browse_name: impl Into<opcua_types::QualifiedName>,
    attributes: &opcua_types::ViewAttributes,
) -> Result<View, StatusCode> {
    let browse_name = browse_name.into();
    let mask = attributes_mask(attributes.specified_attributes, view_attributes_mask())?;
    let display_name =
        display_name_or_browse_name(&mask, attributes.display_name.clone(), &browse_name);
    let event_notifier = if mask.contains(AttributesMask::EVENT_NOTIFIER) {
        EventNotifier::from_bits_truncate(attributes.event_notifier)
    } else {
        EventNotifier::empty()
    };

    let mut node = View::new(node_id, browse_name, display_name, event_notifier, false);
    if mask.contains(AttributesMask::CONTAINS_NO_LOOPS) {
        node.set_contains_no_loops(attributes.contains_no_loops);
    }
    apply_base_attributes(
        &mut node,
        mask,
        attributes.description.clone(),
        attributes.write_mask,
        attributes.user_write_mask,
    );

    Ok(node)
}

#[cfg(feature = "node-management")]
pub(super) fn attributes_mask(
    specified_attributes: u32,
    allowed_attributes: AttributesMask,
) -> Result<AttributesMask, StatusCode> {
    let mask = AttributesMask::from_bits(specified_attributes)
        .ok_or(StatusCode::BadNodeAttributesInvalid)?;
    if mask.bits() & !allowed_attributes.bits() != 0 {
        return Err(StatusCode::BadNodeAttributesInvalid);
    }
    Ok(mask)
}

#[cfg(feature = "node-management")]
pub(super) fn base_attributes_mask() -> AttributesMask {
    AttributesMask::DESCRIPTION
        | AttributesMask::DISPLAY_NAME
        | AttributesMask::WRITE_MASK
        | AttributesMask::USER_WRITE_MASK
}

#[cfg(feature = "node-management")]
pub(super) fn object_attributes_mask() -> AttributesMask {
    base_attributes_mask() | AttributesMask::EVENT_NOTIFIER
}

#[cfg(feature = "node-management")]
pub(super) fn variable_attributes_mask() -> AttributesMask {
    base_attributes_mask()
        | AttributesMask::ACCESS_LEVEL
        | AttributesMask::ARRAY_DIMENSIONS
        | AttributesMask::DATA_TYPE
        | AttributesMask::HISTORIZING
        | AttributesMask::MINIMUM_SAMPLING_INTERVAL
        | AttributesMask::USER_ACCESS_LEVEL
        | AttributesMask::VALUE
        | AttributesMask::VALUE_RANK
}

#[cfg(feature = "node-management")]
pub(super) fn method_attributes_mask() -> AttributesMask {
    base_attributes_mask() | AttributesMask::EXECUTABLE | AttributesMask::USER_EXECUTABLE
}

#[cfg(feature = "node-management")]
pub(super) fn object_type_attributes_mask() -> AttributesMask {
    base_attributes_mask() | AttributesMask::IS_ABSTRACT
}

#[cfg(feature = "node-management")]
pub(super) fn variable_type_attributes_mask() -> AttributesMask {
    base_attributes_mask()
        | AttributesMask::ARRAY_DIMENSIONS
        | AttributesMask::DATA_TYPE
        | AttributesMask::IS_ABSTRACT
        | AttributesMask::VALUE
        | AttributesMask::VALUE_RANK
}

#[cfg(feature = "node-management")]
pub(super) fn reference_type_attributes_mask() -> AttributesMask {
    base_attributes_mask()
        | AttributesMask::INVERSE_NAME
        | AttributesMask::IS_ABSTRACT
        | AttributesMask::SYMMETRIC
}

#[cfg(feature = "node-management")]
pub(super) fn data_type_attributes_mask() -> AttributesMask {
    base_attributes_mask() | AttributesMask::IS_ABSTRACT
}

#[cfg(feature = "node-management")]
pub(super) fn view_attributes_mask() -> AttributesMask {
    base_attributes_mask() | AttributesMask::CONTAINS_NO_LOOPS | AttributesMask::EVENT_NOTIFIER
}

#[cfg(feature = "node-management")]
pub(super) fn display_name_or_browse_name(
    mask: &AttributesMask,
    display_name: LocalizedText,
    browse_name: &opcua_types::QualifiedName,
) -> LocalizedText {
    if mask.contains(AttributesMask::DISPLAY_NAME) {
        display_name
    } else {
        browse_name.name.to_string().into()
    }
}

#[cfg(feature = "node-management")]
pub(super) fn apply_base_attributes<T: NodeBase>(
    node: &mut T,
    mask: AttributesMask,
    description: LocalizedText,
    write_mask: u32,
    user_write_mask: u32,
) {
    if mask.contains(AttributesMask::DESCRIPTION) {
        node.set_description(description);
    }
    if mask.contains(AttributesMask::WRITE_MASK) {
        node.set_write_mask(WriteMask::from_bits_truncate(write_mask));
    }
    if mask.contains(AttributesMask::USER_WRITE_MASK) {
        node.set_user_write_mask(WriteMask::from_bits_truncate(user_write_mask));
    }
}
