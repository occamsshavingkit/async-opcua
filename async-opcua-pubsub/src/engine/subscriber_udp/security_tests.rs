use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_server::address_space::{AddressSpace, VariableBuilder};
use opcua_types::{
    AttributeId, ContextOwned, DataEncoding, DataTypeId, MessageSecurityMode, NodeId, NumericRange,
    TimestampsToReturn, Variant,
};
use tokio::{net::UdpSocket, time::timeout};
use tokio_util::sync::CancellationToken;

use super::{PubSubEngine, SubscriberDatagramProcessor};
use crate::{
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    DataSetReaderConfig, FieldTargetConfig, PubSubConnectionConfig, ReaderGroupConfig,
    SecurityGroup, SubscriberRuntime,
};

const SECURITY_GROUP_ID: &str = "udp-line-a";
const POLICY: SecurityPolicy = SecurityPolicy::PubSubAes256Ctr;

fn target_value(space: &AddressSpace, node_id: &NodeId) -> Option<Variant> {
    space
        .find(node_id)?
        .as_node()
        .get_attribute(
            TimestampsToReturn::Neither,
            AttributeId::Value,
            &NumericRange::None,
            &DataEncoding::Binary,
        )?
        .value
}

#[tokio::test]
async fn udp_ingress_accepts_valid_security_and_rejects_tamper_and_replay() {
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let receiver_address = receiver.local_addr().unwrap();
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = NodeId::new(1, "SecureUdpTarget");
    VariableBuilder::new(&target, "SecureUdpTarget", "SecureUdpTarget")
        .data_type(DataTypeId::Double)
        .value(Variant::Double(0.0))
        .insert(&space);
    let address_space = Arc::new(RwLock::new(space));
    let connection = PubSubConnectionConfig {
        connection_id: "secure-udp".to_string(),
        name: "secure-udp".to_string(),
        address: format!("udp://{receiver_address}"),
        writer_groups: Vec::new(),
        reader_groups: vec![ReaderGroupConfig {
            reader_group_id: 1,
            security_mode: Some(MessageSecurityMode::SignAndEncrypt),
            security_policy_uri: Some(POLICY.to_uri().to_string()),
            security_group_id: Some(SECURITY_GROUP_ID.to_string()),
            dataset_readers: vec![DataSetReaderConfig {
                dataset_reader_id: 1,
                dataset_writer_id: 42,
                publisher_id: Some(PublisherId::UInt16(11)),
                writer_group_id: Some(7),
                network_message_number: Some(3),
                target_variables: vec![FieldTargetConfig::value(0, target.clone())],
                ..DataSetReaderConfig::default()
            }],
        }],
    };
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(address_space.clone(), vec![connection.clone()])
            .expect("secured UDP fixture should be valid"),
    ));
    let mut engine =
        PubSubEngine::with_connections(address_space.clone(), vec![connection.clone()]);
    engine.register_security_group(
        SecurityGroup::new(SECURITY_GROUP_ID, Duration::from_secs(3600))
            .expect("security group fixture should be valid"),
    );
    let security = engine
        .prepare_subscriber_security_processor(&connection)
        .expect("secured UDP reader should prepare");
    let processor = SubscriberDatagramProcessor::new(
        runtime.clone(),
        &connection.connection_id,
        connection.reader_groups[0].dataset_readers.clone(),
        security,
    );
    let message = UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(71.5)],
        }],
    };
    let secured = engine
        .encode_publisher_uadp_message(
            SECURITY_GROUP_ID,
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message,
            &ContextOwned::default().context(),
        )
        .expect("secured UDP payload should encode");
    let mut tampered = secured.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    let cancel = CancellationToken::new();
    let handles = engine.spawn_udp_subscriber(connection, receiver, processor, cancel.clone());
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    sender.send_to(&secured, receiver_address).await.unwrap();
    sender.send_to(&tampered, receiver_address).await.unwrap();
    sender.send_to(&secured, receiver_address).await.unwrap();
    timeout(Duration::from_secs(1), async {
        loop {
            let status = runtime
                .read()
                .reader_status(1)
                .expect("secured UDP reader status should exist");
            if status.accepted_count == 1 && status.security_failure_count == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("secured UDP datagrams should be processed");
    cancel.cancel();
    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(71.5))
    );
}
