//! Read service integration tests — OPC UA Part 4 v1.05 §5.11.2 (Attribute Service Set / Read).

use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use crate::utils::{
    client_user_token, default_client, default_server, test_node_manager, ChannelNotifications,
    Tester,
};

use super::utils::{array_value, read_value_id, read_value_ids, setup};
use chrono::TimeDelta;
use opcua::{
    client::{HistoryReadAction, Session},
    server::address_space::{
        AccessLevel, DataTypeBuilder, EventNotifier, MethodBuilder, ObjectBuilder,
        ObjectTypeBuilder, ReferenceTypeBuilder, VariableBuilder, VariableTypeBuilder, ViewBuilder,
    },
    types::{
        AggregateConfiguration, AttributeId, DataTypeId, DataValue, DateTime, HistoryData,
        HistoryReadValueId, LocalizedText, NodeClass, NodeId, ObjectId, ObjectTypeId,
        QualifiedName, ReadProcessedDetails, ReadRawModifiedDetails, ReadValueId, ReferenceTypeId,
        SessionDiagnosticsDataType, StatusCode, SubscriptionDiagnosticsDataType,
        TimestampsToReturn, VariableId, VariableTypeId, Variant, WriteMask, WriteValue,
    },
};
use opcua_client::{services::Read, DefaultRetryPolicy, ExponentialBackoff, UARequest};
use opcua_types::NumericRange;

#[tokio::test]
async fn read() {
    let (tester, _nm, session) = setup().await;

    // Read the service level, plus the ServerArray, which must contain this
    // server's application URI at index 0 (OPC UA Part 5, 6.3.1).
    tester.handle.set_service_level(123);
    let r = session
        .read(
            &[
                read_value_id(AttributeId::Value, VariableId::Server_ServiceLevel),
                read_value_id(AttributeId::Value, VariableId::Server_ServerArray),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(2, r.len());
    assert_eq!(&Variant::Byte(123), r[0].value.as_ref().unwrap());
    assert_eq!(Some(StatusCode::Good), r[1].status);
    assert_eq!(
        array_value(&r[1]),
        &vec![Variant::String("urn:integration_server".into())]
    );
}

#[tokio::test]
async fn read_variable() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .historizing(true)
            .array_dimensions(&[2])
            .value(vec![1, 2])
            .description("Description")
            .value_rank(1)
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

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Value,
                    AttributeId::Historizing,
                    AttributeId::ArrayDimensions,
                    AttributeId::Description,
                    AttributeId::ValueRank,
                    AttributeId::DataType,
                    AttributeId::AccessLevel,
                    AttributeId::UserAccessLevel,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        array_value(&r[0]),
        &vec![Variant::Int32(1), Variant::Int32(2)]
    );
    assert_eq!(r[1].value, Some(Variant::Boolean(true)));
    assert_eq!(array_value(&r[2]), &vec![Variant::UInt32(2)]);
    assert_eq!(
        r[3].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(r[4].value, Some(Variant::Int32(1)));
    assert_eq!(
        r[5].value,
        Some(Variant::NodeId(Box::new(DataTypeId::Int32.into())))
    );
    assert_eq!(r[6].value, Some(Variant::Byte(1)));
    assert_eq!(r[7].value, Some(Variant::Byte(1)));
    assert_eq!(
        r[8].value,
        Some(Variant::LocalizedText(Box::new("TestVar1".into())))
    );
    assert_eq!(
        r[9].value,
        Some(Variant::QualifiedName(Box::new("TestVar1".into())))
    );
    assert_eq!(
        r[10].value,
        Some(Variant::Int32(NodeClass::Variable as i32))
    );
    assert_eq!(r[11].value, Some(Variant::NodeId(Box::new(id))));
}

#[tokio::test]
async fn localized_text_read_locale() {
    // T072a — OPC UA Part 4 §5.4: ActivateSession localeIds apply to the Session and guide the
    // language selected for LocalizedText returned by services such as Read.
    let server = default_server()
        .locale_ids(vec!["en-US".to_string(), "de-DE".to_string()])
        .with_node_manager(test_node_manager());
    let client =
        default_client(0, false).preferred_locales(vec!["de-DE".to_string(), "en-US".to_string()]);
    let mut tester = Tester::new_custom_client(server, client).await;
    let nm = tester
        .handle
        .node_managers()
        .get_of_type::<crate::utils::TestNodeManager>()
        .unwrap();
    let (session, lp) = tester.connect_default().await.unwrap();
    lp.spawn();
    tokio::time::timeout(Duration::from_secs(2), session.wait_for_connection())
        .await
        .unwrap();

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "LocalizedReadLocale", "Initial")
            .write_mask(WriteMask::DISPLAY_NAME)
            .data_type(DataTypeId::String)
            .value("value")
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let writes = [
        localized_text_write_value(&id, LocalizedText::new("de-DE", "Deutsch")),
        localized_text_write_value(&id, LocalizedText::new("en-US", "English")),
    ];
    let statuses = session.write(&writes).await.unwrap();
    assert_eq!(statuses, vec![StatusCode::Good, StatusCode::Good]);

    let values = session
        .read(
            &[read_value_id(AttributeId::DisplayName, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert!(
        values[0].status.is_none() || values[0].status == Some(StatusCode::Good),
        "DisplayName read should succeed, got {:?}",
        values[0].status
    );
    assert_eq!(
        values[0].value,
        Some(Variant::LocalizedText(Box::new(LocalizedText::new(
            "de-DE", "Deutsch"
        )))),
        "Read must select the German DisplayName because the session localeIds prefer de-DE"
    );
}

#[tokio::test]
async fn unsupported_special_locale_write_is_rejected() {
    // T072b — OPC UA Part 4 §5.4 forbids special "mul" or "qst" locales in Write, and
    // §5.11.4 requires Bad_LocaleNotSupported for unsupported write locales.
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "LocalizedWriteLocale", "English")
            .write_mask(WriteMask::DISPLAY_NAME)
            .data_type(DataTypeId::String)
            .value("value")
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let statuses = session
        .write(&[localized_text_write_value(
            &id,
            LocalizedText::new("mul", "Multiple languages"),
        )])
        .await
        .unwrap();
    assert_eq!(statuses, vec![StatusCode::BadLocaleNotSupported]);

    let values = session
        .read(
            &[read_value_id(AttributeId::DisplayName, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(
        values[0].value,
        Some(Variant::LocalizedText(Box::new(LocalizedText::from(
            "English"
        )))),
        "Rejected special-locale write must leave DisplayName unchanged"
    );
}

fn localized_text_write_value(node_id: &NodeId, text: LocalizedText) -> WriteValue {
    WriteValue {
        value: DataValue {
            value: Some(Variant::LocalizedText(Box::new(text))),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::now()),
            ..Default::default()
        },
        node_id: node_id.clone(),
        attribute_id: AttributeId::DisplayName as u32,
        index_range: NumericRange::None,
    }
}

#[tokio::test]
async fn read_object() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ObjectBuilder::new(&id, "TestObj1", "TestObj1")
            .description("Description")
            .event_notifier(EventNotifier::SUBSCRIBE_TO_EVENTS)
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&ObjectTypeId::FolderType.into()),
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::EventNotifier,
                    AttributeId::WriteMask,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new("TestObj1".into())))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new("TestObj1".into())))
    );
    assert_eq!(r[3].value, Some(Variant::Int32(NodeClass::Object as i32)));
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::Byte(EventNotifier::SUBSCRIBE_TO_EVENTS.bits()))
    );
    assert_eq!(
        r[6].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
}

