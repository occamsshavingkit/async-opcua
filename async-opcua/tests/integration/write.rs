//! Write + HistoryUpdate integration tests — OPC UA Part 4 v1.05 §5.11.4 (Attribute Service Set /
//! Write); HistoryUpdate is §5.11.5.

use chrono::TimeDelta;
use opcua::{
    client::{HistoryReadAction, HistoryUpdateAction, Session},
    server::address_space::{
        AccessLevel, DataTypeBuilder, EventNotifier, MethodBuilder, NodeType, ObjectBuilder,
        ObjectTypeBuilder, ReferenceTypeBuilder, VariableBuilder, VariableTypeBuilder, ViewBuilder,
    },
    types::{
        AccessLevelExType, AttributeId, ByteString, DataTypeId, DataValue, DateTime, HistoryData,
        HistoryReadValueId, LocalizedText, NodeId, ObjectId, ObjectTypeId, QualifiedName,
        ReadRawModifiedDetails, ReferenceTypeId, StatusCode, TimestampsToReturn, UpdateDataDetails,
        VariableTypeId, Variant, WriteMask, WriteValue,
    },
};
use opcua_types::NumericRange;
// Write is not implemented in the core library itself, only in the test node manager,
// we still test here to test write functionality in the address space.
use super::utils::{array_value, read_value_id, setup};

fn write_value(
    attribute_id: AttributeId,
    value: impl Into<Variant>,
    node_id: impl Into<NodeId>,
) -> WriteValue {
    WriteValue {
        value: DataValue {
            value: Some(value.into()),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::now()),
            ..Default::default()
        },
        node_id: node_id.into(),
        attribute_id: attribute_id as u32,
        index_range: NumericRange::None,
    }
}

async fn write_then_read(session: &Session, values: &[WriteValue]) {
    let r = session.write(values).await.unwrap();
    assert_eq!(r.len(), values.len());
    for s in r {
        assert_eq!(s, StatusCode::Good);
    }

    let reads: Vec<_> = values
        .iter()
        .map(|r| read_value_id(AttributeId::from_u32(r.attribute_id).unwrap(), &r.node_id))
        .collect();

    let r = session
        .read(&reads, TimestampsToReturn::Both, 0.0)
        .await
        .unwrap();

    assert_eq!(r.len(), values.len());
    for (read, write) in r.into_iter().zip(values) {
        assert_eq!(read.value, write.value.value);
    }
}

