//! Reverse bridge sample.
//!
//! Starts an OPC UA server on `localhost:0` and mirrors all Variable nodes found
//! on a source OPC UA server (given by `--source <url>`) into its own address space.

use std::collections::HashSet;
use std::sync::Arc;

use log::{error, info, warn};

use opcua::client::{ClientBuilder, IdentityToken};
use opcua::crypto::SecurityPolicy;
use opcua::nodes::VariableBuilder;
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::server::node_manager::memory::simple_node_manager;
use opcua::server::{ServerBuilder, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID};
use opcua::types::{
    BrowseDescription, BrowseDirection, BrowseResultMask, DataTypeId, ExpandedNodeId,
    MessageSecurityMode, NodeClass, NodeId, ObjectId, ReadValueId, ReferenceDescription,
    ReferenceTypeId, StatusCode, TimestampsToReturn, UserTokenPolicy, VariableTypeId,
};

const NAMESPACE_URI: &str = "urn:reverse-bridge";

/// Parse `--source <url>` from the command line.
fn parse_args() -> Result<String, String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--source" {
            return args
                .next()
                .ok_or_else(|| "--source requires a URL argument".to_owned());
        }
    }
    Err("missing required --source <url> argument".to_owned())
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let source_url = match parse_args() {
        Ok(url) => url,
        Err(e) => {
            eprintln!("Error: {e}");
            eprintln!("Usage: samples-reverse-bridge --source <opc.tcp://host:port>");
            std::process::exit(1);
        }
    };

    // Build the bridge server on localhost:0 (OS-assigned port).
    let (server, handle) = ServerBuilder::new()
        .application_name("OPC UA Reverse Bridge")
        .application_uri("urn:reverse-bridge")
        .product_uri("urn:reverse-bridge")
        .create_sample_keypair(true)
        .host("localhost")
        .port(0)
        .add_endpoint(
            "none",
            ServerEndpoint::new(
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                &[ANONYMOUS_USER_TOKEN_ID.to_owned()],
            ),
        )
        .with_node_manager(simple_node_manager(
            NamespaceMetadata {
                namespace_uri: NAMESPACE_URI.to_owned(),
                ..Default::default()
            },
            "reverse-bridge",
        ))
        .trust_client_certs(true)
        .build()
        .expect("failed to build server");

    let node_manager = handle
        .node_managers()
        .get_of_type::<opcua::server::node_manager::memory::SimpleNodeManager>()
        .expect("simple node manager not found");
    let ns = handle
        .get_namespace_index(NAMESPACE_URI)
        .expect("namespace not registered");

    // Spawn the mirroring task. It connects to the source, browses, reads, and
    // populates this server's address space with mirrored variables.
    let nm = node_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = mirror_source(&source_url, nm, ns).await {
            error!("mirroring failed: {e}");
        }
    });

    info!("reverse-bridge server listening on localhost (OS-assigned port)");
    server.run().await.unwrap();
}

