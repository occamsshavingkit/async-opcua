use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use log::info;
use opcua::crypto::random;
use opcua::nodes::{
    BaseEventType, DataTypeBuilder, Event, EventField, ObjectBuilder, ObjectTypeBuilder,
    ReferenceDirection, VariableBuilder, VariableTypeBuilder,
};
use opcua::server::address_space::{EventNotifier, MethodBuilder};
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::server::node_manager::memory::{simple_node_manager, SimpleNodeManager};
use opcua::server::{ServerBuilder, ServerEndpoint, SubscriptionCache, ANONYMOUS_USER_TOKEN_ID};
use opcua::types::{
    ua_encodable, DataTypeDefinition, DataTypeId, DataValue, DateTime, ExpandedMessageInfo,
    ExpandedNodeId, MessageSecurityMode, NodeId, ObjectId, ObjectTypeId, ReferenceTypeId,
    SecurityPolicy, StatusCode, StructureDefinition, StructureField, StructureType, UAString,
    VariableTypeId,
};

const NAMESPACE_URI: &str = "https://github.com/cactuaroid/OpcUaChatServer";

const CHAT_LOG_DATA_TYPE_ID: u32 = 100;
const CHAT_LOG_ENC_TYPE_ID: u32 = 101;

#[ua_encodable]
#[derive(Debug, Clone, Default, EventField)]
struct ChatLog {
    at: DateTime,
    name: UAString,
    content: UAString,
}

impl ExpandedMessageInfo for ChatLog {
    fn full_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId {
            node_id: NodeId::new(0, CHAT_LOG_ENC_TYPE_ID),
            namespace_uri: NAMESPACE_URI.into(),
            server_index: 0,
        }
    }

    fn full_json_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId::null()
    }

    fn full_xml_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId::null()
    }

    fn full_data_type_id(&self) -> ExpandedNodeId {
        ExpandedNodeId {
            node_id: NodeId::new(0, CHAT_LOG_DATA_TYPE_ID),
            namespace_uri: NAMESPACE_URI.into(),
            server_index: 0,
        }
    }
}

#[derive(Clone, Event)]
#[opcua(
    identifier = "i=7",
    namespace = "https://github.com/cactuaroid/OpcUaChatServer"
)]
struct ChatLogEventType {
    base: BaseEventType,
    own_namespace_index: u16,
    chat_log: ChatLog,
}

impl ChatLogEventType {
    fn new(
        type_id: NodeId,
        event_id: opcua::types::ByteString,
        message: impl Into<opcua::types::LocalizedText>,
        time: DateTime,
        chat_log: ChatLog,
    ) -> Self {
        Self {
            base: BaseEventType::new(type_id, event_id, message, time)
                .set_source_node(ObjectId::Server.into())
                .set_source_name("ChatLogs".into())
                .set_severity(500),
            own_namespace_index: 1,
            chat_log,
        }
    }
}

static CHAT_LOG_TYPE_LOADER: std::sync::LazyLock<opcua::types::TypeLoaderInstance> =
    std::sync::LazyLock::new(|| {
        let mut inst = opcua::types::TypeLoaderInstance::new();
        inst.add_binary_type(
            CHAT_LOG_DATA_TYPE_ID,
            CHAT_LOG_ENC_TYPE_ID,
            opcua::types::binary_decode_to_enc::<ChatLog>,
        );
        inst
    });

#[derive(Debug, Clone, Copy)]
struct ChatTypeLoader;

impl opcua::types::StaticTypeLoader for ChatTypeLoader {
    fn instance() -> &'static opcua::types::TypeLoaderInstance {
        &CHAT_LOG_TYPE_LOADER
    }

    fn namespace() -> &'static str {
        NAMESPACE_URI
    }
}

fn register_chat_log_data_type(addr: &mut opcua::nodes::AddressSpace, ns: u16) {
    let struct_id = NodeId::new(ns, CHAT_LOG_DATA_TYPE_ID);
    let enc_id = NodeId::new(ns, CHAT_LOG_ENC_TYPE_ID);

    ObjectBuilder::new(&enc_id, "Default Binary", "Default Binary")
        .reference(
            struct_id.clone(),
            ReferenceTypeId::HasEncoding,
            ReferenceDirection::Inverse,
        )
        .has_type_definition(ObjectTypeId::DataTypeEncodingType)
        .insert(addr);

    let type_def = DataTypeDefinition::Structure(StructureDefinition {
        default_encoding_id: enc_id,
        base_data_type: DataTypeId::Structure.into(),
        structure_type: StructureType::Structure,
        fields: Some(vec![
            StructureField {
                name: "At".into(),
                data_type: DataTypeId::DateTime.into(),
                value_rank: -1,
                ..Default::default()
            },
            StructureField {
                name: "Name".into(),
                data_type: DataTypeId::String.into(),
                value_rank: -1,
                ..Default::default()
            },
            StructureField {
                name: "Content".into(),
                data_type: DataTypeId::String.into(),
                value_rank: -1,
                ..Default::default()
            },
        ]),
    });

    DataTypeBuilder::new(&struct_id, "ChatLog", "ChatLog")
        .subtype_of(DataTypeId::Structure)
        .data_type_definition(type_def)
        .reference(
            enc_id,
            ReferenceTypeId::HasEncoding,
            ReferenceDirection::Forward,
        )
        .insert(addr);
}

