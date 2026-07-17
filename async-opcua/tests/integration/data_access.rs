//! Feature 100 — Data Access conformance completion: TwoStateDiscreteType, MultiStateDiscreteType,
//! MultiStateValueDiscreteType (OPC-10000-8 §5.3.3.2-5.3.3.4), and the ArrayItemType family --
//! YArrayItemType, XYArrayItemType, ImageItemType, CubeItemType, NDimensionArrayItemType
//! (OPC-10000-8 §5.3.4.2-5.3.4.6).

use super::utils::setup;
use opcua::server::address_space::ObjectBuilder;
use opcua::server::data_access::{
    create_cube_item_variable, create_image_item_variable, create_multi_state_discrete_variable,
    create_multi_state_value_discrete_variable, create_nd_dimension_array_item_variable,
    create_two_state_discrete_variable, create_xy_array_item_variable,
    create_y_array_item_variable, update_multi_state_value_discrete, ArrayItemBaseProperties,
};
use opcua::types::{
    Array, AttributeId, AxisInformation, AxisScaleEnumeration, DataTypeId, EUInformation,
    EnumValueType, LocalizedText, NodeId, ObjectId, Range, ReadValueId, TimestampsToReturn,
    VariantScalarTypeId, XVType,
};
use opcua_types::Variant;

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

fn array_item_base_properties() -> ArrayItemBaseProperties {
    ArrayItemBaseProperties {
        eu_range: Range {
            low: -90.0,
            high: 5.0,
        },
        engineering_units: EUInformation {
            namespace_uri: "http://www.opcfoundation.org/UA/units/un/cefact".into(),
            unit_id: 12878,
            display_name: LocalizedText::new("en", "dB"),
            description: LocalizedText::new("en", "decibel"),
        },
        title: LocalizedText::new("en", "Magnitude"),
        axis_scale_type: AxisScaleEnumeration::Linear,
    }
}

fn x_axis_definition() -> AxisInformation {
    AxisInformation {
        engineering_units: EUInformation {
            namespace_uri: "http://www.opcfoundation.org/UA/units/un/cefact".into(),
            unit_id: 4933722,
            display_name: LocalizedText::new("en", "kHz"),
            description: LocalizedText::new("en", "kilohertz"),
        },
        eu_range: Range {
            low: 0.0,
            high: 25.0,
        },
        title: LocalizedText::new("en", "Frequency"),
        axis_scale_type: AxisScaleEnumeration::Linear,
        axis_steps: None,
    }
}

// ---------------------------------------------------------------------------
// CU 2361 (+ 2426 byproduct): TwoStateDiscreteType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_state_discrete_exposes_true_false_states_and_value() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "TwoStateParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "TwoStateParent", "TwoStateParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_two_state_discrete_variable(
            space,
            2,
            "Valve",
            parent_id,
            true,
            LocalizedText::new("en", "OPEN"),
            LocalizedText::new("en", "CLOSED"),
        );
    }

    assert_eq!(
        read_one(&session, value_id.clone()).await,
        Some(Variant::Boolean(true))
    );
    assert_eq!(
        read_one(&session, NodeId::new(2, "Valve_TrueState")).await,
        Some(Variant::from(LocalizedText::new("en", "OPEN")))
    );
    assert_eq!(
        read_one(&session, NodeId::new(2, "Valve_FalseState")).await,
        Some(Variant::from(LocalizedText::new("en", "CLOSED")))
    );
}

// ---------------------------------------------------------------------------
// CU 2988: MultiStateDiscreteType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multi_state_discrete_exposes_enum_strings_and_value() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "MultiStateParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "MultiStateParent", "MultiStateParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_multi_state_discrete_variable(
            space,
            2,
            "Door",
            parent_id,
            1,
            array_variant(
                VariantScalarTypeId::LocalizedText,
                vec![
                    Variant::from(LocalizedText::new("en", "OPEN")),
                    Variant::from(LocalizedText::new("en", "CLOSE")),
                    Variant::from(LocalizedText::new("en", "IN TRANSIT")),
                ],
            ),
        );
    }

    assert_eq!(
        read_one(&session, value_id.clone()).await,
        Some(Variant::UInt32(1))
    );
    let Some(Variant::Array(arr)) = read_one(&session, NodeId::new(2, "Door_EnumStrings")).await
    else {
        panic!("EnumStrings should read back as an array");
    };
    let strings: Vec<_> = arr
        .values
        .iter()
        .map(|v| match v {
            Variant::LocalizedText(t) => t.text.as_ref().to_owned(),
            other => panic!("expected LocalizedText entry, got {other:?}"),
        })
        .collect();
    assert_eq!(strings, vec!["OPEN", "CLOSE", "IN TRANSIT"]);
}

