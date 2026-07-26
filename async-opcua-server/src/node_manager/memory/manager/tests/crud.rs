use super::*;

#[tokio::test]
async fn duplicate_browse_name_returns_operation_level_bad_browse_name_duplicated() {
    let context = request_context();
    let parent_id = NodeId::new(1, "parent");
    let existing_child_id = NodeId::new(1, "existing-child");
    let duplicate_child_id = NodeId::new(1, "duplicate-child");
    let duplicate_browse_name = QualifiedName::new(1, "DuplicateName");
    let address_space = AddressSpace::new();
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(object_node(&parent_id, "parent"), None);
    address_space.insert(
        Object::new(
            &existing_child_id,
            duplicate_browse_name.clone(),
            "DuplicateName",
            EventNotifier::empty(),
        ),
        Some(&[(
            &parent_id,
            &NodeId::from(ReferenceTypeId::HasComponent),
            ReferenceDirection::Inverse,
        )]),
    );
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_object_node_item(&parent_id, &duplicate_child_id, duplicate_browse_name);

    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }

    assert_eq!(item.status(), StatusCode::BadBrowseNameDuplicated);
    assert!(item.added_node_id().is_null());
    assert!(!manager
        .address_space()
        .read()
        .node_exists(&duplicate_child_id));
}

#[tokio::test]
async fn invalid_type_definition_returns_operation_level_bad_type_definition_invalid() {
    let context = request_context();
    let parent_id = NodeId::new(1, "parent");
    let new_node_id = NodeId::new(1, "child-with-invalid-type");
    let invalid_type_definition = NodeId::new(2, "missing-object-type");
    let address_space = AddressSpace::new();
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(object_node(&parent_id, "parent"), None);
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_object_node_item_with_type_definition(
        &parent_id,
        &new_node_id,
        QualifiedName::new(1, "ChildWithInvalidType"),
        ExpandedNodeId::from(invalid_type_definition),
    );

    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }

    assert_eq!(item.status(), StatusCode::BadTypeDefinitionInvalid);
    assert!(item.added_node_id().is_null());
    assert!(!manager.address_space().read().node_exists(&new_node_id));
}