/// Connect to the source server, browse all variables, read their values, and
/// create mirrored Variable nodes in this server's address space.
async fn mirror_source(
    source_url: &str,
    node_manager: Arc<opcua::server::node_manager::memory::SimpleNodeManager>,
    ns: u16,
) -> Result<(), String> {
    let mut client = ClientBuilder::new()
        .application_name("Reverse Bridge Client")
        .application_uri("urn:reverse-bridge-client")
        .product_uri("urn:reverse-bridge-client")
        .trust_server_certs(true)
        .create_sample_keypair(true)
        .session_retry_limit(3)
        .client()
        .map_err(|e| format!("failed to build client: {e:?}"))?;

    let (session, event_loop) = client
        .connect_to_matching_endpoint(
            (
                source_url,
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        )
        .await
        .map_err(|e| format!("failed to connect to source {source_url}: {e}"))?;
    let _handle = event_loop.spawn();
    if !session.wait_for_connection().await {
        return Err(format!(
            "failed to establish session with source {source_url}"
        ));
    }

    // Recursively collect all Variable target NodeIds starting from ObjectsFolder.
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut variables: Vec<ReferenceDescription> = Vec::new();
    let root: NodeId = ObjectId::ObjectsFolder.into();
    browse_recursive(&session, &root, &mut visited, &mut variables).await;
    info!("discovered {} variable(s) on source", variables.len());

    // Read each variable's value and create a mirror in our address space.
    let mut mirror_index: u32 = 0;
    for desc in variables {
        let source_id = match resolve_node_id(&desc.node_id) {
            Some(id) => id,
            None => {
                warn!(
                    "skipping variable with unresolvable node id: {:?}",
                    desc.node_id
                );
                continue;
            }
        };

        let to_read = [ReadValueId::from(source_id.clone())];
        let values = match session
            .read(&to_read, TimestampsToReturn::Neither, 0.0)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                warn!("failed to read {:?}: {e}", source_id);
                continue;
            }
        };

        let value = match values.into_iter().next() {
            Some(dv) => dv.value,
            None => {
                warn!("no value returned for {:?}", source_id);
                continue;
            }
        };

        let display_text = desc.display_name.text.as_ref().to_string();
        if display_text.is_empty() {
            warn!("skipping variable with empty display name: {:?}", source_id);
            continue;
        }

        let mirror_id = NodeId::new(ns, format!("mirror-{mirror_index}"));
        mirror_index += 1;

        let address_space = node_manager.address_space();
        {
            let address_space = address_space.write();
            let mut builder =
                VariableBuilder::new(&mirror_id, display_text.as_str(), display_text.as_str())
                    .organized_by(NodeId::objects_folder_id())
                    .has_type_definition(VariableTypeId::BaseDataVariableType);

            if let Some(variant) = value {
                if let Some(dt) = variant.data_type() {
                    builder = builder.data_type(dt.node_id);
                }
                builder = builder.value(variant);
            } else {
                builder = builder.data_type(DataTypeId::BaseDataType);
            }
            builder.insert(&*address_space);
        }

        // Organize mirrors under a folder-like object so they are browsable.
        info!("mirrored {display_text} ({:?}) -> {mirror_id:?}", source_id);
    }

    // Disconnect gracefully.
    let _ = session.disconnect().await;
    Ok(())
}

/// Recursively browse from `node`, descending into Object nodes and collecting
/// Variable references into `variables`.
async fn browse_recursive(
    session: &Arc<opcua::client::Session>,
    node: &NodeId,
    visited: &mut HashSet<NodeId>,
    variables: &mut Vec<ReferenceDescription>,
) {
    if !visited.insert(node.clone()) {
        return;
    }

    let desc = BrowseDescription {
        node_id: node.clone(),
        browse_direction: BrowseDirection::Forward,
        reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
        include_subtypes: true,
        node_class_mask: 0,
        result_mask: BrowseResultMask::All as u32,
    };

    let results = match session.browse(&[desc], 0, None).await {
        Ok(r) => r,
        Err(e) => {
            warn!("browse failed for {:?}: {e}", node);
            return;
        }
    };

    for result in results {
        if result.status_code.is_bad() && result.status_code != StatusCode::BadNoMatch {
            warn!("browse result bad for {:?}: {}", node, result.status_code);
        }
        let Some(refs) = result.references else {
            continue;
        };
        for r in refs {
            match r.node_class {
                NodeClass::Variable => variables.push(r.clone()),
                NodeClass::Object => {
                    if let Some(child) = resolve_node_id(&r.node_id) {
                        Box::pin(browse_recursive(session, &child, visited, variables)).await;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Resolve an `ExpandedNodeId` from a browse result into a local `NodeId`.
/// For same-server nodes (server_index 0, no namespace URI) this is just the
/// inner NodeId.
fn resolve_node_id(id: &ExpandedNodeId) -> Option<NodeId> {
    if id.server_index != 0 || id.namespace_uri.value().is_some() {
        return None;
    }
    Some(id.node_id.clone())
}