// ---------------------------------------------------------------------------
// CU 2831: MultiStateValueDiscreteType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multi_state_value_discrete_tracks_non_contiguous_enum_values() {
    let (_tester, nm, session) = setup().await;

    let enum_values = vec![
        EnumValueType {
            value: 1,
            display_name: LocalizedText::new("en", "Low"),
            description: LocalizedText::null(),
        },
        EnumValueType {
            value: 4,
            display_name: LocalizedText::new("en", "Medium"),
            description: LocalizedText::null(),
        },
        EnumValueType {
            value: 8,
            display_name: LocalizedText::new("en", "High"),
            description: LocalizedText::null(),
        },
    ];

    let parent_id = NodeId::new(2, "MultiStateValueParent");
    let handle;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "MultiStateValueParent", "MultiStateValueParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        handle = create_multi_state_value_discrete_variable(
            space,
            2,
            "Level",
            parent_id,
            1,
            &enum_values,
        );
    }

    assert_eq!(
        read_one(&session, handle.value_id.clone()).await,
        Some(Variant::Int64(1))
    );
    assert_eq!(
        read_one(&session, handle.value_as_text_id.clone()).await,
        Some(Variant::from(LocalizedText::new("en", "Low")))
    );

    {
        let guard = nm.address_space().write();
        update_multi_state_value_discrete(&guard, &handle, &enum_values, 8);
    }
    assert_eq!(
        read_one(&session, handle.value_id.clone()).await,
        Some(Variant::Int64(8))
    );
    assert_eq!(
        read_one(&session, handle.value_as_text_id.clone()).await,
        Some(Variant::from(LocalizedText::new("en", "High")))
    );

    // A value with no matching EnumValues entry -> ValueAsText reads back null
    // (OPC-10000-8 §5.3.3.4).
    {
        let guard = nm.address_space().write();
        update_multi_state_value_discrete(&guard, &handle, &enum_values, 99);
    }
    assert_eq!(
        read_one(&session, handle.value_as_text_id).await,
        Some(Variant::Empty)
    );
}

// ---------------------------------------------------------------------------
// CU 3323: YArrayItemType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn y_array_item_exposes_spectrum_and_x_axis_definition() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "YArrayParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "YArrayParent", "YArrayParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_y_array_item_variable(
            space,
            2,
            "Spectrum",
            parent_id,
            DataTypeId::Double.into(),
            array_variant(
                VariantScalarTypeId::Double,
                vec![
                    Variant::from(-90.0),
                    Variant::from(-45.0),
                    Variant::from(2.0),
                ],
            ),
            array_item_base_properties(),
            x_axis_definition(),
        );
    }

    let Some(Variant::Array(arr)) = read_one(&session, value_id).await else {
        panic!("YArrayItem value should read back as an array");
    };
    assert_eq!(arr.values.len(), 3);

    let value = read_one(&session, NodeId::new(2, "Spectrum_EURange")).await;
    let Some(Variant::ExtensionObject(eo)) = value else {
        panic!("EURange should be an ExtensionObject, got {value:?}");
    };
    let range = eo.inner_as::<Range>().expect("should decode as Range");
    assert_eq!(range.low, -90.0);
    assert_eq!(range.high, 5.0);

    let value = read_one(&session, NodeId::new(2, "Spectrum_XAxisDefinition")).await;
    let Some(Variant::ExtensionObject(eo)) = value else {
        panic!("XAxisDefinition should be an ExtensionObject, got {value:?}");
    };
    let axis = eo
        .inner_as::<AxisInformation>()
        .expect("should decode as AxisInformation");
    assert_eq!(axis.title, LocalizedText::new("en", "Frequency"));
    assert_eq!(axis.eu_range.high, 25.0);
}

// ---------------------------------------------------------------------------
// CU 3324: XYArrayItemType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn xy_array_item_exposes_xv_type_peaks() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "XYArrayParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "XYArrayParent", "XYArrayParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_xy_array_item_variable(
            space,
            2,
            "Peaks",
            parent_id,
            array_variant(
                VariantScalarTypeId::ExtensionObject,
                vec![
                    Variant::from(opcua_types::ExtensionObject::from_message(XVType {
                        x: 1.0,
                        value: 10.0,
                    })),
                    Variant::from(opcua_types::ExtensionObject::from_message(XVType {
                        x: 2.5,
                        value: 20.0,
                    })),
                ],
            ),
            array_item_base_properties(),
            x_axis_definition(),
        );
    }

    let Some(Variant::Array(arr)) = read_one(&session, value_id).await else {
        panic!("XYArrayItem value should read back as an array");
    };
    assert_eq!(arr.values.len(), 2);
    let Variant::ExtensionObject(eo) = &arr.values[1] else {
        panic!("expected ExtensionObject entries");
    };
    let peak = eo.inner_as::<XVType>().expect("should decode as XVType");
    assert_eq!(peak.x, 2.5);
    assert_eq!(peak.value, 20.0);
}

