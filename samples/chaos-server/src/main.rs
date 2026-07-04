//! A "chaos" server that mutates its own address space unpredictably.
//!
//! This is an error-handling exercise: every 1-5 seconds a background task picks a
//! random variable and changes its value, data type, or status code, so clients
//! must cope with values that shift underneath them.
//!
//!   cargo run -p samples-chaos-server

use std::time::Duration;

use log::{info, warn};
use opcua::crypto::SecurityPolicy;
use opcua::server::address_space::Variable;
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::server::node_manager::memory::{simple_node_manager, SimpleNodeManager};
use opcua::server::{ServerBuilder, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID};
use opcua::types::{DataValue, MessageSecurityMode, NodeId, StatusCode, UAString, Variant};
use rand::rngs::OsRng;
use rand::Rng;

const NAMESPACE: &str = "urn:ChaosServer";

/// Status codes we cycle through to exercise client error handling.
const CHAOS_STATUSES: [StatusCode; 4] = [
    StatusCode::Good,
    StatusCode::BadUnexpectedError,
    StatusCode::BadOutOfService,
    StatusCode::BadNoData,
];

/// Produce a random variant value of a random scalar type.
fn random_variant(rng: &mut impl Rng) -> Variant {
    match rng.gen_range(0..6u8) {
        0 => Variant::Boolean(rng.gen()),
        1 => Variant::Int32(rng.gen()),
        2 => Variant::UInt32(rng.gen()),
        3 => Variant::Double(rng.gen()),
        4 => Variant::Float(rng.gen()),
        _ => Variant::String(UAString::from(format!("chaos-{}", rng.gen::<u32>()))),
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let (server, handle) = ServerBuilder::new()
        .application_name("Async OPC-UA Chaos Server sample")
        .application_uri("urn:async_opcua_chaos_server")
        .product_uri("urn:async_opcua_chaos_server")
        .create_sample_keypair(true)
        .host("localhost")
        .port(0)
        .add_endpoint(
            "standard",
            ServerEndpoint::new(
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                &[ANONYMOUS_USER_TOKEN_ID.to_owned()],
            ),
        )
        .with_node_manager(simple_node_manager(
            NamespaceMetadata {
                namespace_uri: NAMESPACE.to_owned(),
                ..Default::default()
            },
            "chaos",
        ))
        .trust_client_certs(true)
        .diagnostics_enabled(true)
        .build()
        .unwrap();

    let node_manager = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .unwrap();
    let ns = handle.get_namespace_index(NAMESPACE).unwrap();
    let subscriptions = handle.subscriptions().clone();

    // The list of variable node ids the chaos task is allowed to mutate.
    let chaos_node_ids: Vec<NodeId> = (0..12u32).map(|i| NodeId::new(ns, i)).collect();

    {
        let address_space = node_manager.address_space();
        let mut address_space = address_space.write();
        let folder = NodeId::new(ns, "ChaosFolder");
        address_space.add_folder(&folder, "Chaos", "Chaos", &NodeId::objects_folder_id());
        address_space.add_variables(
            chaos_node_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    Variable::new(id, format!("Chaos{i}"), format!("Chaos{i}"), i as i32)
                })
                .collect(),
            &folder,
        );
    }

    // Background chaos task: every 1-5s pick a random node and mutate it.
    {
        let nm = node_manager.clone();
        let subs = subscriptions.clone();
        let node_ids = chaos_node_ids.clone();
        tokio::spawn(async move {
            let mut rng = OsRng;
            loop {
                let delay = rng.gen_range(1..=5);
                tokio::time::sleep(Duration::from_secs(delay)).await;

                let id = &node_ids[rng.gen_range(0..node_ids.len())];
                // Two in three times: swap value (and likely data type).
                // One in three times: also flip the status code.
                let dv = if rng.gen_bool(0.33) {
                    let status = CHAOS_STATUSES[rng.gen_range(0..CHAOS_STATUSES.len())];
                    DataValue::new_now_status(random_variant(&mut rng), status)
                } else {
                    DataValue::new_now(random_variant(&mut rng))
                };

                if let Err(e) = nm.set_values(&subs, [(id, None, dv)].into_iter()) {
                    warn!("chaos write to {id} failed: {e}");
                } else {
                    info!("chaos: mutated {id}");
                }
            }
        });
    }

    // Graceful shutdown on Ctrl+C.
    let handle_c = handle.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("shutting down");
            handle_c.cancel();
        }
    });

    info!("chaos-server listening on opc.tcp://localhost:0/ (OS-assigned port)");
    server.run().await.unwrap();
}
