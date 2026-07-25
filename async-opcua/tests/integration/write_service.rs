//! Write service structure integration tests — OPC UA Part 4 v1.05 §5.11.4.

use std::time::Duration;

use opcua::{
    nodes::VariableBuilder,
    server::{
        diagnostics::NamespaceMetadata,
        node_manager::memory::{simple_node_manager, SimpleNodeManager},
    },
    types::{
        AttributeId, DataTypeId, DataValue, EUInformation, ExtensionObject, LocalizedText, NodeId,
        NumericRange, QualifiedName, ReadValueId, StatusCode, TimestampsToReturn, Variant,
        WriteValue,
    },
};

use crate::utils::{default_server, Tester};

const NAMESPACE_URI: &str = "urn:async-opcua:integration:write-service";

// CU 2203: a structured value must survive the complete Write/Read service path.
#[tokio::test]
async fn write_extension_object_round_trips_through_read_service() {
    // Given a SimpleNodeManager with a client-writable BaseDataType variable.
    let server = default_server().with_node_manager(simple_node_manager(
        NamespaceMetadata {
            namespace_uri: NAMESPACE_URI.to_owned(),
            ..Default::default()
        },
        "write-service",
    ));
    let mut tester = Tester::new(server, false).await;
    let node_manager = tester
        .handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .expect("SimpleNodeManager should be installed");
    let namespace_index = tester
        .handle
        .get_namespace_index(NAMESPACE_URI)
        .expect("test namespace should be registered");
    let node_id = NodeId::new(namespace_index, "StructuredValue");

    {
        let address_space = node_manager.address_space().write();
        VariableBuilder::new(
            &node_id,
            QualifiedName::new(namespace_index, "StructuredValue"),
            "StructuredValue",
        )
        .data_type(DataTypeId::BaseDataType)
        .value("initial")
        .writable()
        .insert(&*address_space);
    }

    let (session, event_loop) = tester.connect_default().await.unwrap();
    event_loop.spawn();
    tokio::time::timeout(Duration::from_secs(2), session.wait_for_connection())
        .await
        .unwrap();

    let expected = EUInformation {
        namespace_uri: "urn:async-opcua:units".into(),
        unit_id: 4408652,
        display_name: LocalizedText::new("en", "m/s"),
        description: LocalizedText::new("en", "metres per second"),
    };
    let extension_object = ExtensionObject::from_message(expected.clone());
    let expected_type_id = extension_object.binary_type_id();

    // When the ExtensionObject is written through the Write service.
    let write_results = session
        .write(&[WriteValue {
            node_id: node_id.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
            value: DataValue::new_now(Variant::ExtensionObject(extension_object)),
        }])
        .await
        .unwrap();
    assert_eq!(write_results, vec![StatusCode::Good]);

    // Then the Read service returns the same structured body and encoding type.
    let read_values = session
        .read(
            &[ReadValueId::new_value(node_id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(read_values.len(), 1);
    assert_eq!(read_values[0].status, Some(StatusCode::Good));

    let Some(Variant::ExtensionObject(actual_extension_object)) = &read_values[0].value else {
        panic!(
            "expected ExtensionObject from Read service, got {:?}",
            read_values[0].value
        );
    };
    assert_eq!(actual_extension_object.binary_type_id(), expected_type_id);

    let actual = actual_extension_object
        .inner_as::<EUInformation>()
        .expect("ExtensionObject body should decode as EUInformation");
    assert_eq!(actual.namespace_uri, expected.namespace_uri);
    assert_eq!(actual.unit_id, expected.unit_id);
    assert_eq!(actual.display_name, expected.display_name);
    assert_eq!(actual.description, expected.description);
}
