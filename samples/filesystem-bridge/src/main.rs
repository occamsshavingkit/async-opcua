//! A demonstration OPC UA server that mirrors a filesystem directory tree into the
//! OPC UA address space.
//!
//! * Directories become Object (Folder) nodes organized under the Objects folder.
//! * Files become Variable nodes. UTF-8 decodable files use the `String` data type,
//!   all other files use `ByteString`.
//!
//! The hierarchy is built once at startup by recursively walking `--root` (default
//! `/tmp`). This sample does not perform live filesystem watching.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use log::{info, warn};
use opcua::crypto::SecurityPolicy;
use opcua::server::address_space::{AddressSpace, VariableBuilder};
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::server::node_manager::memory::{simple_node_manager, SimpleNodeManager};
use opcua::server::{ServerBuilder, ServerEndpoint, ANONYMOUS_USER_TOKEN_ID};
use opcua::types::{
    ByteString, DataTypeId, MessageSecurityMode, NodeId, ObjectId, UAString, VariableTypeId,
};

const NAMESPACE_URI: &str = "urn:filesystem-bridge";

/// Files larger than this are represented by an empty value to avoid loading
/// arbitrarily large blobs into memory in this demonstration server.
const MAX_FILE_SIZE: u64 = 1 << 20; // 1 MiB

fn parse_root_arg() -> PathBuf {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--root" {
            if let Some(value) = args.next() {
                return PathBuf::from(value);
            }
            warn!("--root given without a value, falling back to /tmp");
        } else {
            warn!("ignoring unknown argument: {arg}");
        }
    }
    PathBuf::from("/tmp")
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let root = parse_root_arg();
    info!("filesystem-bridge root: {}", root.display());

    let (server, handle) = ServerBuilder::new()
        .application_name("OPC UA Filesystem Bridge")
        .application_uri("urn:async_opcua_filesystem_bridge")
        .product_uri("urn:async_opcua_filesystem_bridge")
        .create_sample_keypair(true)
        .host("localhost")
        .port(0)
        .add_endpoint(
            "filesystem",
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
            "filesystem",
        ))
        .trust_client_certs(true)
        .diagnostics_enabled(true)
        .build()
        .expect("failed to build server");

    let node_manager = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .unwrap();
    let ns = handle.get_namespace_index(NAMESPACE_URI).unwrap();

    {
        let address_space = node_manager.address_space();
        let mut address_space = address_space.write();
        build_tree(
            &mut address_space,
            ns,
            &root,
            &ObjectId::ObjectsFolder.into(),
        );
    }

    info!("filesystem-bridge listening on opc.tcp://localhost:0/");
    server.run().await.unwrap();
}

/// Recursively add a directory and its contents to the address space under `parent_node_id`.
fn build_tree(addr: &mut AddressSpace, ns: u16, path: &Path, parent_node_id: &NodeId) {
    let display_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let folder_node_id = NodeId::new(ns, path.to_string_lossy().into_owned());

    let created = addr.add_folder(
        &folder_node_id,
        display_name.clone(),
        display_name.clone(),
        parent_node_id,
    );
    if !created {
        warn!("failed to add folder node for {}", path.display());
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("cannot read directory {}: {e}", path.display());
            return;
        }
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        // Use symlink_metadata so symlinks themselves are treated as files rather
        // than being followed, which could introduce cycles.
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(e) => {
                warn!("cannot stat {}: {e}", entry_path.display());
                continue;
            }
        };
        if file_type.is_dir() {
            build_tree(addr, ns, &entry_path, &folder_node_id);
        } else if file_type.is_file() {
            add_file_node(addr, ns, &entry_path, &folder_node_id);
        }
    }
}

/// Add a single file as a Variable node. UTF-8 files become `String`, others `ByteString`.
fn add_file_node(addr: &mut AddressSpace, ns: u16, path: &Path, parent_node_id: &NodeId) {
    let browse_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let node_id = NodeId::new(ns, path.to_string_lossy().into_owned());

    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            warn!("cannot stat file {}: {e}", path.display());
            return;
        }
    };

    let bytes = if metadata.len() > MAX_FILE_SIZE {
        warn!(
            "skipping contents of {} ({} bytes > {} limit)",
            path.display(),
            metadata.len(),
            MAX_FILE_SIZE
        );
        Vec::new()
    } else {
        fs::read(path).unwrap_or_else(|e| {
            warn!("cannot read file {}: {e}", path.display());
            Vec::new()
        })
    };

    if let Ok(text) = String::from_utf8(bytes.clone()) {
        VariableBuilder::new(&node_id, &browse_name, &browse_name)
            .organized_by(parent_node_id)
            .data_type(DataTypeId::String)
            .has_type_definition(VariableTypeId::BaseDataVariableType)
            .value(UAString::from(text))
            .insert(addr);
    } else {
        VariableBuilder::new(&node_id, &browse_name, &browse_name)
            .organized_by(parent_node_id)
            .data_type(DataTypeId::ByteString)
            .has_type_definition(VariableTypeId::BaseDataVariableType)
            .value(ByteString::from(bytes))
            .insert(addr);
    }
}
