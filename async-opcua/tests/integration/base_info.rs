//! Feature 097 — Base Info conformance completion: OrderedListType/IOrderedObjectType
//! (OPC-10000-5 §6.10/§6.11), SelectionListType (§7.18), OptionSetType (§7.17), ValueAsText
//! (OPC-10000-3), ReferenceDescriptionVariableType/HasReferenceDescription (OPC-10000-23 §5),
//! CurrencyUnit property (OPC-10000-5 §12.2.12.2).

use super::utils::setup;
use opcua::server::address_space::ObjectBuilder;
use opcua::server::base_info::{
    add_ordered_object, attach_reference_description, create_currency_variable,
    create_enum_variable_with_value_as_text, create_option_set_variable,
    create_ordered_list_in_address_space, create_selection_list_variable, update_enum_value,
};
use opcua::types::{
    Array, AttributeId, CurrencyUnitType, DataTypeId, ExpandedNodeId, LocalizedText, NodeId,
    ObjectId, ObjectTypeId, ReadValueId, ReferenceTypeId, TimestampsToReturn, VariableId,
    VariantScalarTypeId,
};
use opcua_types::{DateTime, Variant};
use std::time::Duration;

fn array_variant(scalar: VariantScalarTypeId, values: Vec<Variant>) -> Variant {
    Variant::Array(Box::new(
        Array::new(scalar, values).expect("array should be well-formed"),
    ))
}

async fn read_one(session: &opcua_client::Session, node_id: NodeId) -> Option<Variant> {
    let r = session
        .read(
            &[ReadValueId {
                node_id,
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    r[0].value.clone()
}

// ---------------------------------------------------------------------------
// US1 (CU 2512 + CU 3560): OrderedListType / IOrderedObjectType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ordered_list_children_are_ordered_and_interface_conformant() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "OrderedListRoot");
    let (list_id, child_ids, number_in_list_ids) = {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "OrderedListRoot", "OrderedListRoot")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        let list_id = create_ordered_list_in_address_space(space, 2, "MyList", parent_id.clone());
        let mut child_ids = Vec::new();
        let mut number_in_list_ids = Vec::new();
        for i in 0..3 {
            let child = add_ordered_object(space, 2, &list_id, &format!("Item{i}"), i as i64);
            number_in_list_ids.push(NodeId::new(2, format!("Item{i}_NumberInList")));
            child_ids.push(child);
        }
        (list_id, child_ids, number_in_list_ids)
    };

    // HasOrderedComponent exposes all 3 children (OPC-10000-5 §6.10 requires the reference
    // itself to exist; it does NOT require the Browse service to return them in any particular
    // order -- the spec's own §6.11 rationale for NumberInList is precisely that "not all
    // Clients consider the order returned by the Browse Service", so NumberInList (checked
    // below) is the authoritative order signal, not Browse response order).
    let refs = session
        .browse(
            &[opcua::types::BrowseDescription {
                node_id: list_id.clone(),
                browse_direction: opcua::types::BrowseDirection::Forward,
                reference_type_id: ReferenceTypeId::HasOrderedComponent.into(),
                include_subtypes: true,
                node_class_mask: 0,
                result_mask: opcua::types::BrowseResultMask::All as u32,
            }],
            1000,
            None,
        )
        .await
        .unwrap();
    let references = refs[0].references.clone().unwrap_or_default();
    assert_eq!(references.len(), 3);
    let browsed_ids: std::collections::HashSet<_> = references
        .iter()
        .map(|r| r.node_id.node_id.clone())
        .collect();
    let expected_ids: std::collections::HashSet<_> = child_ids.iter().cloned().collect();
    assert_eq!(
        browsed_ids, expected_ids,
        "HasOrderedComponent must reference exactly the 3 children"
    );

    // NumberInList is unique per child and matches its position -- this, not Browse order, is
    // how a client reconstructs the list's true order.
    for (i, number_in_list_id) in number_in_list_ids.iter().enumerate() {
        let value = read_one(&session, number_in_list_id.clone()).await;
        assert_eq!(
            value,
            Some(Variant::Int64(i as i64)),
            "NumberInList should match the child's list position"
        );
    }

    // Each child implements IOrderedObjectType via HasInterface.
    for child_id in &child_ids {
        let space = nm.address_space().read();
        assert!(
            space.has_reference(
                child_id,
                &NodeId::from(ObjectTypeId::IOrderedObjectType),
                ReferenceTypeId::HasInterface,
            ),
            "ordered child must implement IOrderedObjectType via HasInterface (closes CU 3560 too)"
        );
    }
}

// ---------------------------------------------------------------------------
// US2 (CU 2711): SelectionListType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn selection_list_exposes_selections_descriptions_and_restrict_flag() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "SelectionListParent");
    let var_id = {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "SelectionListParent", "SelectionListParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        create_selection_list_variable(
            space,
            2,
            "Recipe",
            parent_id.clone(),
            DataTypeId::String.into(),
            Variant::from("Bread"),
            array_variant(
                VariantScalarTypeId::String,
                vec![
                    Variant::from("Bread"),
                    Variant::from("Cake"),
                    Variant::from("Pie"),
                ],
            ),
            Some(array_variant(
                VariantScalarTypeId::LocalizedText,
                vec![
                    Variant::from(LocalizedText::new("en", "Bread")),
                    Variant::from(LocalizedText::new("en", "Cake")),
                    Variant::from(LocalizedText::new("en", "Pie")),
                ],
            )),
            Some(true),
        )
    };

    let selections_id = NodeId::new(2, "Recipe_Selections");
    let descriptions_id = NodeId::new(2, "Recipe_SelectionDescriptions");
    let restrict_id = NodeId::new(2, "Recipe_RestrictToList");

    match read_one(&session, selections_id).await {
        Some(Variant::Array(arr)) => assert_eq!(arr.values.len(), 3),
        other => panic!("Selections = {other:?}, expected a 3-element array"),
    }
    match read_one(&session, descriptions_id).await {
        Some(Variant::Array(arr)) => assert_eq!(arr.values.len(), 3),
        other => panic!("SelectionDescriptions = {other:?}, expected a 3-element array"),
    }
    assert_eq!(
        read_one(&session, restrict_id).await,
        Some(Variant::Boolean(true))
    );
    assert_eq!(
        read_one(&session, var_id).await,
        Some(Variant::from("Bread"))
    );
}