#[tokio::test]
async fn write_variable() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::ARRAY_DIMENSIONS
                    | WriteMask::VALUE_RANK
                    | WriteMask::DATA_TYPE
                    | WriteMask::ACCESS_LEVEL
                    | WriteMask::USER_ACCESS_LEVEL
                    | WriteMask::HISTORIZING,
            )
            .data_type(DataTypeId::String)
            .value("value")
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(AttributeId::DisplayName, LocalizedText::from("NewVar"), &id),
            write_value(AttributeId::BrowseName, QualifiedName::from("NewVar"), &id),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(AttributeId::ValueRank, 1, &id),
            write_value(AttributeId::ArrayDimensions, vec![2u32], &id),
            write_value(
                AttributeId::DataType,
                Variant::NodeId(Box::new(DataTypeId::Int32.into())),
                &id,
            ),
            write_value(
                AttributeId::AccessLevel,
                (AccessLevel::CURRENT_READ
                    | AccessLevel::CURRENT_WRITE
                    | AccessLevel::HISTORY_READ)
                    .bits(),
                &id,
            ),
            write_value(
                AttributeId::UserAccessLevel,
                (AccessLevel::CURRENT_READ
                    | AccessLevel::CURRENT_WRITE
                    | AccessLevel::HISTORY_READ)
                    .bits(),
                &id,
            ),
            write_value(AttributeId::Historizing, true, &id),
            write_value(AttributeId::Value, vec![1, 2], &id),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_object() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ObjectBuilder::new(&id, "TestObj1", "TestObj1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::EVENT_NOTIFIER,
            )
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&ObjectTypeId::FolderType.into()),
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(AttributeId::DisplayName, LocalizedText::from("NewObj"), &id),
            write_value(AttributeId::BrowseName, QualifiedName::from("NewObj"), &id),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(
                AttributeId::EventNotifier,
                EventNotifier::SUBSCRIBE_TO_EVENTS.bits(),
                &id,
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_view() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ViewBuilder::new(&id, "TestView1", "TestView1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::EVENT_NOTIFIER
                    | WriteMask::CONTAINS_NO_LOOPS,
            )
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(
                AttributeId::DisplayName,
                LocalizedText::from("NewView"),
                &id,
            ),
            write_value(AttributeId::BrowseName, QualifiedName::from("NewView"), &id),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(
                AttributeId::EventNotifier,
                EventNotifier::SUBSCRIBE_TO_EVENTS.bits(),
                &id,
            ),
            write_value(AttributeId::ContainsNoLoops, true, &id),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_method() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        MethodBuilder::new(&id, "TestMethod1", "TestMethod1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::EXECUTABLE
                    | WriteMask::USER_EXECUTABLE,
            )
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(
                AttributeId::DisplayName,
                LocalizedText::from("NewMethod"),
                &id,
            ),
            write_value(
                AttributeId::BrowseName,
                QualifiedName::from("NewMethod"),
                &id,
            ),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(AttributeId::Executable, true, &id),
            write_value(AttributeId::UserExecutable, true, &id),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_object_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ObjectTypeBuilder::new(&id, "TestObjectType1", "TestObjectType1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::IS_ABSTRACT,
            )
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(
                AttributeId::DisplayName,
                LocalizedText::from("NewObjectType"),
                &id,
            ),
            write_value(
                AttributeId::BrowseName,
                QualifiedName::from("NewObjectType"),
                &id,
            ),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(AttributeId::IsAbstract, true, &id),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_variable_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableTypeBuilder::new(&id, "TestVariableType1", "TestVariableType1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::IS_ABSTRACT
                    | WriteMask::DATA_TYPE
                    | WriteMask::ARRAY_DIMENSIONS
                    | WriteMask::VALUE_FOR_VARIABLE_TYPE
                    | WriteMask::VALUE_RANK,
            )
            .data_type(DataTypeId::String)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(
                AttributeId::DisplayName,
                LocalizedText::from("NewVariableType"),
                &id,
            ),
            write_value(
                AttributeId::BrowseName,
                QualifiedName::from("NewVariableType"),
                &id,
            ),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(AttributeId::IsAbstract, true, &id),
            write_value(AttributeId::ValueRank, 1, &id),
            write_value(AttributeId::ArrayDimensions, vec![2u32], &id),
            write_value(
                AttributeId::DataType,
                Variant::NodeId(Box::new(DataTypeId::Int32.into())),
                &id,
            ),
            write_value(AttributeId::Value, vec![1, 2], &id),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_data_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        DataTypeBuilder::new(&id, "TestObjectType1", "TestObjectType1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::IS_ABSTRACT,
            )
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(
                AttributeId::DisplayName,
                LocalizedText::from("NewDataType"),
                &id,
            ),
            write_value(
                AttributeId::BrowseName,
                QualifiedName::from("NewDataType"),
                &id,
            ),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(AttributeId::IsAbstract, true, &id),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_reference_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ReferenceTypeBuilder::new(&id, "TestRefType1", "TestRefType1")
            .description("Description")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::IS_ABSTRACT
                    | WriteMask::SYMMETRIC
                    | WriteMask::INVERSE_NAME,
            )
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    write_then_read(
        &session,
        &[
            write_value(
                AttributeId::DisplayName,
                LocalizedText::from("NewRefType"),
                &id,
            ),
            write_value(
                AttributeId::BrowseName,
                QualifiedName::from("NewRefType"),
                &id,
            ),
            write_value(
                AttributeId::Description,
                LocalizedText::from("Description"),
                &id,
            ),
            write_value(AttributeId::IsAbstract, true, &id),
            write_value(AttributeId::Symmetric, true, &id),
            write_value(
                AttributeId::InverseName,
                LocalizedText::from("Inverse"),
                &id,
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn write_invalid() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .write_mask(
                WriteMask::DISPLAY_NAME
                    | WriteMask::BROWSE_NAME
                    | WriteMask::DESCRIPTION
                    | WriteMask::DATA_TYPE
                    | WriteMask::HISTORIZING,
            )
            .data_type(DataTypeId::String)
            .value("value")
            .access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let r = session
        .write(&[
            // Wrong type
            write_value(AttributeId::DataType, LocalizedText::from("uhoh"), &id),
            // Not valid for variables.
            write_value(AttributeId::EventNotifier, 1, &id),
            // Not allowed
            write_value(
                AttributeId::AccessLevel,
                (AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE).bits(),
                &id,
            ),
            // Not allowed value
            write_value(AttributeId::Value, "foo", &id),
        ])
        .await
        .unwrap();

    assert_eq!(r[0], StatusCode::BadTypeMismatch);
    assert_eq!(r[1], StatusCode::BadNotWritable);
    assert_eq!(r[2], StatusCode::BadNotWritable);
    assert_eq!(r[3], StatusCode::BadUserAccessDenied);
}

#[tokio::test]
async fn write_wrong_scalar_type_is_rejected() {
    // Part 4 §5.11.4: writing a value whose data type is neither the node's data type nor a
    // subtype of it must return Bad_TypeMismatch. Previously a mismatched scalar was silently
    // accepted (only arrays were checked). Found by the node-opcua interop harness.
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TypedInt32", "TypedInt32")
            .data_type(DataTypeId::Int32)
            .value(0i32)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // A String written to an Int32 node is a type mismatch...
    let r = session
        .write(&[write_value(AttributeId::Value, "not-an-int", &id)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::BadTypeMismatch);

    // ...while a correctly-typed Int32 write succeeds.
    let r = session
        .write(&[write_value(AttributeId::Value, 42i32, &id)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good);
}

#[tokio::test]
async fn write_limits() {
    let (tester, _nm, session) = setup().await;

    let write_limit = tester
        .handle
        .info()
        .config
        .limits
        .operational
        .max_nodes_per_write;

    // Write zero. This doesn't actually reach the server, since we intercept it in the client.
    // we still protect against it on the server, but we don't have a way to bypass that check here.
    let r = session.write(&[]).await.unwrap_err();
    assert_eq!(r.status(), StatusCode::BadNothingToDo);

    // Too many operations
    let ops: Vec<_> = (0..(write_limit + 1))
        .map(|r| write_value(AttributeId::Value, 123, NodeId::new(2, r as u32)))
        .collect();

    let r = session.write(&ops).await.unwrap_err();
    assert_eq!(r.status(), StatusCode::BadTooManyOperations);

    // Exact number of operations
    let ops: Vec<_> = (0..write_limit)
        .map(|r| write_value(AttributeId::Value, 123, NodeId::new(2, r as u32)))
        .collect();

    session.write(&ops).await.unwrap();
}

#[tokio::test]
async fn write_null_node_id_is_invalid_operation_node_id() {
    // OPC UA Part 4 §5.11.4.4 Table 55: Write operation-level results include
    // Bad_NodeIdInvalid for a NodeId that is not valid for the operation.
    let (_tester, _nm, session) = setup().await;

    let r = session
        .write(&[write_value(AttributeId::Value, 123, NodeId::null())])
        .await
        .unwrap();

    assert_eq!(r, vec![StatusCode::BadNodeIdInvalid]);
}

#[tokio::test]
async fn write_bytestring_to_byte_array() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .value(vec![0u8; 16])
            .data_type(DataTypeId::Byte)
            .value_rank(1)
            .access_level(AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let bytes = ByteString::from(vec![0x1u8, 0x2u8, 0x3u8, 0x4u8]);
    let mut write = write_value(AttributeId::Value, bytes, &id);
    write.index_range = NumericRange::Range(0, 4);
    let r = session.write(&[write]).await.unwrap();
    assert_eq!(StatusCode::Good, r[0]);

    {
        let sp = nm.address_space().read();
        let node = sp.find(&id).unwrap();
        let NodeType::Variable(ref v) = *node else {
            panic!("");
        };
        let val = v.value(
            TimestampsToReturn::Both,
            &opcua::types::NumericRange::None,
            &Default::default(),
            0.0,
        );

        println!("{val:?}");

        let arr = array_value(&val);
        assert_eq!(16, arr.len());
        assert_eq!(
            &arr[0..5],
            &[
                Variant::Byte(1),
                Variant::Byte(2),
                Variant::Byte(3),
                Variant::Byte(4),
                Variant::Byte(0)
            ]
        );
    }
}

#[tokio::test]
async fn write_index_range() {
    let (tester, nm, session) = setup().await;

    let id1 = nm.inner().next_node_id();
    let id2 = nm.inner().next_node_id();
    for id in [&id1, &id2] {
        nm.inner().add_node(
            nm.address_space(),
            tester.handle.type_tree(),
            VariableBuilder::new(id, "TestVar", "TestVar")
                .value(vec![0u8; 16])
                .data_type(DataTypeId::Byte)
                .value_rank(1)
                .access_level(AccessLevel::CURRENT_WRITE)
                .user_access_level(AccessLevel::CURRENT_WRITE)
                .build()
                .into(),
            &ObjectId::ObjectsFolder.into(),
            &ReferenceTypeId::Organizes.into(),
            Some(&VariableTypeId::BaseDataVariableType.into()),
            Vec::new(),
        );
    }

    let nodes_to_write = [
        WriteValue {
            node_id: id1.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::Index(12),
            value: DataValue::new_now(vec![73u8]),
        },
        WriteValue {
            node_id: id2.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::Range(4, 12),
            value: DataValue::new_now(vec![1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8]),
        },
    ];

    let r = session.write(&nodes_to_write).await.unwrap();
    assert_eq!(r[0], StatusCode::Good);
    assert_eq!(r[1], StatusCode::Good);

    let sp = nm.address_space().read();
    // Node 1
    let node = sp.find(&id1).unwrap();
    let NodeType::Variable(ref v) = *node else {
        panic!("");
    };
    let val = v.value(
        TimestampsToReturn::Both,
        &opcua::types::NumericRange::None,
        &Default::default(),
        0.0,
    );
    let mut bytes: Vec<_> = vec![0u8; 16];
    bytes[12] = 73;
    assert_eq!(val.value.unwrap(), bytes.into());
    // Node 2
    let node = sp.find(&id2).unwrap();
    let NodeType::Variable(ref v) = *node else {
        panic!("");
    };
    let val = v.value(
        TimestampsToReturn::Both,
        &opcua::types::NumericRange::None,
        &Default::default(),
        0.0,
    );
    let mut bytes: Vec<_> = vec![0u8; 16];
    #[allow(clippy::needless_range_loop)]
    for i in 4..13 {
        bytes[i] = (i - 3) as u8;
    }
    assert_eq!(val.value.unwrap(), bytes.into());
}

// CU 2820: Part 3 §8.58 Table 42 — WriteFullArrayOnly=1 means Write of
// IndexRange is NOT supported for that Variable. Part 4 §5.11.4 Table 53 — a
// Server shall return Bad_WriteNotSupported in that case.
#[tokio::test]
async fn write_index_range_rejected_when_write_full_array_only() {
    let (tester, nm, session) = setup().await;

    let full_array_only_id = nm.inner().next_node_id();
    let normal_id = nm.inner().next_node_id();

    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&full_array_only_id, "FullArrayOnlyVar", "FullArrayOnlyVar")
            .value(vec![0u8; 4])
            .data_type(DataTypeId::Byte)
            .value_rank(1)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .access_level_ex(AccessLevelExType::WriteFullArrayOnly)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&normal_id, "NormalArrayVar", "NormalArrayVar")
            .value(vec![0u8; 4])
            .data_type(DataTypeId::Byte)
            .value_rank(1)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // An IndexRange Write to the WriteFullArrayOnly Variable is rejected...
    let r = session
        .write(&[WriteValue {
            node_id: full_array_only_id.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::Index(1),
            value: DataValue::new_now(vec![9u8]),
        }])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::BadWriteNotSupported);

    // ...and the stored value is unchanged.
    let read = session
        .read(
            &[read_value_id(AttributeId::Value, &full_array_only_id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(read[0].value, Some(vec![0u8; 4].into()));

    // A full-array (no IndexRange) Write to the same Variable still succeeds.
    let r = session
        .write(&[WriteValue {
            node_id: full_array_only_id.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
            value: DataValue::new_now(vec![1u8, 2, 3, 4]),
        }])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good);
    let read = session
        .read(
            &[read_value_id(AttributeId::Value, &full_array_only_id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(read[0].value, Some(vec![1u8, 2, 3, 4].into()));

    // A Variable WITHOUT WriteFullArrayOnly still accepts IndexRange Writes
    // (regression guard for CU 3147's existing IndexRange write support).
    let r = session
        .write(&[WriteValue {
            node_id: normal_id.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::Index(1),
            value: DataValue::new_now(vec![9u8]),
        }])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good);
}

// CU 2936: Part 4 §5.11.4 Table 53 — "If the SourceTimestamp or the
// ServerTimestamp is specified, the Server shall use these values." Proves a
// non-Good client StatusCode plus explicit, distinct timestamps round-trip
// through Write then Read (not just the value payload).
#[tokio::test]
async fn write_status_code_and_timestamps_round_trip() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "StatusTimestampVar", "StatusTimestampVar")
            .value(0i32)
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let source_timestamp = DateTime::now() - TimeDelta::try_seconds(120).unwrap();
    let server_timestamp = DateTime::now() - TimeDelta::try_seconds(60).unwrap();

    let r = session
        .write(&[WriteValue {
            node_id: id.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
            value: DataValue {
                value: Some(42i32.into()),
                status: Some(StatusCode::Uncertain),
                source_timestamp: Some(source_timestamp),
                server_timestamp: Some(server_timestamp),
                ..Default::default()
            },
        }])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good);

    let read = session
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(read[0].value, Some(42i32.into()));
    assert_eq!(read[0].status, Some(StatusCode::Uncertain));
    assert_eq!(read[0].source_timestamp, Some(source_timestamp));
    assert_eq!(read[0].server_timestamp, Some(server_timestamp));
}

// CU 4237: Part 3 §8.58 Table 42 — NonVolatile (bit 12) and Constant (bit 13)
// on AccessLevelEx. The generic AccessLevelEx bitmask plumbing already
// handles arbitrary bits; this proves it for this specific pair.
#[tokio::test]
async fn access_level_ex_non_volatile_and_constant_round_trip() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "NonVolatileConstantVar", "NonVolatileConstantVar")
            .value(1i32)
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .access_level_ex(AccessLevelExType::NonVolatile | AccessLevelExType::Constant)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let read = session
        .read(
            &[read_value_id(AttributeId::AccessLevelEx, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::UInt32(access_level_ex)) = read[0].value else {
        panic!(
            "expected UInt32 AccessLevelEx value, got {:?}",
            read[0].value
        );
    };
    assert_ne!(
        0,
        access_level_ex & AccessLevelExType::NonVolatile.bits() as u32
    );
    assert_ne!(
        0,
        access_level_ex & AccessLevelExType::Constant.bits() as u32
    );
}

#[tokio::test]
async fn history_update_insert() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .historizing(true)
            .value(0)
            .description("Description")
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::HISTORY_WRITE | AccessLevel::HISTORY_READ)
            .user_access_level(AccessLevel::HISTORY_WRITE | AccessLevel::HISTORY_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let start = DateTime::now() - TimeDelta::try_seconds(1000).unwrap();

    let action = HistoryUpdateAction::UpdateDataDetails(UpdateDataDetails {
        node_id: id.clone(),
        perform_insert_replace: opcua::types::PerformUpdateType::Insert,
        update_values: Some(
            (0..1000)
                .map(|v| DataValue {
                    value: Some((v as i32).into()),
                    status: Some(StatusCode::Good),
                    source_timestamp: Some(start + TimeDelta::try_seconds(v).unwrap()),
                    ..Default::default()
                })
                .collect(),
        ),
    });

    let results = session.history_update(&[action]).await.unwrap();
    assert_eq!(1, results.len());
    assert_eq!(StatusCode::Good, results[0].status_code);
    let res = results[0].operation_results.as_ref().unwrap();
    for s in res {
        assert_eq!(s, &StatusCode::GoodEntryInserted);
    }

    let r = session
        .history_read(
            HistoryReadAction::ReadRawModifiedDetails(ReadRawModifiedDetails {
                is_read_modified: false,
                start_time: start,
                end_time: start + TimeDelta::try_seconds(2000).unwrap(),
                num_values_per_node: 1000,
                return_bounds: false,
            }),
            TimestampsToReturn::Both,
            false,
            &[HistoryReadValueId {
                node_id: id.clone(),
                index_range: Default::default(),
                data_encoding: Default::default(),
                continuation_point: Default::default(),
            }],
        )
        .await
        .unwrap();

    let v = &r[0];
    assert!(v.continuation_point.is_null());
    assert_eq!(v.status_code, StatusCode::Good);
    let data = v
        .history_data
        .inner_as::<HistoryData>()
        .unwrap()
        .data_values
        .as_ref()
        .unwrap();

    assert_eq!(data.len(), 1000);
    for (idx, it) in data.iter().enumerate() {
        let v = match it.value.as_ref().unwrap() {
            Variant::Int32(v) => *v,
            _ => panic!("Wrong value type: {:?}", it.value),
        };
        assert_eq!(idx as i32, v);
        assert_eq!(
            it.source_timestamp,
            Some(start + TimeDelta::try_seconds(idx as i64).unwrap())
        );
    }
}

#[tokio::test]
async fn history_update_fail() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .historizing(true)
            .value(0)
            .description("Description")
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Write nothing
    let r = session.history_update(&[]).await.unwrap_err();
    assert_eq!(r.status(), StatusCode::BadNothingToDo);

    let history_update_limit = tester
        .handle
        .info()
        .config
        .limits
        .operational
        .max_nodes_per_history_update;

    // Write too many
    let r = session
        .history_update(
            &(0..(history_update_limit + 1))
                .map(|i| {
                    HistoryUpdateAction::UpdateDataDetails(UpdateDataDetails {
                        node_id: NodeId::new(2, i as u32),
                        perform_insert_replace: opcua::types::PerformUpdateType::Insert,
                        update_values: None,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap_err();

    assert_eq!(r.status(), StatusCode::BadTooManyOperations);

    // Write without access
    let r = session
        .history_update(&[HistoryUpdateAction::UpdateDataDetails(UpdateDataDetails {
            node_id: id.clone(),
            perform_insert_replace: opcua::types::PerformUpdateType::Insert,
            update_values: None,
        })])
        .await
        .unwrap();

    assert_eq!(r[0].status_code, StatusCode::BadUserAccessDenied);

    // Write node that doesn't exist
    let r = session
        .history_update(&[HistoryUpdateAction::UpdateDataDetails(UpdateDataDetails {
            node_id: NodeId::new(2, 100),
            perform_insert_replace: opcua::types::PerformUpdateType::Insert,
            update_values: None,
        })])
        .await
        .unwrap();

    assert_eq!(r[0].status_code, StatusCode::BadNodeIdUnknown);
}

#[tokio::test]
async fn write_value_rank_mismatch_is_rejected() {
    // Part 4 §5.11.4 / Part 3 §5.6: the written value's array-ness must match the node's
    // ValueRank. Previously only the data type was checked, so an array written to a scalar node
    // (or a scalar to an array node) was silently accepted. Sibling of the scalar-type bug above.
    let (tester, nm, session) = setup().await;

    let scalar = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&scalar, "ScalarI32", "ScalarI32")
            .data_type(DataTypeId::Int32)
            .value_rank(-1)
            .value(0i32)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let arr = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&arr, "ArrI32", "ArrI32")
            .data_type(DataTypeId::Int32)
            .value_rank(1)
            .value(vec![1i32, 2, 3])
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Array written to a scalar node -> rejected.
    let r = session
        .write(&[write_value(AttributeId::Value, vec![1i32, 2, 3], &scalar)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::BadTypeMismatch, "array -> scalar node");

    // Scalar written to an array node -> rejected.
    let r = session
        .write(&[write_value(AttributeId::Value, 5i32, &arr)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::BadTypeMismatch, "scalar -> array node");

    // Correctly-shaped writes still succeed.
    let r = session
        .write(&[write_value(AttributeId::Value, 7i32, &scalar)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good, "scalar -> scalar node");
    let r = session
        .write(&[write_value(AttributeId::Value, vec![9i32, 8], &arr)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good, "array -> array node");
}

#[tokio::test]
async fn write_out_of_bounds_index_range_is_rejected() {
    // Part 4 §7.22: writing to an index beyond the array's bounds returns Bad_IndexRangeNoData
    // and must not mutate the value.
    let (tester, nm, session) = setup().await;
    let arr = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&arr, "OobArr", "OobArr")
            .data_type(DataTypeId::Int32)
            .value_rank(1)
            .value(vec![1i32, 2, 3])
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let wv = WriteValue {
        value: DataValue {
            value: Some(99i32.into()),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::now()),
            ..Default::default()
        },
        node_id: arr.clone(),
        attribute_id: AttributeId::Value as u32,
        index_range: NumericRange::Index(10),
    };
    let r = session.write(&[wv]).await.unwrap();
    assert_eq!(r[0], StatusCode::BadIndexRangeNoData);

    // The array must be unchanged.
    let r = session
        .read(
            &[read_value_id(AttributeId::Value, &arr)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(
        r[0].value,
        Some(Variant::from(vec![1i32, 2, 3])),
        "out-of-bounds write must not mutate the array"
    );
}

/// Part 4/3 auditing: a Write emits an AuditWriteUpdateEventType (i=2100) from the Server node,
/// recording the written AttributeId.
#[tokio::test]
async fn write_emits_audit_event() {
    use crate::utils::ChannelNotifications;
    use opcua::types::{
        EventFilter, ExtensionObject, MonitoredItemCreateRequest, MonitoringMode,
        MonitoringParameters, ReadValueId, SimpleAttributeOperand,
    };
    use std::time::Duration;

    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "AuditedVar", "AuditedVar")
            .data_type(DataTypeId::String)
            .value("initial")
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let (notifs, _, mut events) = ChannelNotifications::new();
    let sub_id = session
        .create_subscription(Duration::from_millis(100), 100, 20, 1000, 0, true, notifs)
        .await
        .unwrap();
    let select = vec![
        SimpleAttributeOperand {
            type_definition_id: NodeId::new(0, 2041), // BaseEventType
            browse_path: Some(vec![QualifiedName::new(0, "EventType")]),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
        },
        SimpleAttributeOperand {
            type_definition_id: NodeId::new(0, 2100), // AuditWriteUpdateEventType
            browse_path: Some(vec![QualifiedName::new(0, "AttributeId")]),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
        },
    ];
    let res = session
        .create_monitored_items(
            sub_id,
            TimestampsToReturn::Both,
            vec![MonitoredItemCreateRequest {
                item_to_monitor: ReadValueId {
                    node_id: ObjectId::Server.into(),
                    attribute_id: AttributeId::EventNotifier as u32,
                    ..Default::default()
                },
                monitoring_mode: MonitoringMode::Reporting,
                requested_parameters: MonitoringParameters {
                    sampling_interval: 0.0,
                    queue_size: 10,
                    discard_oldest: true,
                    filter: ExtensionObject::new(EventFilter {
                        select_clauses: Some(select),
                        where_clause: Default::default(),
                    }),
                    ..Default::default()
                },
            }],
        )
        .await
        .unwrap();
    assert_eq!(res[0].result.status_code, StatusCode::Good);

    let r = session
        .write(&[write_value(AttributeId::Value, "audited", &id)])
        .await
        .unwrap();
    assert_eq!(r[0], StatusCode::Good);

    // AuditWriteUpdateEventType = i=2100; AttributeId::Value = 13.
    let audit_type = Variant::from(NodeId::new(0, 2100));
    let mut found = false;
    for _ in 0..5 {
        let Ok(Some((_h, v))) = tokio::time::timeout(Duration::from_secs(3), events.recv()).await
        else {
            break;
        };
        let fields = v.unwrap();
        if fields[0] == audit_type {
            assert_eq!(
                fields[1],
                Variant::from(AttributeId::Value as u32),
                "audit must carry the written AttributeId"
            );
            found = true;
            break;
        }
    }
    assert!(
        found,
        "an AuditWriteUpdateEventType must be delivered after a Write"
    );
}

#[tokio::test]
async fn server_diagnostics_enabled_flag_write_requires_privilege() {
    // Feature 053 US1 (P5-04) — OPC UA Part 5 §6.3.3: EnabledFlag toggles diagnostics
    // collection. Fail closed (constitution §IV): a session without the diagnostics-write
    // privilege gets Bad_UserAccessDenied; the ordinary read_diagnostics privilege is not
    // sufficient to write.
    use crate::utils::{client_user_token, default_server, Tester};
    use opcua::types::VariableId;
    use std::time::Duration;

    let server = default_server().diagnostics_enabled(true);
    let mut tester = Tester::new(server, false).await;

    // Anonymous session: write denied.
    let (session, lp) = tester.connect_default().await.unwrap();
    lp.spawn();
    tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
        .await
        .unwrap();
    let r = session
        .write(&[write_value(
            AttributeId::Value,
            false,
            VariableId::Server_ServerDiagnostics_EnabledFlag,
        )])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadUserAccessDenied, r[0]);

    // User with read_diagnostics (but no write privilege): write still denied.
    let (session, lp) = tester
        .connect(
            opcua_crypto::SecurityPolicy::Aes128Sha256RsaOaep,
            opcua_types::MessageSecurityMode::SignAndEncrypt,
            client_user_token(),
        )
        .await
        .unwrap();
    lp.spawn();
    tokio::time::timeout(Duration::from_secs(5), session.wait_for_connection())
        .await
        .unwrap();
    let r = session
        .write(&[write_value(
            AttributeId::Value,
            false,
            VariableId::Server_ServerDiagnostics_EnabledFlag,
        )])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadUserAccessDenied, r[0]);

    // The flag must be unchanged after the rejected writes.
    let read = session
        .read(
            &[read_value_id(
                AttributeId::Value,
                VariableId::Server_ServerDiagnostics_EnabledFlag,
            )],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Boolean(true)), read[0].value);
}

// ---------------------------------------------------------------------------
// Feature 053 US2 (P4-ATTR-04): write range/enumeration validation.
// OPC UA Part 4 §5.11.4 (Bad_OutOfRange write result); Part 8 §5.3.2.2 (EURange)
// and §5.3.3.3/§5.3.3.4 (writes of values outside a discrete item's enumeration
// should be answered with Bad_OutOfRange).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_outside_eurange_is_rejected() {
    use opcua::types::Range;

    let (tester, nm, session) = setup().await;

    // Analog scalar with EURange [0, 100].
    let var_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&var_id, "AnalogVar", "AnalogVar")
            .value(10.0f64)
            .data_type(DataTypeId::Double)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );
    let prop_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&prop_id, "EURange", "EURange")
            .value(Range {
                low: 0.0,
                high: 100.0,
            })
            .data_type(DataTypeId::Range)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &var_id,
        &ReferenceTypeId::HasProperty.into(),
        Some(&VariableTypeId::PropertyType.into()),
        Vec::new(),
    );

    // Out of range → Bad_OutOfRange, stored value unchanged.
    let r = session
        .write(&[write_value(AttributeId::Value, 150.0f64, &var_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadOutOfRange, r[0]);
    let read = session
        .read(
            &[read_value_id(AttributeId::Value, &var_id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Double(10.0)), read[0].value);

    // Below the range low bound is out of range too.
    let r = session
        .write(&[write_value(AttributeId::Value, -0.5f64, &var_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadOutOfRange, r[0]);

    // In range → Good.
    let r = session
        .write(&[write_value(AttributeId::Value, 99.5f64, &var_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::Good, r[0]);
}

#[tokio::test]
async fn write_array_element_outside_eurange_is_rejected() {
    use opcua::types::Range;

    let (tester, nm, session) = setup().await;

    let var_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&var_id, "AnalogArray", "AnalogArray")
            .value(vec![1.0f64, 2.0])
            .value_rank(1)
            .data_type(DataTypeId::Double)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );
    let prop_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&prop_id, "EURange", "EURange")
            .value(Range {
                low: 0.0,
                high: 100.0,
            })
            .data_type(DataTypeId::Range)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &var_id,
        &ReferenceTypeId::HasProperty.into(),
        Some(&VariableTypeId::PropertyType.into()),
        Vec::new(),
    );

    // Whole-array write with one element out of range → Bad_OutOfRange.
    let r = session
        .write(&[write_value(
            AttributeId::Value,
            vec![50.0f64, 500.0],
            &var_id,
        )])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadOutOfRange, r[0]);

    // Index-ranged write of an out-of-range element → Bad_OutOfRange.
    let mut wv = write_value(AttributeId::Value, vec![500.0f64], &var_id);
    wv.index_range = NumericRange::Index(1);
    let r = session.write(&[wv]).await.unwrap();
    assert_eq!(StatusCode::BadOutOfRange, r[0]);

    // Index-ranged write of an in-range element → Good.
    let mut wv = write_value(AttributeId::Value, vec![50.0f64], &var_id);
    wv.index_range = NumericRange::Index(1);
    let r = session.write(&[wv]).await.unwrap();
    assert_eq!(StatusCode::Good, r[0]);
}

#[tokio::test]
async fn write_undefined_enum_value_is_rejected() {
    use opcua::server::address_space::DataTypeBuilder;
    use opcua::types::{DataTypeDefinition, EnumDefinition, EnumField};

    let (tester, nm, session) = setup().await;

    // Custom enumeration DataType with values {0, 1, 2}.
    let enum_type_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        DataTypeBuilder::new(&enum_type_id, "TestEnum", "TestEnum")
            .data_type_definition(DataTypeDefinition::Enum(EnumDefinition {
                fields: Some(
                    [("A", 0i64), ("B", 1), ("C", 2)]
                        .iter()
                        .map(|(name, value)| EnumField {
                            value: *value,
                            display_name: (*name).into(),
                            description: Default::default(),
                            name: (*name).into(),
                        })
                        .collect(),
                ),
            }))
            .build()
            .into(),
        &DataTypeId::Enumeration.into(),
        &ReferenceTypeId::HasSubtype.into(),
        None,
        Vec::new(),
    );

    let var_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&var_id, "EnumVar", "EnumVar")
            .value(1i32)
            .data_type(&enum_type_id)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Undefined enumeration value → Bad_OutOfRange, stored value unchanged.
    let r = session
        .write(&[write_value(AttributeId::Value, 7i32, &var_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadOutOfRange, r[0]);
    let read = session
        .read(
            &[read_value_id(AttributeId::Value, &var_id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Int32(1)), read[0].value);

    // Defined value → Good.
    let r = session
        .write(&[write_value(AttributeId::Value, 2i32, &var_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::Good, r[0]);

    // Array of enum values: any undefined element → Bad_OutOfRange.
    let arr_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&arr_id, "EnumArr", "EnumArr")
            .value(vec![0i32, 1])
            .value_rank(1)
            .data_type(&enum_type_id)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );
    let r = session
        .write(&[write_value(AttributeId::Value, vec![1i32, 7], &arr_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::BadOutOfRange, r[0]);
    let r = session
        .write(&[write_value(AttributeId::Value, vec![1i32, 2], &arr_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::Good, r[0]);
}

#[tokio::test]
async fn write_unconstrained_integer_is_unaffected_by_range_validation() {
    // Regression guard: a Variable with a plain integer DataType (no enumeration
    // definition, no EURange property) accepts any type-compatible value.
    let (tester, nm, session) = setup().await;
    let var_id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&var_id, "PlainInt", "PlainInt")
            .value(1i32)
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );
    let r = session
        .write(&[write_value(AttributeId::Value, 123456i32, &var_id)])
        .await
        .unwrap();
    assert_eq!(StatusCode::Good, r[0]);
}