#[tokio::test]
async fn read_view() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ViewBuilder::new(&id, "TestView1", "TestView1")
            .description("Description")
            .event_notifier(EventNotifier::SUBSCRIBE_TO_EVENTS)
            .contains_no_loops(true)
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::EventNotifier,
                    AttributeId::WriteMask,
                    AttributeId::ContainsNoLoops,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new("TestView1".into())))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new("TestView1".into())))
    );
    assert_eq!(r[3].value, Some(Variant::Int32(NodeClass::View as i32)));
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::Byte(EventNotifier::SUBSCRIBE_TO_EVENTS.bits()))
    );
    assert_eq!(
        r[6].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
    assert_eq!(r[7].value, Some(Variant::Boolean(true)));
}

#[tokio::test]
async fn read_method() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        MethodBuilder::new(&id, "TestMethod1", "TestMethod1")
            .description("Description")
            .user_executable(false)
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        None,
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::WriteMask,
                    AttributeId::Executable,
                    AttributeId::UserExecutable,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new("TestMethod1".into())))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new("TestMethod1".into())))
    );
    assert_eq!(r[3].value, Some(Variant::Int32(NodeClass::Method as i32)));
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
    assert_eq!(r[6].value, Some(Variant::Boolean(true)));
    assert_eq!(r[7].value, Some(Variant::Boolean(false)));
}

#[tokio::test]
async fn read_object_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ObjectTypeBuilder::new(&id, "TestObjectType1", "TestObjectType1")
            .description("Description")
            .is_abstract(true)
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &ObjectTypeId::BaseObjectType.into(),
        &ReferenceTypeId::HasSubtype.into(),
        None,
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::WriteMask,
                    AttributeId::IsAbstract,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new("TestObjectType1".into())))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new("TestObjectType1".into())))
    );
    assert_eq!(
        r[3].value,
        Some(Variant::Int32(NodeClass::ObjectType as i32))
    );
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
    assert_eq!(r[6].value, Some(Variant::Boolean(true)));
}

#[tokio::test]
async fn read_variable_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableTypeBuilder::new(&id, "TestVariableType1", "TestVariableType1")
            .description("Description")
            .is_abstract(true)
            .data_type(DataTypeId::Int32)
            .array_dimensions(&[2])
            .value(vec![1, 2])
            .value_rank(1)
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &ObjectTypeId::BaseObjectType.into(),
        &ReferenceTypeId::HasSubtype.into(),
        None,
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::WriteMask,
                    AttributeId::IsAbstract,
                    AttributeId::DataType,
                    AttributeId::ArrayDimensions,
                    AttributeId::ValueRank,
                    AttributeId::Value,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new("TestVariableType1".into())))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new("TestVariableType1".into())))
    );
    assert_eq!(
        r[3].value,
        Some(Variant::Int32(NodeClass::VariableType as i32))
    );
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
    assert_eq!(r[6].value, Some(Variant::Boolean(true)));
    assert_eq!(
        r[7].value,
        Some(Variant::NodeId(Box::new(DataTypeId::Int32.into())))
    );
    assert_eq!(array_value(&r[8]), &vec![Variant::UInt32(2)]);
    assert_eq!(r[9].value, Some(Variant::Int32(1)));
    assert_eq!(
        array_value(&r[10]),
        &vec![Variant::Int32(1), Variant::Int32(2)]
    );
}

#[tokio::test]
async fn read_data_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        DataTypeBuilder::new(&id, "TestDataType1", "TestDataType1")
            .description("Description")
            .is_abstract(true)
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &DataTypeId::BaseDataType.into(),
        &ReferenceTypeId::HasSubtype.into(),
        None,
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::WriteMask,
                    AttributeId::IsAbstract,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new("TestDataType1".into())))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new("TestDataType1".into())))
    );
    assert_eq!(r[3].value, Some(Variant::Int32(NodeClass::DataType as i32)));
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
    assert_eq!(r[6].value, Some(Variant::Boolean(true)));
}

#[tokio::test]
async fn read_reference_type() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        ReferenceTypeBuilder::new(&id, "TestReferenceType1", "TestReferenceType1")
            .description("Description")
            .is_abstract(true)
            .symmetric(true)
            .inverse_name("Inverse")
            .write_mask(WriteMask::DISPLAY_NAME)
            .build()
            .into(),
        &ReferenceTypeId::References.into(),
        &ReferenceTypeId::HasSubtype.into(),
        None,
        Vec::new(),
    );

    let r = session
        .read(
            &read_value_ids(
                &[
                    AttributeId::Description,
                    AttributeId::DisplayName,
                    AttributeId::BrowseName,
                    AttributeId::NodeClass,
                    AttributeId::NodeId,
                    AttributeId::WriteMask,
                    AttributeId::IsAbstract,
                    AttributeId::Symmetric,
                    AttributeId::InverseName,
                ],
                &id,
            ),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("Description".into())))
    );
    assert_eq!(
        r[1].value,
        Some(Variant::LocalizedText(Box::new(
            "TestReferenceType1".into()
        )))
    );
    assert_eq!(
        r[2].value,
        Some(Variant::QualifiedName(Box::new(
            "TestReferenceType1".into()
        )))
    );
    assert_eq!(
        r[3].value,
        Some(Variant::Int32(NodeClass::ReferenceType as i32))
    );
    assert_eq!(r[4].value, Some(Variant::NodeId(Box::new(id))));
    assert_eq!(
        r[5].value,
        Some(Variant::UInt32(WriteMask::DISPLAY_NAME.bits()))
    );
    assert_eq!(r[6].value, Some(Variant::Boolean(true)));
    assert_eq!(r[7].value, Some(Variant::Boolean(true)));
    assert_eq!(
        r[8].value,
        Some(Variant::LocalizedText(Box::new("Inverse".into())))
    );
}