// ---------------------------------------------------------------------------
// US3 (CU 3127): OptionSetType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn option_set_exposes_per_bit_values_and_bitmask() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "OptionSetParent");
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "OptionSetParent", "OptionSetParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        create_option_set_variable(
            space,
            2,
            "DeviceStatus",
            parent_id.clone(),
            DataTypeId::UInt32.into(),
            Variant::from(0b101u32),
            array_variant(
                VariantScalarTypeId::LocalizedText,
                vec![
                    Variant::from(LocalizedText::new("en", "Running")),
                    Variant::from(LocalizedText::new("en", "Fault")),
                    Variant::from(LocalizedText::new("en", "Maintenance")),
                ],
            ),
            Some(array_variant(
                VariantScalarTypeId::Boolean,
                vec![
                    Variant::from(true),
                    Variant::from(false),
                    Variant::from(true),
                ],
            )),
        );
    }

    let values_id = NodeId::new(2, "DeviceStatus_OptionSetValues");
    let bit_mask_id = NodeId::new(2, "DeviceStatus_BitMask");

    match read_one(&session, values_id).await {
        Some(Variant::Array(arr)) => assert_eq!(arr.values.len(), 3),
        other => panic!("OptionSetValues = {other:?}, expected a 3-element array"),
    }
    match read_one(&session, bit_mask_id).await {
        Some(Variant::Array(arr)) => {
            assert_eq!(
                arr.values,
                vec![
                    Variant::Boolean(true),
                    Variant::Boolean(false),
                    Variant::Boolean(true)
                ]
            );
        }
        other => panic!("BitMask = {other:?}, expected [true, false, true]"),
    }
}

// ---------------------------------------------------------------------------
// US4 (CU 2969): ValueAsText.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn value_as_text_tracks_enumerated_value_changes() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "EnumParent");
    let enum_values = vec![
        (0i64, LocalizedText::new("en", "Idle")),
        (1i64, LocalizedText::new("en", "Running")),
        (2i64, LocalizedText::new("en", "Faulted")),
    ];
    let handle = {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "EnumParent", "EnumParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        create_enum_variable_with_value_as_text(
            space,
            2,
            "State",
            parent_id.clone(),
            DataTypeId::Int32.into(),
            &enum_values,
            0,
        )
    };

    assert_eq!(
        read_one(&session, handle.value_as_text_id.clone()).await,
        Some(Variant::from(LocalizedText::new("en", "Idle")))
    );

    {
        let space = nm.address_space().read();
        update_enum_value(&space, &handle, &enum_values, 1);
    }
    assert_eq!(
        read_one(&session, handle.value_as_text_id.clone()).await,
        Some(Variant::from(LocalizedText::new("en", "Running")))
    );

    {
        let space = nm.address_space().read();
        update_enum_value(&space, &handle, &enum_values, 2);
    }
    assert_eq!(
        read_one(&session, handle.value_as_text_id).await,
        Some(Variant::from(LocalizedText::new("en", "Faulted")))
    );
}

