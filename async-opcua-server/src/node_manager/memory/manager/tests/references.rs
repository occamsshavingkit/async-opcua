use super::*;
use crate::node_manager::memory::manager::references::reference_type_is_abstract;

#[tokio::test]
async fn abstract_reference_type_returns_operation_level_bad_reference_type_id_invalid() {
    let context = request_context();
    let source_id = NodeId::new(1, "source");
    let target_id = NodeId::new(1, "target");
    let abstract_reference_type = NodeId::from(ReferenceTypeId::References);
    let address_space = AddressSpace::new();
    address_space.add_namespace("http://opcfoundation.org/UA/", 0);
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(object_node(&source_id, "source"), None);
    address_space.insert::<_, NodeId>(object_node(&target_id, "target"), None);
    address_space.insert::<_, NodeId>(
        ReferenceType::new(
            &abstract_reference_type,
            "References",
            "References",
            None,
            true,
            true,
        ),
        None,
    );
    assert_eq!(
        context.type_tree.read().get(&abstract_reference_type),
        Some(NodeClass::ReferenceType)
    );
    assert!(reference_type_is_abstract(
        &address_space,
        &abstract_reference_type
    ));
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_reference_item_with_type(&source_id, &target_id, &abstract_reference_type);

    {
        let mut references = vec![&mut item];
        manager
            .add_references(&context, references.as_mut_slice())
            .await
            .unwrap();
    }

    assert_eq!(item.result_status(), StatusCode::BadReferenceTypeIdInvalid);
    assert_eq!(item.source_status(), StatusCode::BadReferenceTypeIdInvalid);
    assert_eq!(item.target_status(), StatusCode::BadReferenceTypeIdInvalid);
    assert!(!manager.address_space().read().has_reference(
        &source_id,
        &target_id,
        &abstract_reference_type
    ));
}

#[tokio::test]
async fn standard_abstract_reference_type_returns_operation_level_bad_reference_type_id_invalid() {
    let context = request_context();
    let source_id = NodeId::new(1, "source");
    let target_id = NodeId::new(1, "target");
    let abstract_reference_type = NodeId::from(ReferenceTypeId::References);
    let address_space = AddressSpace::new();
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(object_node(&source_id, "source"), None);
    address_space.insert::<_, NodeId>(object_node(&target_id, "target"), None);
    assert_eq!(
        context.type_tree.read().get(&abstract_reference_type),
        Some(NodeClass::ReferenceType)
    );
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_reference_item_with_type(&source_id, &target_id, &abstract_reference_type);

    {
        let mut references = vec![&mut item];
        manager
            .add_references(&context, references.as_mut_slice())
            .await
            .unwrap();
    }

    assert_eq!(item.result_status(), StatusCode::BadReferenceTypeIdInvalid);
    assert_eq!(item.source_status(), StatusCode::BadReferenceTypeIdInvalid);
    assert_eq!(item.target_status(), StatusCode::BadReferenceTypeIdInvalid);
    assert!(!manager.address_space().read().has_reference(
        &source_id,
        &target_id,
        &abstract_reference_type
    ));
}
#[tokio::test]
async fn add_references_target_node_class_mismatch_is_bad_node_class_invalid() {
    // OPC 10000-4 §5.8.3: the declared targetNodeClass must match the actual
    // target node's NodeClass. Unspecified means the client asserts nothing.
    let context = request_context();
    let source_id = NodeId::new(1, "source");
    let target_id = NodeId::new(1, "target"); // an Object
    let reference_type = NodeId::from(ReferenceTypeId::HasComponent);
    context.type_tree.write().add_type_node(
        &reference_type,
        &NodeId::from(ReferenceTypeId::References),
        NodeClass::ReferenceType,
        false,
    );
    let build_manager = || {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 1);
        address_space.insert::<_, NodeId>(
            ReferenceType::new(
                &reference_type,
                "HasComponent",
                "HasComponent",
                None,
                false,
                false,
            ),
            None,
        );
        address_space.insert::<_, NodeId>(object_node(&source_id, "source"), None);
        address_space.insert::<_, NodeId>(object_node(&target_id, "target"), None);
        InMemoryNodeManager::new(TestImpl, address_space)
    };

    // Mismatch: target is an Object but Variable is declared -> rejected, no reference.
    let manager = build_manager();
    let mut item =
        add_reference_item_full(&source_id, &target_id, &reference_type, NodeClass::Variable);
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::BadNodeClassInvalid);
    assert!(!manager
        .address_space()
        .read()
        .has_reference(&source_id, &target_id, &reference_type));

    // Matching class -> accepted.
    let manager = build_manager();
    let mut item =
        add_reference_item_full(&source_id, &target_id, &reference_type, NodeClass::Object);
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::Good);
    assert!(manager
        .address_space()
        .read()
        .has_reference(&source_id, &target_id, &reference_type));

    // Unspecified -> no assertion, accepted.
    let manager = build_manager();
    let mut item = add_reference_item_full(
        &source_id,
        &target_id,
        &reference_type,
        NodeClass::Unspecified,
    );
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::Good);
    assert!(manager
        .address_space()
        .read()
        .has_reference(&source_id, &target_id, &reference_type));
}
#[tokio::test]
async fn add_references_has_subtype_between_mismatched_classes_is_rejected() {
    // OPC 10000-3 §5.3: HasSubtype connects a type node to a subtype of the
    // SAME type NodeClass. ObjectType -> VariableType is forbidden;
    // ObjectType -> ObjectType is allowed.
    let context = request_context();
    let src_type = NodeId::new(1, "src-object-type");
    let var_type = NodeId::new(1, "a-variable-type");
    let obj_type_2 = NodeId::new(1, "another-object-type");
    let has_subtype = NodeId::from(ReferenceTypeId::HasSubtype);
    context.type_tree.write().add_type_node(
        &has_subtype,
        &NodeId::from(ReferenceTypeId::References),
        NodeClass::ReferenceType,
        false,
    );
    let build_manager = || {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 1);
        address_space.insert::<_, NodeId>(
            ReferenceType::new(&has_subtype, "HasSubtype", "HasSubtype", None, false, false),
            None,
        );
        address_space.insert::<_, NodeId>(
            ObjectType::new(&src_type, "SrcType", "SrcType", false),
            None,
        );
        address_space.insert::<_, NodeId>(
            ObjectType::new(&obj_type_2, "ObjType2", "ObjType2", false),
            None,
        );
        address_space.insert::<_, NodeId>(
            VariableType::new(
                &var_type,
                "VarType",
                "VarType",
                DataTypeId::BaseDataType.into(),
                false,
                -1,
            ),
            None,
        );
        InMemoryNodeManager::new(TestImpl, address_space)
    };

    // ObjectType -> VariableType: forbidden.
    let manager = build_manager();
    let mut item =
        add_reference_item_full(&src_type, &var_type, &has_subtype, NodeClass::Unspecified);
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::BadReferenceNotAllowed);

    // ObjectType -> ObjectType: allowed.
    let manager = build_manager();
    let mut item =
        add_reference_item_full(&src_type, &obj_type_2, &has_subtype, NodeClass::Unspecified);
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::Good);
}

