//! Publisher startup URI acceptance tests.

use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_pubsub::{PubSubConnectionConfig, PubSubEngine, WriterGroupConfig};
use opcua_server::address_space::AddressSpace;

fn connection(
    connection_id: &str,
    address: &str,
    writer_groups: Vec<WriterGroupConfig>,
) -> PubSubConnectionConfig {
    PubSubConnectionConfig {
        connection_id: connection_id.to_string(),
        name: connection_id.to_string(),
        address: address.to_string(),
        writer_groups,
        reader_groups: Vec::new(),
    }
}

#[tokio::test]
async fn startup_accepts_standard_mixed_case_opc_udp_address() {
    // Given: the OPC-10000-14 §§7.3.2.2-7.3.2.3 UDP URI scheme.
    let udp = connection("udp", "OpC.UdP://127.0.0.1:4840", Vec::new());
    let mut engine =
        PubSubEngine::with_connections(Arc::new(RwLock::new(AddressSpace::new())), vec![udp]);

    // When: publisher startup classifies the connection.
    let result = engine.start();

    // Then: the standards-compliant scheme is accepted before any writer groups run.
    assert_eq!(result, Ok(()));
    engine.stop().await;
}
