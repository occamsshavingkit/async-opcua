//! Companion specification NodeSet imports (OPC 10000-1 Annex A).
//!
//! Each companion spec is gated behind a `companion-{name}` Cargo feature.
//! Enabled specs are imported into the address space during server startup via
//! the runtime NodeSet2 XML importer.
#![allow(dead_code, unused_imports, unreachable_pub, clippy::all)] // Public API functions, called by downstream users

use crate::address_space::AddressSpace;
use parking_lot::RwLock;

/// Import a companion NodeSet XML into the address space.
fn import_companion_xml(
    address_space: &RwLock<AddressSpace>,
    name: &str,
    xml: &str,
    dependent_namespaces: Vec<String>,
) {
    let import = match opcua_nodes::NodeSet2Import::new_str("en", xml, dependent_namespaces) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(
                "Failed to load companion spec '{}' NodeSet XML: {}",
                name,
                e
            );
            return;
        }
    };
    let space = address_space.write();
    let mut namespaces = opcua_nodes::NamespaceMap::default();
    space.import_node_set(&import, &mut namespaces);
    tracing::info!("Imported companion spec '{}' into address space", name);
}

macro_rules! companion {
    ($feature:literal, $name:ident, $path:literal, $deps:expr) => {
        #[cfg(feature = $feature)]
        #[allow(dead_code)]
        pub fn $name(address_space: &RwLock<AddressSpace>) {
            let xml = include_str!($path);
            import_companion_xml(address_space, stringify!($name), xml, $deps);
        }
        #[cfg(not(feature = $feature))]
        pub fn $name(_address_space: &RwLock<AddressSpace>) {}
    };
}

// Most commonly used companion specs in industrial automation
companion!(
    "companion-di",
    import_di,
    "../../../schemas/companion/DI/Opc.Ua.Di.NodeSet2.xml",
    vec![]
);
companion!(
    "companion-autoid",
    import_autoid,
    "../../../schemas/companion/AutoID/Opc.Ua.AutoID.NodeSet2.xml",
    vec!["http://opcfoundation.org/UA/DI/".into()]
);
companion!(
    "companion-robotics",
    import_robotics,
    "../../../schemas/companion/Robotics/Opc.Ua.Robotics.NodeSet2.xml",
    vec!["http://opcfoundation.org/UA/DI/".into()]
);
companion!(
    "companion-cnc",
    import_cnc,
    "../../../schemas/companion/CNC/Opc.Ua.CNC.NodeSet.xml",
    vec!["http://opcfoundation.org/UA/DI/".into()]
);
companion!(
    "companion-machinetool",
    import_machinetool,
    "../../../schemas/companion/MachineTool/Opc.Ua.MachineTool.NodeSet2.xml",
    vec!["http://opcfoundation.org/UA/DI/".into()]
);
companion!(
    "companion-profinet",
    import_profinet,
    "../../../schemas/companion/PROFINET/Opc.Ua.Pn.NodeSet2.xml",
    vec![]
);
companion!(
    "companion-isa95",
    import_isa95,
    "../../../schemas/companion/ISA-95/Opc.ISA95.NodeSet2.xml",
    vec![]
);
companion!(
    "companion-packml",
    import_packml,
    "../../../schemas/companion/PackML/Opc.Ua.PackML.NodeSet2.xml",
    vec![]
);

/// Import all enabled companion specifications.
#[cfg(feature = "companion")]
pub fn import_all_companions(address_space: &RwLock<AddressSpace>) {
    import_di(address_space);
    import_autoid(address_space);
    import_robotics(address_space);
    import_cnc(address_space);
    import_machinetool(address_space);
    import_profinet(address_space);
    import_isa95(address_space);
    import_packml(address_space);
}