#[tokio::test]
async fn add_references_second_has_type_definition_is_rejected() {
    // OPC 10000-3 §5.5.1: an Object is the SourceNode of exactly one
    // HasTypeDefinition Reference. A second one (to a different target) is
    // rejected even though the duplicate-same-target check wouldn't catch it.
    let context = request_context();
    let source_id = NodeId::new(1, "instance");
    let type_a = NodeId::new(1, "type-a");
    let type_b = NodeId::new(1, "type-b");
    let has_type_def = NodeId::from(ReferenceTypeId::HasTypeDefinition);
    context.type_tree.write().add_type_node(
        &has_type_def,
        &NodeId::from(ReferenceTypeId::References),
        NodeClass::ReferenceType,
        false,
    );
    let build_manager = |with_existing: bool| {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 1);
        address_space.insert::<_, NodeId>(
            ReferenceType::new(
                &has_type_def,
                "HasTypeDefinition",
                "HasTypeDefinition",
                None,
                false,
                false,
            ),
            None,
        );
        address_space.insert::<_, NodeId>(object_node(&source_id, "instance"), None);
        address_space.insert::<_, NodeId>(object_node(&type_a, "type-a"), None);
        address_space.insert::<_, NodeId>(object_node(&type_b, "type-b"), None);
        if with_existing {
            address_space.insert_reference(&source_id, &type_a, &has_type_def);
        }
        InMemoryNodeManager::new(TestImpl, address_space)
    };

    // A second HasTypeDefinition to a different target -> rejected.
    let manager = build_manager(true);
    let mut item =
        add_reference_item_full(&source_id, &type_b, &has_type_def, NodeClass::Unspecified);
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::BadReferenceNotAllowed);
    assert!(!manager
        .address_space()
        .read()
        .has_reference(&source_id, &type_b, &has_type_def));

    // The first HasTypeDefinition on a node without one -> accepted.
    let manager = build_manager(false);
    let mut item =
        add_reference_item_full(&source_id, &type_a, &has_type_def, NodeClass::Unspecified);
    {
        let mut refs = vec![&mut item];
        manager
            .add_references(&context, refs.as_mut_slice())
            .await
            .unwrap();
    }
    assert_eq!(item.source_status(), StatusCode::Good);
    assert!(manager
        .address_space()
        .read()
        .has_reference(&source_id, &type_a, &has_type_def));
}

