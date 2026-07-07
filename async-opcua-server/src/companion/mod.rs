//! Companion specification NodeSet imports (OPC 10000-1 Annex A).
//!
//! 44 companion specs available behind Cargo feature flags.
//! Each imports its published NodeSet XML at server startup.
#![allow(dead_code, unused_imports, unreachable_pub, clippy::all)]

use crate::address_space::AddressSpace;
use parking_lot::RwLock;

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
    ($feature:literal, $name:ident, $path:literal) => {
        #[cfg(feature = $feature)]
        pub fn $name(address_space: &RwLock<AddressSpace>) {
            let xml = include_str!($path);
            import_companion_xml(address_space, stringify!($name), xml, vec![]);
        }
        #[cfg(not(feature = $feature))]
        pub fn $name(_address_space: &RwLock<AddressSpace>) {}
    };
}

companion!(
    "companion-adi",
    import_adi,
    "../../../schemas/companion/ADI/Opc.Ua.Adi.NodeSet2.xml"
);
companion!(
    "companion-amb",
    import_amb,
    "../../../schemas/companion/AMB/Opc.Ua.AMB.NodeSet2.xml"
);
companion!(
    "companion-aml",
    import_aml,
    "../../../schemas/companion/AML/Opc.Ua.AMLLibraries.NodeSet2.xml"
);
companion!(
    "companion-autoid",
    import_autoid,
    "../../../schemas/companion/AutoID/Opc.Ua.AutoID.NodeSet2.xml"
);
companion!(
    "companion-bacnet",
    import_bacnet,
    "../../../schemas/companion/BACnet/Opc.Ua.BACnet.NodeSet2.xml"
);
companion!(
    "companion-cas",
    import_cas,
    "../../../schemas/companion/CAS/Opc.Ua.CAS.NodeSet2.xml"
);
companion!(
    "companion-cnc",
    import_cnc,
    "../../../schemas/companion/CNC/Opc.Ua.CNC.NodeSet.xml"
);
companion!(
    "companion-cspplusformachine",
    import_cspplusformachine,
    "../../../schemas/companion/CSPPlusForMachine/Opc.Ua.CSPPlusForMachine.NodeSet2.xml"
);
companion!("companion-commercialkitchenequipment", import_commercialkitchenequipment, "../../../schemas/companion/CommercialKitchenEquipment/Opc.Ua.CommercialKitchenEquipment.NodeSet2.xml");
companion!(
    "companion-craneshoists",
    import_craneshoists,
    "../../../schemas/companion/CranesHoists/Opc.Ua.CranesHoists.NodeSet2.xml"
);
companion!(
    "companion-cuttingtool",
    import_cuttingtool,
    "../../../schemas/companion/CuttingTool/Opc.Ua.CuttingTool.NodeSet2.xml"
);
companion!(
    "companion-dexpi",
    import_dexpi,
    "../../../schemas/companion/DEXPI/Opc.Ua.DEXPI.NodeSet2.xml"
);
companion!(
    "companion-di",
    import_di,
    "../../../schemas/companion/DI/Opc.Ua.Di.PackageMetadata.NodeSet2.xml"
);
companion!(
    "companion-demomodel",
    import_demomodel,
    "../../../schemas/companion/DemoModel/DemoModel.NodeSet2.xml"
);
companion!(
    "companion-ecm",
    import_ecm,
    "../../../schemas/companion/ECM/Opc.Ua.ECM.NodeSet2.xml"
);
companion!(
    "companion-fdi",
    import_fdi,
    "../../../schemas/companion/FDI/Opc.Ua.Fdi7.NodeSet2.xml"
);
companion!(
    "companion-fdt",
    import_fdt,
    "../../../schemas/companion/FDT/Opc.Ua.FDT.NodeSet.xml"
);
companion!(
    "companion-gds",
    import_gds,
    "../../../schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml"
);
companion!(
    "companion-gpos",
    import_gpos,
    "../../../schemas/companion/GPOS/Opc.Ua.GPOS.NodeSet2.xml"
);
companion!(
    "companion-i4aas",
    import_i4aas,
    "../../../schemas/companion/I4AAS/Opc.Ua.I4AAS.NodeSet2.xml"
);
companion!(
    "companion-ia",
    import_ia,
    "../../../schemas/companion/IA/Opc.Ua.IA.NodeSet2.examples.xml"
);
companion!(
    "companion-iolink",
    import_iolink,
    "../../../schemas/companion/IOLink/Opc.Ua.IOLinkIODD.NodeSet2.xml"
);
companion!(
    "companion-iredes",
    import_iredes,
    "../../../schemas/companion/IREDES/Opc.Ua.IREDES.NodeSet2.xml"
);
companion!(
    "companion-isa_95",
    import_isa_95,
    "../../../schemas/companion/ISA-95/Opc.ISA95.NodeSet2.xml"
);
companion!(
    "companion-lads",
    import_lads,
    "../../../schemas/companion/LADS/Opc.Ua.LADS.NodeSet2.xml"
);
companion!(
    "companion-lasersystems",
    import_lasersystems,
    "../../../schemas/companion/LaserSystems/LaserSystem-Example.NodeSet2.xml"
);
companion!(
    "companion-mdis",
    import_mdis,
    "../../../schemas/companion/MDIS/Opc.MDIS.NodeSet2.xml"
);
companion!(
    "companion-mtconnect",
    import_mtconnect,
    "../../../schemas/companion/MTConnect/Opc.Ua.MTConnect.NodeSet2.xml"
);
companion!(
    "companion-machinetool",
    import_machinetool,
    "../../../schemas/companion/MachineTool/Opc.Ua.MachineTool.NodeSet2.xml"
);
companion!(
    "companion-machinevision",
    import_machinevision,
    "../../../schemas/companion/MachineVision/Opc.Ua.MachineVision.NodeSet2.xml"
);
companion!(
    "companion-machinery",
    import_machinery,
    "../../../schemas/companion/Machinery/Opc.Ua.Machinery.Examples.NodeSet2.xml"
);
companion!(
    "companion-metalforming",
    import_metalforming,
    "../../../schemas/companion/MetalForming/Opc.Ua.MetalForming.NodeSet2.xml"
);
companion!(
    "companion-onboarding",
    import_onboarding,
    "../../../schemas/companion/Onboarding/Opc.Ua.Onboarding.NodeSet2.xml"
);
companion!(
    "companion-openscs",
    import_openscs,
    "../../../schemas/companion/OpenSCS/Opc.Ua.OPENSCS.NodeSet2.xml"
);
companion!(
    "companion-padim",
    import_padim,
    "../../../schemas/companion/PADIM/Opc.Ua.IRDI.NodeSet2.xml"
);
companion!(
    "companion-paefs",
    import_paefs,
    "../../../schemas/companion/PAEFS/Opc.Ua.PAEFS.NodeSet2.xml"
);
companion!(
    "companion-plcopen",
    import_plcopen,
    "../../../schemas/companion/PLCopen/Opc.Ua.PLCopen.NodeSet2_V1.02.xml"
);
companion!(
    "companion-pnem",
    import_pnem,
    "../../../schemas/companion/PNEM/Opc.Ua.PnEm.NodeSet2.xml"
);
companion!(
    "companion-powerlink",
    import_powerlink,
    "../../../schemas/companion/POWERLINK/Opc.Ua.POWERLINK.NodeSet2.xml"
);
companion!(
    "companion-profinet",
    import_profinet,
    "../../../schemas/companion/PROFINET/Opc.Ua.Pn.NodeSet2.xml"
);
companion!(
    "companion-packml",
    import_packml,
    "../../../schemas/companion/PackML/Opc.Ua.PackML.NodeSet2.xml"
);
companion!(
    "companion-powertrain",
    import_powertrain,
    "../../../schemas/companion/Powertrain/Opc.Ua.Powertrain.NodeSet2.xml"
);
companion!(
    "companion-pumps",
    import_pumps,
    "../../../schemas/companion/Pumps/Opc.Ua.Pumps.NodeSet2.xml"
);
companion!(
    "companion-robotics",
    import_robotics,
    "../../../schemas/companion/Robotics/Opc.Ua.Robotics.NodeSet2.xml"
);

