use std::{sync::Arc, time::Duration};

use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_server::address_space::AddressSpace;
use opcua_types::{ContextOwned, MessageSecurityMode, Variant};

use super::{connection, forward_processed_payloads, insert_target, reader, target_value};
use crate::{
    codec::uadp::{PublisherId, UadpDataSetMessage, UadpNetworkMessage},
    engine::subscriber::SubscriberDatagramProcessor,
    MessageEncoding, PubSubEngine, SecurityGroup, SubscriberRuntime,
};

const SECURITY_GROUP_ID: &str = "mqtt-line-a";
const POLICY: SecurityPolicy = SecurityPolicy::PubSubAes256Ctr;

fn message() -> UadpNetworkMessage {
    UadpNetworkMessage {
        publisher_id: PublisherId::UInt16(11),
        writer_group_id: 7,
        network_message_number: 3,
        sequence_number: 1,
        dataset_messages: vec![UadpDataSetMessage {
            dataset_writer_id: 42,
            sequence_number: 1,
            timestamp: None,
            status: None,
            fields: vec![Variant::Double(61.5)],
        }],
    }
}

#[tokio::test]
async fn mqtt_forwarder_accepts_valid_security_and_rejects_tamper_and_replay() {
    let space = AddressSpace::new();
    space.add_namespace("urn:test", 1);
    let target = insert_target(&space, "SecureMqttTarget");
    let address_space = Arc::new(RwLock::new(space));
    let mut owner = reader(1, MessageEncoding::Uadp, target.clone());
    owner.security_mode = Some(MessageSecurityMode::SignAndEncrypt);
    owner.security_policy_uri = Some(POLICY.to_uri().to_string());
    owner.security_group_id = Some(SECURITY_GROUP_ID.to_string());
    let config = connection(vec![owner.clone()]);
    let runtime = Arc::new(RwLock::new(
        SubscriberRuntime::with_connections(address_space.clone(), vec![config.clone()])
            .expect("secured MQTT fixture should be valid"),
    ));
    let mut engine = PubSubEngine::with_connections(address_space.clone(), vec![config.clone()]);
    engine.register_security_group(
        SecurityGroup::new(SECURITY_GROUP_ID, Duration::from_secs(3600))
            .expect("security group fixture should be valid"),
    );
    let security = engine
        .prepare_subscriber_security_processor(&config)
        .expect("secured MQTT reader should prepare");
    let processor =
        SubscriberDatagramProcessor::new(runtime.clone(), "mqtt-conn", vec![owner], security);
    let secured = engine
        .encode_publisher_uadp_message(
            SECURITY_GROUP_ID,
            MessageSecurityMode::SignAndEncrypt,
            POLICY,
            &message(),
            &ContextOwned::default().context(),
        )
        .expect("secured MQTT payload should encode");
    let mut tampered = secured.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;

    forward_processed_payloads(processor, 1, vec![secured.clone(), tampered, secured]).await;

    assert_eq!(
        target_value(&address_space.read(), &target),
        Some(Variant::Double(61.5))
    );
    let runtime = runtime.read();
    let status = runtime
        .reader_status(1)
        .expect("secured MQTT reader status should exist");
    assert_eq!(status.accepted_count, 1);
    assert_eq!(status.security_failure_count, 2);
}
