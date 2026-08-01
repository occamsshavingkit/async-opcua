//! Standalone UADP PubSub publisher for cross-stack interop with OPC Foundation .NET.
//!
//! Publishes a known float value (72.5) every 50ms on UDP multicast 239.0.0.1:4840.
//! The .NET subscriber in samples/demo-server/interop/dotnet/ receives and verifies it.

use std::sync::Arc;
use std::time::Duration;

use opcua::core::sync::RwLock;
use opcua::server::address_space::{AddressSpace, VariableBuilder};
use opcua::types::NodeId;
use opcua_pubsub::{
    DataSetWriterConfig, MessageEncoding, PubSubBridge, PubSubConnectionConfig,
    PublishedDataSetConfig, UdpPublisher, WriterGroupConfig,
};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let node_id = NodeId::new(1, "InteropTest");
    let space = AddressSpace::new();
    space.add_namespace("http://opcfoundation.org/UA/", 0);
    space.add_namespace("urn:interop", 1);

    let var = VariableBuilder::new(&node_id, "InteropTest", "InteropTest")
        .data_type(opcua::types::DataTypeId::Double)
        .value(72.5f64)
        .build();
    space.insert(var, None::<&[(_, &NodeId, _)]>);

    let address_space = Arc::new(RwLock::new(space));
    let udp_publisher = Arc::new(UdpPublisher::new(address_space.clone()));

    let connection_config = PubSubConnectionConfig {
        reader_groups: Vec::new(),
        connection_id: "InteropPublisher1".to_string(),
        name: "Interop".to_string(),
        address: "udp://239.0.0.1:4840".to_string(),
        writer_groups: vec![WriterGroupConfig {
            writer_group_id: 1,
            publishing_interval: 50,
            encoding: MessageEncoding::Uadp,
            dataset_writers: vec![DataSetWriterConfig {
                dataset_writer_id: 101,
                dataset_name: "InteropDataset".to_string(),
                published_dataset: PublishedDataSetConfig {
                    published_variables: vec![node_id],
                    configuration_version: Default::default(),
                },
            }],
        }],
    };

    let bridge = PubSubBridge::new(
        address_space.clone(),
        connection_config,
        None,
        Some(udp_publisher),
    );

    let cancel = CancellationToken::new();
    let _handle = bridge
        .start(cancel.clone())
        .expect("interop UDP bridge should start with a valid destination");

    println!("Publishing UADP to udp://239.0.0.1:4840 (publisher=InteropPublisher1, writerGroup=1, writer=101, value=72.5)");

    // Run until killed
    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