/// Import all enabled companion specifications.
#[cfg(feature = "companion")]
pub fn import_all_companions(address_space: &RwLock<AddressSpace>) {
    import_adi(address_space);
    import_amb(address_space);
    import_aml(address_space);
    import_autoid(address_space);
    import_bacnet(address_space);
    import_cas(address_space);
    import_cnc(address_space);
    import_cspplusformachine(address_space);
    import_commercialkitchenequipment(address_space);
    import_craneshoists(address_space);
    import_cuttingtool(address_space);
    import_dexpi(address_space);
    import_di(address_space);
    import_demomodel(address_space);
    import_ecm(address_space);
    import_fdi(address_space);
    import_fdt(address_space);
    import_gds(address_space);
    import_gpos(address_space);
    import_i4aas(address_space);
    import_ia(address_space);
    import_iolink(address_space);
    import_iredes(address_space);
    import_isa_95(address_space);
    import_lads(address_space);
    import_lasersystems(address_space);
    import_mdis(address_space);
    import_mtconnect(address_space);
    import_machinetool(address_space);
    import_machinevision(address_space);
    import_machinery(address_space);
    import_metalforming(address_space);
    import_onboarding(address_space);
    import_openscs(address_space);
    import_padim(address_space);
    import_paefs(address_space);
    import_plcopen(address_space);
    import_pnem(address_space);
    import_powerlink(address_space);
    import_profinet(address_space);
    import_packml(address_space);
    import_powertrain(address_space);
    import_pumps(address_space);
    import_robotics(address_space);
}