#[tokio::test]
async fn read_mixed() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .historizing(true)
            .value("value")
            .description("Description")
            .value_rank(1)
            .data_type(DataTypeId::String)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Read from various nodes in different node managers
    tester.handle.set_service_level(200);
    let r = session
        .read(
            &[
                read_value_id(AttributeId::DisplayName, &id),
                read_value_id(AttributeId::Value, &id),
                read_value_id(AttributeId::Value, VariableId::Server_ServiceLevel),
                read_value_id(AttributeId::DisplayName, ObjectId::Server),
                // Wrong attribute
                read_value_id(AttributeId::Value, ObjectId::Server),
                // Invalid node, valid namespace
                read_value_id(AttributeId::Value, nm.inner().next_node_id()),
                // Invalid namespace
                read_value_id(AttributeId::Value, NodeId::new(100, 1)),
                // Index range on non-value
                ReadValueId {
                    node_id: VariableId::Server_ServiceLevel.into(),
                    attribute_id: AttributeId::Value as u32,
                    index_range: opcua_types::NumericRange::Index(1),
                    ..Default::default()
                },
                // Invalid encoding
                ReadValueId {
                    node_id: VariableId::Server_ServiceLevel.into(),
                    attribute_id: AttributeId::Value as u32,
                    data_encoding: QualifiedName::from("foo"),
                    ..Default::default()
                },
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(
        r[0].value,
        Some(Variant::LocalizedText(Box::new("TestVar1".into())))
    );
    assert_eq!(r[1].value, Some(Variant::String("value".into())));
    assert_eq!(r[2].value, Some(Variant::Byte(200)));
    assert_eq!(
        r[3].value,
        Some(Variant::LocalizedText(Box::new("Server".into())))
    );
    assert_eq!(r[4].status, Some(StatusCode::BadAttributeIdInvalid));
    assert_eq!(r[4].value, None);
    assert_eq!(r[5].status, Some(StatusCode::BadNodeIdUnknown));
    assert_eq!(r[5].value, None);
    assert_eq!(r[6].status, Some(StatusCode::BadNodeIdUnknown));
    assert_eq!(r[6].value, None);
    assert_eq!(r[7].status, Some(StatusCode::BadIndexRangeDataMismatch));
    assert_eq!(r[7].value, None);
    assert_eq!(r[8].status, Some(StatusCode::BadDataEncodingInvalid));
    assert_eq!(r[8].value, None);
}

#[tokio::test]
async fn read_null_node_id_is_invalid_operation_node_id() {
    // OPC UA Part 4 §5.11.2.4 Table 49: Read operation-level results include
    // Bad_NodeIdInvalid for a NodeId that is not valid for the operation.
    let (_tester, _nm, session) = setup().await;

    let r = session
        .read(
            &[read_value_id(AttributeId::Value, NodeId::null())],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(r.len(), 1);
    assert_eq!(r[0].status, Some(StatusCode::BadNodeIdInvalid));
    assert_eq!(r[0].value, None);
}

#[tokio::test]
async fn read_limits() {
    let (tester, _nm, session) = setup().await;

    let read_limit = tester
        .handle
        .info()
        .config
        .limits
        .operational
        .max_nodes_per_read;

    // Read zero
    let r = session
        .read(&[], TimestampsToReturn::Both, 0.0)
        .await
        .unwrap_err();
    assert_eq!(r.status(), StatusCode::BadNothingToDo);

    // Invalid max age
    let r = session
        .read(
            &[read_value_id(AttributeId::DisplayName, ObjectId::Server)],
            TimestampsToReturn::Both,
            -15.0,
        )
        .await
        .unwrap_err();
    assert_eq!(r.status(), StatusCode::BadMaxAgeInvalid);

    // Invalid timestamps to return
    let r = session
        .read(
            &[read_value_id(AttributeId::DisplayName, ObjectId::Server)],
            TimestampsToReturn::Invalid,
            0.0,
        )
        .await
        .unwrap_err();
    assert_eq!(r.status(), StatusCode::BadTimestampsToReturnInvalid);

    // Too many operations
    let ops: Vec<_> = (0..(read_limit + 1))
        .map(|r| read_value_id(AttributeId::Value, NodeId::new(2, r as u32)))
        .collect();
    let r = session
        .read(&ops, TimestampsToReturn::Both, 0.0)
        .await
        .unwrap_err();
    assert_eq!(r.status(), StatusCode::BadTooManyOperations);

    // Exact number of operations, should not fail, though the reads will probably fail, mostly.
    let ops: Vec<_> = (0..read_limit)
        .map(|r| read_value_id(AttributeId::Value, NodeId::new(2, r as u32)))
        .collect();
    session
        .read(&ops, TimestampsToReturn::Both, 0.0)
        .await
        .unwrap();
}

#[tokio::test]
async fn history_read_raw() {
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
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::HISTORY_READ)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::HISTORY_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let start = DateTime::now() - TimeDelta::try_seconds(1000).unwrap();

    nm.inner().add_history(
        &id,
        (0..1000).map(|v| DataValue {
            value: Some((v as i32).into()),
            status: Some(StatusCode::Good),
            source_timestamp: Some(start + TimeDelta::try_seconds(v).unwrap()),
            server_timestamp: Some(start + TimeDelta::try_seconds(v).unwrap()),
            ..Default::default()
        }),
    );

    let action = HistoryReadAction::ReadRawModifiedDetails(ReadRawModifiedDetails {
        is_read_modified: false,
        start_time: start,
        end_time: start + TimeDelta::try_seconds(2000).unwrap(),
        num_values_per_node: 100,
        return_bounds: false,
    });

    // Read up to 100, should get the 100 first.
    let r = session
        .history_read(
            action.clone(),
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

    assert_eq!(r.len(), 1);
    let v = &r[0];
    assert!(!v.continuation_point.is_null());
    assert_eq!(v.status_code, StatusCode::Good);
    let mut data = v
        .history_data
        .inner_as::<HistoryData>()
        .unwrap()
        .data_values
        .clone()
        .unwrap();

    assert_eq!(data.len(), 100);

    let mut cp = v.continuation_point.clone();

    // Read the 100 next in a loop until we reach the end.
    for i in 0..9 {
        let r = session
            .history_read(
                action.clone(),
                TimestampsToReturn::Both,
                false,
                &[HistoryReadValueId {
                    node_id: id.clone(),
                    index_range: Default::default(),
                    data_encoding: Default::default(),
                    continuation_point: cp,
                }],
            )
            .await
            .unwrap();

        assert_eq!(r.len(), 1);
        let v = &r[0];
        if i == 8 {
            assert!(v.continuation_point.is_null());
        } else {
            assert!(!v.continuation_point.is_null(), "Expected cp for i = {i}");
        }
        assert_eq!(v.status_code, StatusCode::Good);
        let next_data = v
            .history_data
            .inner_as::<HistoryData>()
            .unwrap()
            .data_values
            .as_ref()
            .unwrap();

        assert_eq!(next_data.len(), 100);
        data.extend(next_data.iter().cloned());

        cp = v.continuation_point.clone();
    }

    // Data should be from 0 to 999, with the correct timestamps
    // This part is more a test of the test node manager,
    // but it's good to verify that continuation points work as expected.
    assert_eq!(1000, data.len());
    for (idx, it) in data.into_iter().enumerate() {
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
async fn history_read_release_continuation_points() {
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
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::HISTORY_READ)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::HISTORY_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let start = DateTime::now() - TimeDelta::try_seconds(1000).unwrap();

    nm.inner().add_history(
        &id,
        (0..1000).map(|v| DataValue {
            value: Some((v as i32).into()),
            status: Some(StatusCode::Good),
            source_timestamp: Some(start + TimeDelta::try_seconds(v).unwrap()),
            server_timestamp: Some(start + TimeDelta::try_seconds(v).unwrap()),
            ..Default::default()
        }),
    );

    let action = HistoryReadAction::ReadRawModifiedDetails(ReadRawModifiedDetails {
        is_read_modified: false,
        start_time: start,
        end_time: start + TimeDelta::try_seconds(2000).unwrap(),
        num_values_per_node: 100,
        return_bounds: false,
    });

    let r = session
        .history_read(
            action.clone(),
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

    assert_eq!(r.len(), 1);
    let v = &r[0];
    assert!(!v.continuation_point.is_null());
    assert_eq!(v.status_code, StatusCode::Good);
    let data = v
        .history_data
        .inner_as::<HistoryData>()
        .unwrap()
        .data_values
        .as_ref()
        .unwrap();

    assert_eq!(data.len(), 100);

    let cp = v.continuation_point.clone();

    let r = session
        .history_read(
            action,
            TimestampsToReturn::Both,
            true,
            &[HistoryReadValueId {
                node_id: id.clone(),
                index_range: Default::default(),
                data_encoding: Default::default(),
                continuation_point: cp,
            }],
        )
        .await
        .unwrap();

    assert_eq!(r.len(), 1);
    let v = &r[0];
    assert!(v.continuation_point.is_null());
    assert_eq!(v.status_code, StatusCode::Good);
    assert!(v.history_data.is_null());
}

#[tokio::test]
async fn history_read_fail() {
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

    let start = DateTime::now() - TimeDelta::try_seconds(1000).unwrap();

    let action = HistoryReadAction::ReadRawModifiedDetails(ReadRawModifiedDetails {
        is_read_modified: false,
        start_time: start,
        end_time: start + TimeDelta::try_seconds(2000).unwrap(),
        num_values_per_node: 100,
        return_bounds: false,
    });

    // Read nothing
    let r = session
        .history_read(action.clone(), TimestampsToReturn::Both, false, &[])
        .await
        .unwrap_err();
    assert_eq!(r.status(), StatusCode::BadNothingToDo);

    let history_read_limit = tester
        .handle
        .info()
        .config
        .limits
        .operational
        .max_nodes_per_history_read_data;

    // Read too many
    let r = session
        .history_read(
            action.clone(),
            TimestampsToReturn::Both,
            false,
            &(0..(history_read_limit + 1))
                .map(|i| HistoryReadValueId {
                    node_id: NodeId::new(2, i as u32),
                    index_range: Default::default(),
                    data_encoding: Default::default(),
                    continuation_point: Default::default(),
                })
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap_err();
    assert_eq!(r.status(), StatusCode::BadTooManyOperations);

    // Read without access
    let r = session
        .history_read(
            action.clone(),
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

    assert_eq!(r[0].status_code, StatusCode::BadUserAccessDenied);

    // Read node that doesn't exist
    let r = session
        .history_read(
            action,
            TimestampsToReturn::Both,
            false,
            &[HistoryReadValueId {
                node_id: NodeId::new(2, 100),
                index_range: Default::default(),
                data_encoding: Default::default(),
                continuation_point: Default::default(),
            }],
        )
        .await
        .unwrap();

    assert_eq!(r[0].status_code, StatusCode::BadNodeIdUnknown);
}

#[tokio::test]
async fn read_retry() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .historizing(true)
            .value(1)
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
    // Tell the node manager to fail fatally three times.
    nm.inner().issues().fatal_read.store(3, Ordering::Relaxed);

    // Make one request first, it should fail
    let e = session
        .read(
            &[ReadValueId {
                node_id: id.clone(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap_err();
    assert_eq!(e.status(), StatusCode::BadInternalError);

    // Use the retry method to send the remaining requests, two more will fail, then
    // the request will succeed.
    let r = session
        .send_with_retry(
            Read::new(&session).node(ReadValueId {
                node_id: id,
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            }),
            DefaultRetryPolicy::new(ExponentialBackoff::new(
                Duration::from_millis(1000),
                Some(3),
                Duration::from_millis(50),
            )),
        )
        .await
        .unwrap();

    assert_eq!(
        r.results.unwrap_or_default().first().unwrap().value,
        Some(Variant::Int32(1))
    );
}

#[tokio::test]
async fn test_diagnostics() {
    let server = default_server().diagnostics_enabled(true);
    let mut tester = Tester::new(server, false).await;
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

    // Make a request that should fail
    session
        .read(
            &[read_value_id(AttributeId::DisplayName, ObjectId::Server)],
            TimestampsToReturn::Both,
            -15.0,
        )
        .await
        .unwrap_err();

    // Fetch a few diagnostics
    let diagnostics = session
        .read(
            &[ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_ServerDiagnosticsSummary_CumulatedSessionCount
                    .into(),
            ), ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_ServerDiagnosticsSummary_CurrentSessionCount
                    .into(),
            ), ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_ServerDiagnosticsSummary_RejectedRequestsCount
                    .into(),
            ), ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_ServerDiagnosticsSummary_SecurityRejectedRequestsCount
                    .into(),
            )],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    assert_eq!(4, diagnostics.len());
    assert_eq!(diagnostics[0].value, Some(Variant::UInt32(1)));
    assert_eq!(diagnostics[1].value, Some(Variant::UInt32(1)));
    assert_eq!(diagnostics[2].value, Some(Variant::UInt32(1)));
    assert_eq!(diagnostics[3].value, Some(Variant::UInt32(0)));
}

#[tokio::test]
async fn test_read_timeout() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "TestVar1", "TestVar1")
            .value(1)
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

    // First, read normally.
    let r = session
        .read(
            &read_value_ids(&[AttributeId::Value], &id),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(r[0].value, Some(Variant::Int32(1)));

    // Next, make read slow, and set a timeout on the client shorter than the time it takes to read, so that the request will timeout.
    nm.inner().issues().slow_read.store(true, Ordering::Relaxed);
    let r = Read::new(&session)
        .node(read_value_id(AttributeId::Value, id.clone()))
        .timeout(Duration::from_millis(100))
        .send(session.channel())
        .await
        .unwrap_err();
    assert!(matches!(r.status(), StatusCode::BadTimeout));

    // Now, wait for a bit, so that the client receives the late response from the server.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Send another request. It shouldn't fail.
    session
        .read(
            &read_value_ids(&[AttributeId::Value], &id),
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn read_invalid_index_range_is_per_operation() {
    // P4-ATTR-01 — OPC UA Part 4 §5.11.2 Table 49: a malformed indexRange must yield operation-level
    // Bad_IndexRangeInvalid for THAT node only; the rest of the batch must still succeed. A malformed
    // range must NOT fail the whole Read (which previously aborted the batch with BadDecodingError).
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "StrVar", "StrVar")
            .value("value")
            .value_rank(-1)
            .data_type(DataTypeId::String)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let r = session
        .read(
            &[
                // Valid plain read.
                read_value_id(AttributeId::Value, &id),
                // Malformed indexRange -> per-node BadIndexRangeInvalid (not a whole-batch fault).
                ReadValueId {
                    node_id: id.clone(),
                    attribute_id: AttributeId::Value as u32,
                    index_range: opcua_types::NumericRange::Invalid("not-a-range".into()),
                    ..Default::default()
                },
                // Another valid read -> proves the bad range did not poison the batch.
                read_value_id(AttributeId::DisplayName, &id),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        // The whole Read must succeed (no ServiceFault) -- the bad range is per-operation.
        .unwrap();

    assert!(
        r[0].value.is_some(),
        "valid read before the bad range must succeed"
    );
    assert_eq!(r[1].status, Some(StatusCode::BadIndexRangeInvalid));
    assert_eq!(r[1].value, None);
    assert!(
        r[2].value.is_some(),
        "valid read after the bad range must succeed"
    );
}

#[tokio::test]
async fn cancel_returns_cancel_count_zero() {
    // P4-SESS-01 — OPC UA Part 4 §5.7.5: Cancel is a supported Session service. With no cancellable
    // outstanding requests it returns Good with cancelCount = 0 (not BadServiceUnsupported).
    let (_tester, _nm, session) = setup().await;
    let count = session.cancel(1).await.unwrap();
    assert_eq!(0, count);
}

#[tokio::test]
async fn server_capabilities_locale_id_array_is_populated() {
    // P5-01 — OPC UA Part 5 §6.3.2: ServerCapabilities.LocaleIdArray is a mandatory property and must
    // reflect the server's configured locales, not a static empty array.
    let (_tester, _nm, session) = setup().await;
    let r = session
        .read(
            &[ReadValueId {
                node_id: VariableId::Server_ServerCapabilities_LocaleIdArray.into(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::Array(arr)) = r[0].value.clone() else {
        panic!(
            "LocaleIdArray must be a populated array, got {:?}",
            r[0].value
        );
    };
    assert!(
        !arr.values.is_empty(),
        "LocaleIdArray must reflect configured locales, not be empty"
    );
    assert!(
        arr.values
            .iter()
            .any(|x| matches!(x, Variant::String(s) if s.as_ref() == "en")),
        "LocaleIdArray should contain the configured locale 'en', got {:?}",
        arr.values
    );
}

#[tokio::test]
async fn server_array_is_populated_string_array() {
    // P5-03 — OPC UA Part 5 §5.3.2: Server_ServerArray (i=2254) is a mandatory String[] listing the
    // Server URIs in this server (at minimum its own ApplicationUri). It must be a non-null, non-empty
    // array: the OPC UA .NET reference stack validates the DataType of ServerArray during session
    // setup and rejects a null/scalar value with Bad_TypeMismatch (found via the .NET interop harness).
    let (_tester, _nm, session) = setup().await;
    let r = session
        .read(
            &[ReadValueId {
                node_id: VariableId::Server_ServerArray.into(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            }],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::Array(arr)) = r[0].value.clone() else {
        panic!(
            "ServerArray must be a populated array, got {:?}",
            r[0].value
        );
    };
    assert!(
        !arr.values.is_empty(),
        "ServerArray must list at least the server's own URI"
    );
    assert!(
        arr.values.iter().all(|x| matches!(x, Variant::String(_))),
        "ServerArray entries must be Strings, got {:?}",
        arr.values
    );
}

#[tokio::test]
async fn min_supported_sample_rate_value_matches_duration_datatype() {
    // P5-02 — OPC UA Part 5 §6.3.2: MinSupportedSampleRate is a Duration (Double). The exposed Value
    // must therefore be a Double, matching the node's DataType attribute (not a UInt32).
    let (_tester, _nm, session) = setup().await;
    let r = session
        .read(
            &[
                ReadValueId {
                    node_id:
                        opcua::types::VariableId::Server_ServerCapabilities_MinSupportedSampleRate
                            .into(),
                    attribute_id: AttributeId::Value as u32,
                    ..Default::default()
                },
                ReadValueId {
                    node_id:
                        opcua::types::VariableId::Server_ServerCapabilities_MinSupportedSampleRate
                            .into(),
                    attribute_id: AttributeId::DataType as u32,
                    ..Default::default()
                },
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    println!(
        "MinSupportedSampleRate Value={:?} DataType={:?}",
        r[0].value, r[1].value
    );
    assert!(
        matches!(r[0].value, Some(Variant::Double(_))),
        "Value must be a Double (Duration), got {:?}",
        r[0].value
    );
}

// ---------------------------------------------------------------------------
// Feature 053 US1 (P5-04): ServerDiagnosticsType mandatory children.
// OPC UA Part 5 v1.05 §6.3.3 (Table 11): EnabledFlag, SubscriptionDiagnosticsArray and
// SessionsDiagnosticsSummary (SessionDiagnosticsArray + SessionSecurityDiagnosticsArray)
// are Mandatory members of ServerDiagnosticsType and must serve live data.
// ---------------------------------------------------------------------------

async fn connect_diagnostics_tester(diagnostics: bool) -> (Tester, Arc<Session>) {
    let server = default_server().diagnostics_enabled(diagnostics);
    let mut tester = Tester::new(server, false).await;
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
    (tester, session)
}

#[tokio::test]
async fn server_diagnostics_enabled_flag_reflects_config() {
    // Part 5 §6.3.3: EnabledFlag is a mandatory Boolean Property of ServerDiagnosticsType,
    // TRUE while the server is collecting diagnostics.
    let (_tester, session) = connect_diagnostics_tester(true).await;
    let r = session
        .read(
            &[ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_EnabledFlag.into(),
            )],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(StatusCode::Good), r[0].status);
    assert_eq!(Some(Variant::Boolean(true)), r[0].value);
}

#[tokio::test]
async fn server_diagnostics_arrays_report_live_entries() {
    // Part 5 §6.3.3: SubscriptionDiagnosticsArray has one SubscriptionDiagnosticsDataType
    // entry per live subscription; SessionDiagnosticsArray one SessionDiagnosticsDataType
    // entry per live session.
    let (_tester, session) = connect_diagnostics_tester(true).await;
    let (notifs, _data, _) = ChannelNotifications::new();
    let sub_id = session
        .create_subscription(Duration::from_millis(500), 100, 20, 1000, 0, true, notifs)
        .await
        .unwrap();

    let r = session
        .read(
            &[
                ReadValueId::new_value(
                    VariableId::Server_ServerDiagnostics_SubscriptionDiagnosticsArray.into(),
                ),
                ReadValueId::new_value(
                    VariableId::Server_ServerDiagnostics_SessionsDiagnosticsSummary_SessionDiagnosticsArray.into(),
                ),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();

    let subs = array_value(&r[0]);
    assert_eq!(1, subs.len(), "one live subscription expected: {:?}", r[0]);
    let Variant::ExtensionObject(sub_diag) = &subs[0] else {
        panic!("expected ExtensionObject, got {:?}", subs[0]);
    };
    let sub_diag: &SubscriptionDiagnosticsDataType = sub_diag.inner_as().unwrap();
    assert_eq!(sub_id, sub_diag.subscription_id);
    assert_eq!(500.0, sub_diag.publishing_interval);
    assert!(sub_diag.publishing_enabled);
    assert_eq!(0, sub_diag.monitored_item_count);

    let sessions = array_value(&r[1]);
    assert_eq!(1, sessions.len(), "one live session expected: {:?}", r[1]);
    let Variant::ExtensionObject(sess_diag) = &sessions[0] else {
        panic!("expected ExtensionObject, got {:?}", sessions[0]);
    };
    let sess_diag: &SessionDiagnosticsDataType = sess_diag.inner_as().unwrap();
    assert!(!sess_diag.session_id.is_null());
    assert_eq!(1, sess_diag.current_subscriptions_count);
    // The subscription's diagnostics row must point back at this session.
    assert_eq!(sess_diag.session_id, sub_diag.session_id);
}

#[tokio::test]
async fn server_diagnostics_disabled_reads_false_and_empty_arrays() {
    // Part 5 §6.3.3: with EnabledFlag == FALSE the server is not collecting diagnostics;
    // the diagnostics arrays read as empty.
    let (_tester, session) = connect_diagnostics_tester(false).await;
    let r = session
        .read(
            &[
                ReadValueId::new_value(
                    VariableId::Server_ServerDiagnostics_EnabledFlag.into(),
                ),
                ReadValueId::new_value(
                    VariableId::Server_ServerDiagnostics_SubscriptionDiagnosticsArray.into(),
                ),
                ReadValueId::new_value(
                    VariableId::Server_ServerDiagnostics_SessionsDiagnosticsSummary_SessionDiagnosticsArray.into(),
                ),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Boolean(false)), r[0].value);
    assert!(
        array_value(&r[1]).is_empty(),
        "SubscriptionDiagnosticsArray must read empty while disabled: {:?}",
        r[1]
    );
    assert!(
        array_value(&r[2]).is_empty(),
        "SessionDiagnosticsArray must read empty while disabled: {:?}",
        r[2]
    );
}

#[tokio::test]
async fn session_security_diagnostics_requires_security_privilege() {
    // Part 5 §6.3.5/§7.15: SessionSecurityDiagnostics content is security-related and
    // must only be exposed to sessions holding the security-diagnostics privilege; the
    // ordinary read_diagnostics privilege is NOT sufficient. Fail closed.
    let (_tester, session) = connect_diagnostics_tester(true).await;
    let r = session
        .read(
            &[ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_SessionsDiagnosticsSummary_SessionSecurityDiagnosticsArray.into(),
            )],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(StatusCode::BadUserAccessDenied), r[0].status);
}

#[tokio::test]
async fn server_diagnostics_privileged_toggle_and_security_array() {
    // Part 5 §6.3.3: a session holding the diagnostics-write privilege can toggle
    // EnabledFlag (disabling collection empties the arrays); Part 5 §6.3.5/§7.15: the
    // security-diagnostics privilege exposes SessionSecurityDiagnosticsArray rows.
    use crate::utils::CLIENT_USERPASS_ID;
    use opcua::server::ServerUserToken;
    use opcua::types::SessionSecurityDiagnosticsDataType;

    let server = default_server().diagnostics_enabled(true).add_user_token(
        CLIENT_USERPASS_ID,
        ServerUserToken::user_pass(
            CLIENT_USERPASS_ID,
            &format!("{CLIENT_USERPASS_ID}_password"),
        )
        .read_diagnostics(true)
        .write_diagnostics(true)
        .read_security_diagnostics(true),
    );
    let mut tester = Tester::new(server, false).await;
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

    // Security array: one row for this session, with its actual security parameters.
    let r = session
        .read(
            &[ReadValueId::new_value(
                VariableId::Server_ServerDiagnostics_SessionsDiagnosticsSummary_SessionSecurityDiagnosticsArray.into(),
            )],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let rows = array_value(&r[0]);
    assert_eq!(1, rows.len(), "one live session expected: {:?}", r[0]);
    let Variant::ExtensionObject(row) = &rows[0] else {
        panic!("expected ExtensionObject, got {:?}", rows[0]);
    };
    let row: &SessionSecurityDiagnosticsDataType = row.inner_as().unwrap();
    assert!(!row.session_id.is_null());
    assert_eq!(
        opcua::types::MessageSecurityMode::SignAndEncrypt,
        row.security_mode
    );

    // Privileged EnabledFlag write: disable, observe empty arrays + false, re-enable.
    let enabled_flag: NodeId = VariableId::Server_ServerDiagnostics_EnabledFlag.into();
    let write = |value: bool| WriteValue {
        node_id: enabled_flag.clone(),
        attribute_id: AttributeId::Value as u32,
        index_range: NumericRange::None,
        value: DataValue {
            value: Some(Variant::Boolean(value)),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::now()),
            ..Default::default()
        },
    };
    let statuses = session.write(&[write(false)]).await.unwrap();
    assert_eq!(vec![StatusCode::Good], statuses);

    let r = session
        .read(
            &[
                ReadValueId::new_value(enabled_flag.clone()),
                ReadValueId::new_value(
                    VariableId::Server_ServerDiagnostics_SubscriptionDiagnosticsArray.into(),
                ),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Boolean(false)), r[0].value);
    assert!(
        array_value(&r[1]).is_empty(),
        "arrays must read empty after EnabledFlag=false: {:?}",
        r[1]
    );

    let statuses = session.write(&[write(true)]).await.unwrap();
    assert_eq!(vec![StatusCode::Good], statuses);
    let r = session
        .read(
            &[ReadValueId::new_value(enabled_flag.clone())],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Boolean(true)), r[0].value);
}

// ---------------------------------------------------------------------------
// Feature 053 US3 (P4-ATTR-03): LocalizedText attribute write locale rules.
// OPC UA Part 4 §5.11.4.1: null text + locale deletes that locale's entry;
// null locale + text sets the invariant (default) text WITHOUT discarding the
// stored locale entries; null + null deletes the entries for all locales;
// unsupported locale → Bad_LocaleNotSupported.
// ---------------------------------------------------------------------------

async fn locale_test_setup() -> (
    Tester,
    Arc<crate::utils::TestNodeManager>,
    Arc<Session>,
    NodeId,
) {
    let server = default_server()
        .locale_ids(vec!["en-US".to_string(), "de-DE".to_string()])
        .with_node_manager(test_node_manager());
    let client =
        default_client(0, false).preferred_locales(vec!["de-DE".to_string(), "en-US".to_string()]);
    let mut tester = Tester::new_custom_client(server, client).await;
    let nm = tester
        .handle
        .node_managers()
        .get_of_type::<crate::utils::TestNodeManager>()
        .unwrap();
    let (session, lp) = tester.connect_default().await.unwrap();
    lp.spawn();
    tokio::time::timeout(Duration::from_secs(2), session.wait_for_connection())
        .await
        .unwrap();

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "LocaleRules", "Initial")
            .write_mask(WriteMask::DISPLAY_NAME)
            .data_type(DataTypeId::String)
            .value("value")
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Seed two locales.
    let writes = [
        localized_text_write_value(&id, LocalizedText::new("de-DE", "Deutsch")),
        localized_text_write_value(&id, LocalizedText::new("en-US", "English")),
    ];
    let statuses = session.write(&writes).await.unwrap();
    assert_eq!(statuses, vec![StatusCode::Good, StatusCode::Good]);

    (tester, nm, session, id)
}

async fn read_display_name(session: &Session, id: &NodeId) -> LocalizedText {
    let values = session
        .read(
            &[read_value_id(AttributeId::DisplayName, id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    match &values[0].value {
        Some(Variant::LocalizedText(t)) => t.as_ref().clone(),
        other => panic!("expected LocalizedText, got {other:?}"),
    }
}

#[tokio::test]
async fn localized_text_null_text_deletes_locale() {
    // §5.11.4.1: "Writing a null String for the text for a locale shall delete the
    // String for that locale." Other locales must be retained.
    let (_tester, _nm, session, id) = locale_test_setup().await;

    // The de-DE-preferring session currently reads German.
    assert_eq!(
        LocalizedText::new("de-DE", "Deutsch"),
        read_display_name(&session, &id).await
    );

    // Delete the de-DE entry (null text + locale).
    let statuses = session
        .write(&[localized_text_write_value(
            &id,
            LocalizedText::new("de-DE", ""),
        )])
        .await
        .unwrap();
    assert_eq!(statuses, vec![StatusCode::Good]);

    // de-DE is gone; the session's next preference (en-US) is served, proving the
    // en-US entry survived the deletion.
    assert_eq!(
        LocalizedText::new("en-US", "English"),
        read_display_name(&session, &id).await
    );
}

#[tokio::test]
async fn localized_text_invariant_write_keeps_locales() {
    // §5.11.4.1: "Writing a null String for the locale and a non-null String for the
    // text is setting the text for an invariant locale" — the per-locale entries are
    // NOT discarded by this.
    let (_tester, _nm, session, id) = locale_test_setup().await;

    let statuses = session
        .write(&[localized_text_write_value(
            &id,
            LocalizedText::new("", "Invariant"),
        )])
        .await
        .unwrap();
    assert_eq!(statuses, vec![StatusCode::Good]);

    // The de-DE-preferring session still reads its locale — entries survived.
    assert_eq!(
        LocalizedText::new("de-DE", "Deutsch"),
        read_display_name(&session, &id).await
    );

    // §5.11.4.1: null text + null locale deletes the entries for ALL locales; the
    // invariant text remains the served fallback.
    let statuses = session
        .write(&[localized_text_write_value(&id, LocalizedText::new("", ""))])
        .await
        .unwrap();
    assert_eq!(statuses, vec![StatusCode::Good]);

    let after = read_display_name(&session, &id).await;
    assert_ne!("Deutsch", after.text.as_ref());
    assert_ne!("English", after.text.as_ref());
}

#[tokio::test]
async fn localized_text_unsupported_locale_rejected_and_store_unchanged() {
    // §5.11.4.1: a syntactically valid but unsupported locale → Bad_LocaleNotSupported,
    // and the stored texts are unchanged.
    let (_tester, _nm, session, id) = locale_test_setup().await;

    let statuses = session
        .write(&[localized_text_write_value(
            &id,
            LocalizedText::new("xx-XX", "Mystery"),
        )])
        .await
        .unwrap();
    assert_eq!(statuses, vec![StatusCode::BadLocaleNotSupported]);

    assert_eq!(
        LocalizedText::new("de-DE", "Deutsch"),
        read_display_name(&session, &id).await
    );
}

#[tokio::test]
async fn localized_text_value_attribute_keeps_single_locale_semantics() {
    // §5.11.4.1 leaves Value-attribute LocalizedText behavior server specific; this
    // server keeps whole-value single-locale semantics (last write wins) — lock-in.
    let (tester, nm, session) = setup().await;
    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "LtValue", "LtValue")
            .data_type(DataTypeId::LocalizedText)
            .value(LocalizedText::new("en-US", "English"))
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
            value: Some(Variant::LocalizedText(Box::new(LocalizedText::new(
                "de-DE", "Deutsch",
            )))),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::now()),
            ..Default::default()
        },
        node_id: id.clone(),
        attribute_id: AttributeId::Value as u32,
        index_range: NumericRange::None,
    };
    let statuses = session.write(&[wv]).await.unwrap();
    assert_eq!(statuses, vec![StatusCode::Good]);

    let values = session
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    // The whole value is the last write — no per-locale merge on Value attributes.
    assert_eq!(
        Some(Variant::LocalizedText(Box::new(LocalizedText::new(
            "de-DE", "Deutsch"
        )))),
        values[0].value
    );
}

// ---------------------------------------------------------------------------
// Feature 053 US4 (P4-ATTR-02): Read maxAge semantics.
// OPC UA Part 4 §5.11.2.2 (Table 47): maxAge = 0 → the server shall attempt to
// read a new value from the data source; maxAge >= max Int32 → it shall attempt
// a cached value; negative values are invalid. Verified architecture (053
// research R4): read callbacks ARE the data source and are invoked on every
// Read with the client's exact maxAge (fresh ≥ any freshness bound the client
// can request), and plain in-memory values are their own source. These tests
// lock that contract in, including the non-finite edge cases.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_max_age_reaches_refreshable_sources() {
    use opcua::server::node_manager::memory::{simple_node_manager, SimpleNodeManager};
    use std::sync::Mutex;

    let namespace = opcua::server::diagnostics::NamespaceMetadata {
        namespace_uri: "urn:rustopcuatestserver".to_owned(),
        namespace_index: 2,
        ..Default::default()
    };
    let server = default_server().with_node_manager(simple_node_manager(namespace, "test"));
    let mut tester = Tester::new(server, false).await;
    let nm = tester
        .handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .expect("SimpleNodeManager not found");
    let (session, lp) = tester.connect_default().await.unwrap();
    lp.spawn();
    tokio::time::timeout(Duration::from_secs(2), session.wait_for_connection())
        .await
        .unwrap();

    let id = NodeId::new(2, "max_age_source");
    {
        let space = nm.address_space().write();
        let var = VariableBuilder::new(&id, "MaxAgeSource", "MaxAgeSource")
            .value(0i32)
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .build();
        space.insert(var, None::<&[(_, &NodeId, _)]>);
    }

    let seen: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_cb = seen.clone();
    nm.inner()
        .add_read_callback(id.clone(), move |_range, _ts, max_age| {
            let mut seen = seen_cb.lock().unwrap();
            seen.push(max_age);
            // A fresh source read every invocation.
            Ok(DataValue::new_now(seen.len() as i32))
        });

    // maxAge = 0: the server shall attempt a fresh source read → callback invoked.
    let r = session
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Int32(1)), r[0].value);

    // Mid-range maxAge is delivered verbatim to the source seam.
    let r = session
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            12345.5,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Int32(2)), r[0].value);

    // maxAge >= max Int32: a cached value is permitted; serving a fresh one
    // satisfies any freshness bound. The parameter still reaches the source.
    let r = session
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            i32::MAX as f64,
        )
        .await
        .unwrap();
    assert_eq!(Some(Variant::Int32(3)), r[0].value);

    let seen = seen.lock().unwrap().clone();
    assert_eq!(vec![0.0, 12345.5, i32::MAX as f64], seen);
}

#[tokio::test]
async fn read_max_age_non_finite_does_not_panic() {
    // Part 4 §5.11.2.2 defines maxAge as a Duration; negative is invalid
    // (Bad_MaxAgeInvalid — existing behavior, regression-guarded here) and
    // non-finite values must not crash the server. NaN fails no `< 0` guard, so
    // the documented interpretation is: treated like any huge maxAge (cached
    // permitted), never an error, never a panic.
    let (tester, nm, session) = setup().await;
    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "MaxAgeEdge", "MaxAgeEdge")
            .value(7i32)
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

    // Negative → whole-service Bad_MaxAgeInvalid (existing contract).
    let err = session
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            -1.0,
        )
        .await
        .unwrap_err();
    assert_eq!(StatusCode::BadMaxAgeInvalid, err.status());

    // NaN and +infinity: served without error (cached-permitted semantics).
    for max_age in [f64::NAN, f64::INFINITY] {
        let r = session
            .read(
                &[read_value_id(AttributeId::Value, &id)],
                TimestampsToReturn::Both,
                max_age,
            )
            .await
            .unwrap();
        assert_eq!(Some(Variant::Int32(7)), r[0].value, "maxAge={max_age}");
    }
}

// ---------------------------------------------------------------------------
// Feature 053 US6 (P3-09): AccessLevelEx optional Variable attribute.
// OPC UA Part 3 §5.6.2: AccessLevelEx (attribute id 27, AccessLevelExType,
// UInt32) — the low 8 bits are identical to AccessLevel; extended bits add
// nonatomicity/write-full-array-only/etc. Part 3 §8.60: WriteMask bit 25.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn access_level_ex_low_byte_mirrors_access_level() {
    let (tester, nm, session) = setup().await;
    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "AlxVar", "AlxVar")
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
        .read(
            &[
                read_value_id(AttributeId::AccessLevelEx, &id),
                read_value_id(AttributeId::AccessLevel, &id),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::UInt32(alx)) = r[0].value else {
        panic!("AccessLevelEx must read as UInt32, got {:?}", r[0]);
    };
    let Some(Variant::Byte(al)) = r[1].value else {
        panic!("AccessLevel must read as Byte, got {:?}", r[1]);
    };
    assert_eq!(
        al as u32,
        alx & 0xFF,
        "Part 3 §5.6.2: the low byte of AccessLevelEx is identical to AccessLevel"
    );

    // The attribute also mirrors on a standard ns=0 Variable.
    let r = session
        .read(
            &[
                read_value_id(AttributeId::AccessLevelEx, VariableId::Server_ServiceLevel),
                read_value_id(AttributeId::AccessLevel, VariableId::Server_ServiceLevel),
            ],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::UInt32(alx)) = r[0].value else {
        panic!("AccessLevelEx must read as UInt32, got {:?}", r[0]);
    };
    let Some(Variant::Byte(al)) = r[1].value else {
        panic!("AccessLevel must read as Byte, got {:?}", r[1]);
    };
    assert_eq!(al as u32, alx & 0xFF);
}

#[tokio::test]
async fn access_level_ex_invalid_on_non_variables() {
    // AccessLevelEx exists only on Variables (Part 3 §5.9 attribute summary).
    let (_tester, _nm, session) = setup().await;
    let r = session
        .read(
            &[read_value_id(
                AttributeId::AccessLevelEx,
                NodeId::from(ObjectId::Server),
            )],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(Some(StatusCode::BadAttributeIdInvalid), r[0].status);
}

#[tokio::test]
async fn access_level_ex_extended_bits_configurable_and_writable() {
    use opcua::types::AccessLevelExType;

    let (tester, nm, session) = setup().await;
    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "AlxExt", "AlxExt")
            .value(1i32)
            .data_type(DataTypeId::Int32)
            .access_level(AccessLevel::CURRENT_READ)
            .user_access_level(AccessLevel::CURRENT_READ)
            .access_level_ex(AccessLevelExType::NonatomicRead)
            .write_mask(WriteMask::ACCESS_LEVEL_EX)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Configured extended bit is served; low byte still mirrors AccessLevel.
    let r = session
        .read(
            &[read_value_id(AttributeId::AccessLevelEx, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::UInt32(alx)) = r[0].value else {
        panic!("expected UInt32, got {:?}", r[0]);
    };
    assert_ne!(
        0,
        alx & AccessLevelExType::NonatomicRead.bits() as u32,
        "configured extended bit must be set: {alx:#x}"
    );
    assert_eq!(AccessLevel::CURRENT_READ.bits() as u32, alx & 0xFF);

    // WriteMask bit 25 permits writing the attribute (Part 3 §8.60).
    let wv = WriteValue {
        node_id: id.clone(),
        attribute_id: AttributeId::AccessLevelEx as u32,
        index_range: NumericRange::None,
        value: DataValue {
            value: Some(Variant::UInt32(
                AccessLevelExType::WriteFullArrayOnly.bits() as u32
                    | AccessLevel::CURRENT_READ.bits() as u32,
            )),
            status: Some(StatusCode::Good),
            source_timestamp: Some(DateTime::now()),
            ..Default::default()
        },
    };
    let statuses = session.write(&[wv]).await.unwrap();
    assert_eq!(vec![StatusCode::Good], statuses);

    let r = session
        .read(
            &[read_value_id(AttributeId::AccessLevelEx, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    let Some(Variant::UInt32(alx)) = r[0].value else {
        panic!("expected UInt32, got {:?}", r[0]);
    };
    assert_ne!(0, alx & AccessLevelExType::WriteFullArrayOnly.bits() as u32);
    assert_eq!(
        0,
        alx & AccessLevelExType::NonatomicRead.bits() as u32,
        "write replaced the extended bits"
    );
}

#[tokio::test]
async fn history_read_processed_returns_history_data_values() {
    use opcua::server::{
        diagnostics::NamespaceMetadata,
        node_manager::memory::{simple_node_manager, SimpleNodeManager},
        InMemoryDataHistory,
    };
    use opcua::types::PerformUpdateType;

    // Install a SimpleNodeManager with the in-memory data history backend so that
    // ReadProcessedDetails is served (the default test node manager has no history backend,
    // so it would return BadHistoryOperationUnsupported).
    let namespace = NamespaceMetadata {
        namespace_uri: "urn:rustopcuatestserver".to_owned(),
        namespace_index: 2,
        ..Default::default()
    };
    let server = default_server().with_node_manager(simple_node_manager(namespace, "test"));
    let mut tester = Tester::new(server, false).await;
    let nm = tester
        .handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .expect("SimpleNodeManager not found");
    nm.inner()
        .set_history_backend(Arc::new(InMemoryDataHistory::new()));
    let (session, lp) = tester.connect_default().await.unwrap();
    lp.spawn();
    tokio::time::timeout(Duration::from_secs(2), session.wait_for_connection())
        .await
        .unwrap();

    let id = NodeId::new(2, "ProcessedVar");
    {
        let space = nm.address_space().write();
        let var = VariableBuilder::new(&id, "ProcessedVar", "ProcessedVar")
            .data_type(DataTypeId::Double)
            .historizing(true)
            .value(0.0f64)
            .user_access_level(
                AccessLevel::HISTORY_READ | AccessLevel::HISTORY_WRITE | AccessLevel::CURRENT_READ,
            )
            .access_level(
                AccessLevel::HISTORY_READ | AccessLevel::HISTORY_WRITE | AccessLevel::CURRENT_READ,
            )
            .build();
        space.insert(var, None::<&[(_, &NodeId, _)]>);
    }

    let start = DateTime::now() - TimeDelta::try_seconds(30).unwrap();
    let values: Vec<DataValue> = [0.0f64, 10.0, 20.0]
        .into_iter()
        .enumerate()
        .map(|(idx, value)| {
            let timestamp = start + TimeDelta::try_seconds((idx as i64) * 10).unwrap();
            DataValue {
                value: Some(Variant::Double(value)),
                status: Some(StatusCode::Good),
                source_timestamp: Some(timestamp),
                server_timestamp: Some(timestamp),
                ..Default::default()
            }
        })
        .collect();

    // Insert values through the backend-supported path (HistoryUpdate) so the
    // in-memory history backend can serve ReadProcessed.
    let update_results = session
        .history_update_data(id.clone(), PerformUpdateType::Insert, values)
        .await
        .unwrap();
    for status in update_results {
        assert!(
            status.is_good() || status == StatusCode::GoodEntryInserted,
            "history update failed: {status}"
        );
    }

    let results = session
        .history_read(
            HistoryReadAction::ReadProcessedDetails(ReadProcessedDetails {
                start_time: start,
                end_time: start + TimeDelta::try_seconds(20).unwrap(),
                processing_interval: 20_000.0,
                aggregate_type: Some(vec![NodeId::new(0u16, 2343u32)]),
                aggregate_configuration: AggregateConfiguration::default(),
            }),
            TimestampsToReturn::Both,
            false,
            &[HistoryReadValueId {
                node_id: id,
                index_range: Default::default(),
                data_encoding: Default::default(),
                continuation_point: Default::default(),
            }],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status_code, StatusCode::Good);
    let history_data = results[0]
        .history_data
        .inner_as::<HistoryData>()
        .expect("processed HistoryRead should return HistoryData");
    let data_values = history_data
        .data_values
        .as_ref()
        .expect("processed HistoryRead should include DataValues");
    assert_eq!(data_values.len(), 1);
    assert_eq!(data_values[0].status, Some(StatusCode::Good));
    assert!(data_values[0].value.is_some());
}