#[tokio::test]
async fn node_attributes_invalid_returns_operation_level_bad_node_attributes_invalid() {
    let context = request_context();
    let parent_id = NodeId::new(1, "parent");
    let new_node_id = NodeId::new(1, "object-with-variable-value-attribute");
    let address_space = AddressSpace::new();
    address_space.add_namespace("http://opcfoundation.org/UA/", 0);
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(
        ObjectType::new(
            &NodeId::from(ObjectTypeId::BaseObjectType),
            "BaseObjectType",
            "BaseObjectType",
            false,
        ),
        None,
    );
    address_space.insert::<_, NodeId>(object_node(&parent_id, "parent"), None);
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut attributes = object_attributes();
    attributes.specified_attributes = AttributesMask::VALUE.bits();
    let mut item = add_object_node_item_with_attributes(
        &parent_id,
        &new_node_id,
        QualifiedName::new(1, "ObjectWithVariableValueAttribute"),
        attributes,
    );

    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }

    assert_eq!(item.status(), StatusCode::BadNodeAttributesInvalid);
    assert!(item.added_node_id().is_null());
    assert!(!manager.address_space().read().node_exists(&new_node_id));
}
#[tokio::test]
async fn add_nodes_abstract_type_definition_in_metadata_is_rejected() {
    // OPC 10000-3 §5.5.2: an abstract ObjectType cannot be instantiated,
    // even when it exists only in the type metadata (no full node in the
    // address space) — the gap P3-03 closes.
    let context = request_context();
    let parent_id = NodeId::new(1, "parent");
    let abstract_type = NodeId::new(2, "abstract-object-type");
    let concrete_type = NodeId::new(2, "concrete-object-type");
    context.type_tree.write().add_type_node(
        &abstract_type,
        &NodeId::from(ObjectTypeId::BaseObjectType),
        NodeClass::ObjectType,
        true,
    );
    context.type_tree.write().add_type_node(
        &concrete_type,
        &NodeId::from(ObjectTypeId::BaseObjectType),
        NodeClass::ObjectType,
        false,
    );
    let build_manager = || {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 1);
        address_space.insert::<_, NodeId>(object_node(&parent_id, "parent"), None);
        InMemoryNodeManager::new(TestImpl, address_space)
    };

    // Abstract type definition (metadata-only) -> rejected, no node created.
    let manager = build_manager();
    let child_abstract = NodeId::new(1, "child-abstract");
    let mut item = add_object_node_item_with_type_definition(
        &parent_id,
        &child_abstract,
        QualifiedName::new(1, "ChildAbstract"),
        ExpandedNodeId::from(abstract_type.clone()),
    );
    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.status(), StatusCode::BadTypeDefinitionInvalid);
    assert!(!manager.address_space().read().node_exists(&child_abstract));

    // Concrete metadata-only type definition -> accepted.
    let manager = build_manager();
    let child_concrete = NodeId::new(1, "child-concrete");
    let mut item = add_object_node_item_with_type_definition(
        &parent_id,
        &child_concrete,
        QualifiedName::new(1, "ChildConcrete"),
        ExpandedNodeId::from(concrete_type.clone()),
    );
    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }
    assert_ne!(item.status(), StatusCode::BadTypeDefinitionInvalid);
}
#[tokio::test]
async fn add_nodes_variable_type_subtype_refinement_is_enforced() {
    // OPC 10000-3 §6.3: a VariableType subtype's DataType/ValueRank may only
    // further-restrict the supertype's.
    let context = request_context();
    let super_type = NodeId::new(1, "super-var-type");
    let int32 = NodeId::from(DataTypeId::Int32);
    let string_dt = NodeId::from(DataTypeId::String);
    {
        // SAFETY: This is called during server startup (single-threaded import phase).
        // No concurrent readers exist, so acquiring type_tree.write() here is safe.
        let mut tt = context.type_tree.write();
        // Register DataTypes so is_subtype_of can judge (String is NOT under Int32).
        tt.add_type_node(
            &int32,
            &NodeId::from(DataTypeId::BaseDataType),
            NodeClass::DataType,
            false,
        );
        tt.add_type_node(
            &string_dt,
            &NodeId::from(DataTypeId::BaseDataType),
            NodeClass::DataType,
            false,
        );
        tt.add_type_node(
            &NodeId::from(ReferenceTypeId::HasSubtype),
            &NodeId::from(ReferenceTypeId::References),
            NodeClass::ReferenceType,
            false,
        );
    }
    // Supertype: DataType Int32, ValueRank Scalar (-1).
    let build_manager = || {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 1);
        address_space.insert::<_, NodeId>(
            VariableType::new(
                &super_type,
                "SuperVarType",
                "SuperVarType",
                int32.clone(),
                false,
                -1,
            ),
            None,
        );
        InMemoryNodeManager::new(TestImpl, address_space)
    };

    // Widened DataType (String is not a subtype of Int32) -> rejected.
    let manager = build_manager();
    let mut item = add_variable_type_subtype_item(
        &super_type,
        &NodeId::new(1, "bad-dt"),
        QualifiedName::new(1, "BadDt"),
        string_dt.clone(),
        -1,
    );
    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.status(), StatusCode::BadNodeAttributesInvalid);

    // Widened ValueRank (scalar supertype -> array subtype) -> rejected.
    let manager = build_manager();
    let mut item = add_variable_type_subtype_item(
        &super_type,
        &NodeId::new(1, "bad-vr"),
        QualifiedName::new(1, "BadVr"),
        int32.clone(),
        1,
    );
    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.status(), StatusCode::BadNodeAttributesInvalid);

    // Valid restriction (same DataType, same ValueRank) -> accepted.
    let manager = build_manager();
    let mut item = add_variable_type_subtype_item(
        &super_type,
        &NodeId::new(1, "good"),
        QualifiedName::new(1, "Good"),
        int32.clone(),
        -1,
    );
    {
        let mut nodes = vec![&mut item];
        manager
            .add_nodes(&context, nodes.as_mut_slice())
            .await
            .unwrap();
    }
    assert_ne!(item.status(), StatusCode::BadNodeAttributesInvalid);
}