// ---------------------------------------------------------------------------
// CU 3325: ImageItemType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn image_item_exposes_2d_matrix_and_both_axis_definitions() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "ImageParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "ImageParent", "ImageParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_image_item_variable(
            space,
            2,
            "Frame",
            parent_id,
            DataTypeId::Byte.into(),
            array_variant(
                VariantScalarTypeId::Byte,
                (0u8..6).map(Variant::from).collect(),
            ),
            3,
            2,
            array_item_base_properties(),
            x_axis_definition(),
            x_axis_definition(),
        );
    }

    let read = session
        .read(
            &[ReadValueId {
                node_id: value_id,
                attribute_id: AttributeId::ArrayDimensions as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(read[0].value, Some(Variant::from(vec![3u32, 2u32])));

    for prop in ["Frame_XAxisDefinition", "Frame_YAxisDefinition"] {
        let value = read_one(&session, NodeId::new(2, prop)).await;
        assert!(
            matches!(value, Some(Variant::ExtensionObject(_))),
            "{prop} should be an ExtensionObject, got {value:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// CU 3326: CubeItemType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cube_item_exposes_3d_volume_and_all_three_axis_definitions() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "CubeParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "CubeParent", "CubeParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_cube_item_variable(
            space,
            2,
            "Volume",
            parent_id,
            DataTypeId::Byte.into(),
            array_variant(
                VariantScalarTypeId::Byte,
                (0u8..8).map(Variant::from).collect(),
            ),
            [2, 2, 2],
            array_item_base_properties(),
            x_axis_definition(),
            x_axis_definition(),
            x_axis_definition(),
        );
    }

    let read = session
        .read(
            &[ReadValueId {
                node_id: value_id,
                attribute_id: AttributeId::ArrayDimensions as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(read[0].value, Some(Variant::from(vec![2u32, 2u32, 2u32])));

    for prop in [
        "Volume_XAxisDefinition",
        "Volume_YAxisDefinition",
        "Volume_ZAxisDefinition",
    ] {
        let value = read_one(&session, NodeId::new(2, prop)).await;
        assert!(
            matches!(value, Some(Variant::ExtensionObject(_))),
            "{prop} should be an ExtensionObject, got {value:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// CU 3327: NDimensionArrayItemType.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nd_dimension_array_item_exposes_one_axis_definition_per_dimension() {
    let (_tester, nm, session) = setup().await;

    let parent_id = NodeId::new(2, "NDArrayParent");
    let value_id;
    {
        let mut guard = nm.address_space().write();
        let space = &mut *guard;
        ObjectBuilder::new(&parent_id, "NDArrayParent", "NDArrayParent")
            .organized_by(ObjectId::ObjectsFolder)
            .insert(space);

        value_id = create_nd_dimension_array_item_variable(
            space,
            2,
            "Tensor",
            parent_id,
            DataTypeId::Byte.into(),
            Variant::Array(Box::new(
                Array::new_multi(
                    VariantScalarTypeId::Byte,
                    (0u8..16).map(Variant::from).collect::<Vec<_>>(),
                    vec![2u32, 2, 2, 2],
                )
                .expect("multi-dimensional array should be well-formed"),
            )),
            &[2, 2, 2, 2],
            array_item_base_properties(),
            vec![
                x_axis_definition(),
                x_axis_definition(),
                x_axis_definition(),
                x_axis_definition(),
            ],
        );
    }

    let Some(Variant::Array(arr)) =
        read_one(&session, NodeId::new(2, "Tensor_AxisDefinition")).await
    else {
        panic!("AxisDefinition should read back as an array");
    };
    assert_eq!(arr.values.len(), 4);
    for entry in &arr.values {
        let Variant::ExtensionObject(eo) = entry else {
            panic!("expected ExtensionObject entries");
        };
        eo.inner_as::<AxisInformation>()
            .expect("should decode as AxisInformation");
    }

    let read = session
        .read(
            &[ReadValueId {
                node_id: value_id,
                attribute_id: AttributeId::ArrayDimensions as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(
        read[0].value,
        Some(Variant::from(vec![2u32, 2u32, 2u32, 2u32]))
    );
}