fn register_chat_types(addr: &mut opcua::nodes::AddressSpace, ns: u16) {
    register_chat_log_data_type(addr, ns);

    let chat_log_type_id = NodeId::new(ns, 18u32);
    VariableTypeBuilder::new(&chat_log_type_id, "ChatLogType", "ChatLogType")
        .subtype_of(VariableTypeId::BaseDataVariableType)
        .data_type(NodeId::new(ns, CHAT_LOG_DATA_TYPE_ID))
        .insert(addr);

    let chat_log_event_type_id = NodeId::new(ns, 7u32);
    let chat_log_prop_id = NodeId::new(ns, 200u32);

    ObjectTypeBuilder::new(
        &chat_log_event_type_id,
        "ChatLogEventType",
        "ChatLogEventType",
    )
    .is_abstract(false)
    .subtype_of(ObjectTypeId::BaseEventType)
    .insert(addr);

    VariableBuilder::new(&chat_log_prop_id, "ChatLog", "ChatLog")
        .property_of(chat_log_event_type_id)
        .data_type(NodeId::new(ns, CHAT_LOG_DATA_TYPE_ID))
        .has_type_definition(chat_log_type_id)
        .has_modelling_rule(ObjectId::ModellingRule_Mandatory)
        .insert(addr);

    let chat_logs_type_id = NodeId::new(ns, 1u32);
    ObjectTypeBuilder::new(&chat_logs_type_id, "ChatLogsType", "ChatLogsType")
        .is_abstract(false)
        .subtype_of(ObjectTypeId::BaseObjectType)
        .generates_event(chat_log_event_type_id)
        .insert(addr);
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let (server, handle) = ServerBuilder::new()
        .application_name("OPC UA Chat Server")
        .application_uri("urn:async_opcua_chat_server")
        .product_uri("urn:async_opcua_chat_server")
        .create_sample_keypair(true)
        .host("localhost")
        .port(4856)
        .add_endpoint(
            "chat",
            ServerEndpoint::new(
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                &[ANONYMOUS_USER_TOKEN_ID.to_owned()],
            ),
        )
        .discovery_urls(vec!["opc.tcp://localhost:4856/".to_owned()])
        .with_node_manager(simple_node_manager(
            NamespaceMetadata {
                namespace_uri: NAMESPACE_URI.to_owned(),
                ..Default::default()
            },
            "chat",
        ))
        .trust_client_certs(true)
        .diagnostics_enabled(true)
        .build()
        .unwrap();

    let node_manager = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .unwrap();
    let ns = handle.get_namespace_index(NAMESPACE_URI).unwrap();
    let subscriptions = handle.subscriptions().clone();

    let post_count = Arc::new(AtomicU32::new(0));
    let post_count_id = NodeId::new(ns, 401u32);
    let chat_logs_id = NodeId::new(ns, 400u32);
    let post_method_id = NodeId::new(ns, 300u32);
    let input_args_id = NodeId::new(ns, 301u32);

    {
        let address_space = node_manager.address_space();
        let mut address_space = address_space.write();

        register_chat_types(&mut address_space, ns);

        ObjectBuilder::new(&chat_logs_id, "ChatLogs", "ChatLogs")
            .event_notifier(EventNotifier::SUBSCRIBE_TO_EVENTS)
            .organized_by(ObjectId::ObjectsFolder)
            .has_type_definition(NodeId::new(ns, 1u32))
            .insert(&mut address_space);

        VariableBuilder::new(&post_count_id, "PostCount", "PostCount")
            .property_of(chat_logs_id.clone())
            .data_type(DataTypeId::UInt32)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(0u32)
            .insert(&mut address_space);

        MethodBuilder::new(&post_method_id, "Post", "Post")
            .component_of(chat_logs_id.clone())
            .input_args(
                &mut address_space,
                &input_args_id,
                &[
                    ("Name", DataTypeId::String).into(),
                    ("Content", DataTypeId::String).into(),
                ],
            )
            .generates_event(NodeId::new(ns, 7u32))
            .insert(&mut address_space);
    }

    node_manager.inner().add_read_callback(
        post_count_id.clone(),
        {
            let pc = post_count.clone();
            move |_, _, _| {
                let value = pc.load(Ordering::Relaxed);
                Ok(DataValue::new_now(value))
            }
        },
    );

    {
        let nm = node_manager.clone();
        let subs = subscriptions.clone();
        let pc = post_count.clone();
        let pc_id = post_count_id.clone();

        node_manager.inner().add_method_callback(
            post_method_id,
            move |args| {
                let name = match args.first() {
                    Some(opcua::types::Variant::String(s)) => s.to_string(),
                    _ => return Err(StatusCode::BadInvalidArgument),
                };
                let content = match args.get(1) {
                    Some(opcua::types::Variant::String(s)) => s.to_string(),
                    _ => return Err(StatusCode::BadInvalidArgument),
                };

                let now = DateTime::now();
                let chat_log = ChatLog {
                    at: now,
                    name: UAString::from(name.clone()),
                    content: UAString::from(content.clone()),
                };

                let new_count = pc.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = nm.set_values(
                    &subs,
                    [(
                        &pc_id,
                        None,
                        DataValue::new_now(new_count),
                    )].into_iter(),
                );

                let event = ChatLogEventType::new(
                    NodeId::new(1u16, 7u32),
                    random::byte_string(128),
                    format!("Chat message from {name}"),
                    now,
                    chat_log,
                );

                subs.notify_events(
                    [(
                        &event as &dyn opcua::nodes::Event,
                        &ObjectId::Server.into(),
                    )]
                    .into_iter(),
                );

                info!("Post: name={name}, content={content}");
                Ok(Vec::new())
            },
        );
    }

    info!("chat-server listening on opc.tcp://localhost:4856/");
    server.run().await.unwrap();
}