#[tokio::test]
async fn duplicate_reference_returns_operation_level_bad_duplicate_reference_not_allowed() {
    let context = request_context();
    let source_id = NodeId::new(1, "source");
    let target_id = NodeId::new(1, "target");
    let reference_type = NodeId::from(ReferenceTypeId::HasComponent);
    let address_space = AddressSpace::new();
    address_space.add_namespace("http://opcfoundation.org/UA/", 0);
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(
        ReferenceType::new(
            &reference_type,
            "HasComponent",
            "HasComponent",
            None,
            false,
            false,
        ),
        None,
    );
    address_space.insert::<_, NodeId>(object_node(&source_id, "source"), None);
    address_space.insert::<_, NodeId>(
        object_node(&target_id, "target"),
        Some(&[(&source_id, &reference_type, ReferenceDirection::Inverse)]),
    );
    context.type_tree.write().add_type_node(
        &reference_type,
        &NodeId::from(ReferenceTypeId::References),
        NodeClass::ReferenceType,
        false,
    );
    assert!(address_space.has_reference(&source_id, &target_id, &reference_type));
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_reference_item_with_type(&source_id, &target_id, &reference_type);

    {
        let mut references = vec![&mut item];
        manager
            .add_references(&context, references.as_mut_slice())
            .await
            .unwrap();
    }

    assert_eq!(
        item.result_status(),
        StatusCode::BadDuplicateReferenceNotAllowed
    );
    assert_eq!(
        item.source_status(),
        StatusCode::BadDuplicateReferenceNotAllowed
    );
    assert_eq!(
        item.target_status(),
        StatusCode::BadDuplicateReferenceNotAllowed
    );
    assert!(manager
        .address_space()
        .read()
        .has_reference(&source_id, &target_id, &reference_type));
}

#[tokio::test]
async fn structural_reference_returns_operation_level_bad_reference_not_allowed() {
    let context = request_context();
    let source_id = NodeId::new(1, "source");
    let target_id = NodeId::new(1, "object-target");
    let reference_type = NodeId::from(ReferenceTypeId::HasProperty);
    let address_space = AddressSpace::new();
    address_space.add_namespace("http://opcfoundation.org/UA/", 0);
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(
        ReferenceType::new(
            &reference_type,
            "HasProperty",
            "HasProperty",
            None,
            false,
            false,
        ),
        None,
    );
    address_space.insert::<_, NodeId>(object_node(&source_id, "source"), None);
    address_space.insert::<_, NodeId>(object_node(&target_id, "object-target"), None);
    context.type_tree.write().add_type_node(
        &reference_type,
        &NodeId::from(ReferenceTypeId::References),
        NodeClass::ReferenceType,
        false,
    );
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let mut item = add_reference_item_with_type(&source_id, &target_id, &reference_type);

    {
        let mut references = vec![&mut item];
        manager
            .add_references(&context, references.as_mut_slice())
            .await
            .unwrap();
    }

    // OPC-10000-3 7.8 requires HasProperty targets to be Variables; an Object
    // target therefore violates the data model and OPC-10000-4 5.8.3.4 maps
    // that operation-level failure to Bad_ReferenceNotAllowed.
    assert_eq!(item.result_status(), StatusCode::BadReferenceNotAllowed);
    assert_eq!(item.source_status(), StatusCode::BadReferenceNotAllowed);
    assert_eq!(item.target_status(), StatusCode::BadReferenceNotAllowed);
    assert!(!manager
        .address_space()
        .read()
        .has_reference(&source_id, &target_id, &reference_type));
}
#[tokio::test]
async fn delete_node_references_cleans_cross_manager_references_without_unrelated_deletes() {
    let context = request_context();
    let local_source_id = NodeId::new(1, "local-source");
    let local_target_id = NodeId::new(1, "local-target");
    let deleted_source_id = NodeId::new(2, "deleted-source");
    let kept_target_id = NodeId::new(2, "kept-target");
    let deleted_target_id = NodeId::new(2, "deleted-target");
    let unrelated_target_id = NodeId::new(2, "unrelated-target");
    let reference_type = NodeId::from(ReferenceTypeId::HasComponent);
    let address_space = AddressSpace::new();
    address_space.add_namespace("urn:test", 1);
    address_space.insert::<_, NodeId>(object_node(&local_source_id, "local-source"), None);
    address_space.insert::<_, NodeId>(object_node(&local_target_id, "local-target"), None);
    address_space.insert_reference(&deleted_source_id, &local_target_id, &reference_type);
    address_space.insert_reference(&local_source_id, &kept_target_id, &reference_type);
    address_space.insert_reference(&local_source_id, &deleted_target_id, &reference_type);
    address_space.insert_reference(&local_source_id, &unrelated_target_id, &reference_type);
    let manager = InMemoryNodeManager::new(TestImpl, address_space);
    let remove_source_refs = deleted_node_item(&deleted_source_id, false);
    let keep_target_refs = deleted_node_item(&kept_target_id, false);
    let remove_target_refs = deleted_node_item(&deleted_target_id, true);

    manager
        .delete_node_references(
            &context,
            &[&remove_source_refs, &keep_target_refs, &remove_target_refs],
        )
        .await;

    let address_space = manager.address_space().read();
    assert!(!address_space.has_reference(&deleted_source_id, &local_target_id, &reference_type));
    assert!(address_space.has_reference(&local_source_id, &kept_target_id, &reference_type));
    assert!(!address_space.has_reference(&local_source_id, &deleted_target_id, &reference_type));
    assert!(address_space.has_reference(&local_source_id, &unrelated_target_id, &reference_type));
}
