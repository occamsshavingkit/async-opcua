pub mod generated;

pub use generated::node_ids::{DataTypeId, ObjectId};

use std::path::PathBuf;
use std::sync::Arc;

use opcua::server::{
    node_manager::memory::simple_node_manager_imports, ServerBuilder, ServerHandle,
};

pub fn build_server(config_path: Option<PathBuf>) -> (opcua::server::Server, ServerHandle) {
    let mut builder = ServerBuilder::new();
    if let Some(path) = config_path {
        builder = builder.with_config_from(path);
    } else {
        builder = builder
            .application_name("custom-codegen-test")
            .application_uri("urn:custom-codegen-test")
            .product_uri("urn:custom-codegen-test")
            .create_sample_keypair(true)
            .pki_dir("./pki-test")
            .discovery_urls(vec!["opc.tcp://127.0.0.1:4840/".to_string()])
            .host("127.0.0.1")
            .add_endpoint(
                "none",
                (
                    "/",
                    opcua::crypto::SecurityPolicy::None,
                    opcua::types::MessageSecurityMode::None,
                    &["ANONYMOUS"] as &[&str],
                ),
            );
    }
    builder
        .with_node_manager(simple_node_manager_imports(
            vec![Box::new(generated::ProfinetNamespace)],
            "ProfiNet",
        ))
        .trust_client_certs(true)
        .with_type_loader(Arc::new(crate::generated::types::GeneratedTypeLoader))
        .build()
        .unwrap()
}