// ---------------------------------------------------------------------------
// US5 (CU 3996): ReferenceDescriptionVariableType / HasReferenceDescription.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reference_description_documents_a_real_reference() {
    let (_tester, nm, session) = setup().await;

    let source_id = NodeId::new(2, "RefDescSource");
    let target_id = NodeId::new(2, "RefDescTarget");
    let rd_id = {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&source_id, "RefDescSource", "RefDescSource")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);
        ObjectBuilder::new(&target_id, "RefDescTarget", "RefDescTarget")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);
        space.insert_reference(&source_id, &target_id, ReferenceTypeId::HasNotifier);

        attach_reference_description(
            space,
            2,
            "SourceToTarget",
            &source_id,
            ReferenceTypeId::HasNotifier.into(),
            true,
            ExpandedNodeId::from(target_id.clone()),
        )
    };

    // The described reference is documented via HasReferenceDescription from the source.
    {
        let space = nm.address_space().read();
        assert!(space.has_reference(&source_id, &rd_id, ReferenceTypeId::HasReferenceDescription,));
    }

    let value = read_one(&session, rd_id).await;
    let Some(Variant::ExtensionObject(eo)) = value else {
        panic!(
            "ReferenceDescriptionVariableType Value should be an ExtensionObject, got {value:?}"
        );
    };
    let described = eo
        .inner_as::<opcua_types::ReferenceDescriptionDataType>()
        .expect("should decode as ReferenceDescriptionDataType");
    assert_eq!(described.source_node, source_id);
    assert_eq!(
        described.reference_type,
        NodeId::from(ReferenceTypeId::HasNotifier)
    );
    assert!(described.is_forward);
    assert_eq!(described.target_node, ExpandedNodeId::from(target_id));
}

// ---------------------------------------------------------------------------
// US6 (CU 5240): CurrencyUnit property.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn currency_unit_property_reports_iso4217_fields() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "CurrencyParent");
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "CurrencyParent", "CurrencyParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        create_currency_variable(
            space,
            2,
            "Price",
            parent_id.clone(),
            19.99,
            CurrencyUnitType {
                numeric_code: 840,
                exponent: 2,
                alphabetic_code: "USD".into(),
                currency: LocalizedText::new("en", "US Dollar"),
            },
        );
    }

    let currency_id = NodeId::new(2, "Price_CurrencyUnit");
    let value = read_one(&session, currency_id).await;
    let Some(Variant::ExtensionObject(eo)) = value else {
        panic!("CurrencyUnit Value should be an ExtensionObject, got {value:?}");
    };
    let currency = eo
        .inner_as::<CurrencyUnitType>()
        .expect("should decode as CurrencyUnitType");
    assert_eq!(currency.numeric_code, 840);
    assert_eq!(currency.exponent, 2);
    assert_eq!(currency.alphabetic_code.as_ref(), "USD");
    assert_eq!(currency.currency, LocalizedText::new("en", "US Dollar"));
}

// ---------------------------------------------------------------------------
// US7 (CU 3198): EstimatedReturnTime.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn estimated_return_time_reflects_scheduled_shutdown_and_is_null_otherwise() {
    let (tester, _nm, session) = setup().await;

    // Nothing scheduled yet: null.
    let before = read_one(&session, VariableId::Server_EstimatedReturnTime.into()).await;
    assert!(
        before.is_none() || matches!(before, Some(Variant::Empty)),
        "EstimatedReturnTime should be null before any shutdown is scheduled, got {before:?}"
    );

    let expected_return = DateTime::from(DateTime::now().checked_ticks() + 3_600 * 10_000_000);
    tester.handle.shutdown_after_with_return_time(
        Duration::from_secs(3600),
        "Scheduled maintenance",
        Some(expected_return),
    );

    let after = read_one(&session, VariableId::Server_EstimatedReturnTime.into()).await;
    match after {
        Some(Variant::DateTime(dt)) => assert_eq!(*dt, expected_return),
        other => panic!("EstimatedReturnTime = {other:?}, expected Some(DateTime(..))"),
    }
}
