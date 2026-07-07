//! Companion specification NodeSet imports (OPC 10000-1 Annex A).
//!
//! Companion NodeSet files are NOT distributed with async-opcua. They are
//! published by the OPC Foundation at https://github.com/OPCFoundation/UA-Nodeset.
//!
//! Clone the repository into `schemas/companion/` before building with any
//! `companion-*` features. See `schemas/companion/README.md` for instructions.
#![allow(dead_code, unused_imports, unreachable_pub, clippy::all, unexpected_cfgs)]

use std::path::PathBuf;

use parking_lot::RwLock;
use tracing::warn;

use crate::address_space::AddressSpace;

/// Base path for companion NodeSet XML files.
/// Defaults to `schemas/companion/` relative to the workspace root.
static COMPANION_BASE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../schemas/companion");

fn import_companion_xml(
    address_space: &RwLock<AddressSpace>,
    name: &str,
    relative_path: &str,
    dependent_namespaces: Vec<String>,
) {
    let path = PathBuf::from(COMPANION_BASE).join(relative_path);
    let xml = match std::fs::read_to_string(&path) {
        Ok(x) => x,
        Err(e) => {
            warn!(
                "Companion spec '{}' NodeSet XML not found at {}: {}. \
                 Clone https://github.com/OPCFoundation/UA-Nodeset into schemas/companion/",
                name,
                path.display(),
                e
            );
            return;
        }
    };
    let import = match opcua_nodes::NodeSet2Import::new_str("en", &xml, dependent_namespaces) {
        Ok(i) => i,
        Err(e) => {
            warn!("Failed to load companion spec '{}' NodeSet XML: {}", name, e);
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
            import_companion_xml(address_space, stringify!($name), $path, vec![]);
        }
        #[cfg(not(feature = $feature))]
        pub fn $name(_address_space: &RwLock<AddressSpace>) {}
    };
}

companion!("companion-adi", import_adi, "ADI/Opc.Ua.Adi.NodeSet2.xml");
companion!("companion-amb", import_amb, "AMB/Opc.Ua.AMB.NodeSet2.xml");
companion!("companion-aml", import_aml, "AML/Opc.Ua.AMLBaseTypes.NodeSet2.xml");
companion!("companion-autoid", import_autoid, "AutoID/Opc.Ua.AutoID.NodeSet2.xml");
companion!("companion-bacnet", import_bacnet, "BACnet/Opc.Ua.BACnet.NodeSet2.xml");
companion!("companion-cas", import_cas, "CAS/Opc.Ua.CAS.NodeSet2.xml");
companion!("companion-cnc", import_cnc, "CNC/Opc.Ua.CNC.NodeSet.xml");
companion!("companion-cspplusformachine", import_cspplusformachine, "CSPPlusForMachine/Opc.Ua.CSPPlusForMachine.NodeSet2.xml");
companion!("companion-commercialkitchenequipment", import_commercialkitchenequipment, "CommercialKitchenEquipment/Opc.Ua.CommercialKitchenEquipment.NodeSet2.xml");
companion!("companion-craneshoists", import_craneshoists, "CranesHoists/Opc.Ua.CranesHoists.NodeSet2.xml");
companion!("companion-cuttingtool", import_cuttingtool, "CuttingTool/Opc.Ua.CuttingTool.NodeSet2.xml");
companion!("companion-dexpi", import_dexpi, "DEXPI/Opc.Ua.DEXPI.NodeSet2.xml");
companion!("companion-di", import_di, "DI/Opc.Ua.Di.NodeSet2.xml");
companion!("companion-ecm", import_ecm, "ECM/Opc.Ua.ECM.NodeSet2.xml");
companion!("companion-fdi", import_fdi, "FDI/Opc.Ua.Fdi5.NodeSet2.xml");
companion!("companion-fdt", import_fdt, "FDT/Opc.Ua.FDT.NodeSet.xml");
companion!("companion-gds", import_gds, "GDS/Opc.Ua.Gds.NodeSet2.xml");
companion!("companion-gms", import_gms, "GMS/opc.ua.gms.nodeset2.xml");
companion!("companion-gpos", import_gpos, "GPOS/Opc.Ua.GPOS.NodeSet2.xml");
companion!("companion-i4aas", import_i4aas, "I4AAS/Opc.Ua.I4AAS.NodeSet2.xml");
companion!("companion-ia", import_ia, "IA/Opc.Ua.IA.NodeSet2.xml");
companion!("companion-iolink", import_iolink, "IOLink/Opc.Ua.IOLink.NodeSet2.xml");
companion!("companion-iredes", import_iredes, "IREDES/Opc.Ua.IREDES.NodeSet2.xml");
companion!("companion-isa95", import_isa95, "ISA-95/Opc.ISA95.NodeSet2.xml");
companion!("companion-isa95_jobcontrol", import_isa95_jobcontrol, "ISA95-JOBCONTROL/opc.ua.isa95-jobcontrol.nodeset2.xml");
companion!("companion-lads", import_lads, "LADS/Opc.Ua.LADS.NodeSet2.xml");
companion!("companion-lasersystems", import_lasersystems, "LaserSystems/Opc.Ua.LaserSystems.NodeSet2.xml");
companion!("companion-machinery", import_machinery, "Machinery/Opc.Ua.Machinery.NodeSet2.xml");
companion!("companion-machinetool", import_machinetool, "MachineTool/Opc.Ua.MachineTool.NodeSet2.xml");
companion!("companion-machinevision", import_machinevision, "MachineVision/Opc.Ua.MachineVision.NodeSet2.xml");
companion!("companion-mdis", import_mdis, "MDIS/Opc.MDIS.NodeSet2.xml");
companion!("companion-metalforming", import_metalforming, "MetalForming/Opc.Ua.MetalForming.NodeSet2.xml");
companion!("companion-mtconnect", import_mtconnect, "MTConnect/Opc.Ua.MTConnect.NodeSet2.xml");
companion!("companion-openscs", import_openscs, "OpenSCS/Opc.Ua.OPENSCS.NodeSet2.xml");
companion!("companion-padim", import_padim, "PADIM/Opc.Ua.PADIM.NodeSet2.xml");
companion!("companion-paefs", import_paefs, "PAEFS/Opc.Ua.PAEFS.NodeSet2.xml");
companion!("companion-plcopen", import_plcopen, "PLCopen/Opc.Ua.PLCopen.NodeSet2_V1.02.xml");
companion!("companion-pndrv", import_pndrv, "PNDRV/Opc.Ua.PNDRV.Nodeset2.xml");
companion!("companion-pnem", import_pnem, "PNEM/Opc.Ua.PnEm.NodeSet2.xml");
companion!("companion-pnenc", import_pnenc, "PNENC/Opc.Ua.PnEnc.Nodeset2.xml");
companion!("companion-pngsdgm", import_pngsdgm, "PNGSDGM/opc.ua.pngsdgm.Nodeset2.xml");
companion!("companion-pnrio", import_pnrio, "PNRIO/Opc.Ua.PnRio.Nodeset2.xml");
companion!("companion-powerlink", import_powerlink, "POWERLINK/Opc.Ua.POWERLINK.NodeSet2.xml");
companion!("companion-powertrain", import_powertrain, "Powertrain/Opc.Ua.Powertrain.NodeSet2.xml");
companion!("companion-profinet", import_profinet, "PROFINET/Opc.Ua.Pn.NodeSet2.xml");
companion!("companion-pumps", import_pumps, "Pumps/Opc.Ua.Pumps.NodeSet2.xml");
companion!("companion-robotics", import_robotics, "Robotics/Opc.Ua.Robotics.NodeSet2.xml");
companion!("companion-rsl", import_rsl, "RSL/Opc.Ua.RSL.NodeSet2.xml");
companion!("companion-safety", import_safety, "Safety/Opc.Ua.Safety.NodeSet2.xml");
companion!("companion-scales", import_scales, "Scales/Opc.Ua.Scales.NodeSet2.xml");
companion!("companion-sercos", import_sercos, "Sercos/Sercos.NodeSet2.xml");
companion!("companion-shotblasting", import_shotblasting, "Shotblasting/Opc.Ua.Shotblasting.NodeSet2.xml");
companion!("companion-scheduler", import_scheduler, "Scheduler/Opc.Ua.Scheduler.NodeSet2.xml");
companion!("companion-tmc", import_tmc, "TMC/Opc.Ua.TMC.NodeSet2.xml");
companion!("companion-weihenstephan", import_weihenstephan, "Weihenstephan/Opc.Ua.Weihenstephan.NodeSet2.xml");
companion!("companion-wmtp", import_wmtp, "WMTP/Opc.Ua.WMTP.NodeSet2.xml");  
companion!("companion-woodworking", import_woodworking, "Woodworking/Opc.Ua.Woodworking.NodeSet2.xml");
companion!("companion-wot", import_wot, "WoT/Opc.Ua.WotCon.NodeSet2.xml");
companion!("companion-packml", import_packml, "PackML/Opc.Ua.PackML.NodeSet2.xml");

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
    import_ecm(address_space);
    import_fdi(address_space);
    import_fdt(address_space);
    import_gds(address_space);
    import_gms(address_space);
    import_gpos(address_space);
    import_i4aas(address_space);
    import_ia(address_space);
    import_iolink(address_space);
    import_iredes(address_space);
    import_isa95(address_space);
    import_isa95_jobcontrol(address_space);
    import_lads(address_space);
    import_lasersystems(address_space);
    import_machinery(address_space);
    import_machinetool(address_space);
    import_machinevision(address_space);
    import_mdis(address_space);
    import_metalforming(address_space);
    import_mtconnect(address_space);
    import_openscs(address_space);
    import_padim(address_space);
    import_paefs(address_space);
    import_plcopen(address_space);
    import_pndrv(address_space);
    import_pnem(address_space);
    import_pnenc(address_space);
    import_pngsdgm(address_space);
    import_pnrio(address_space);
    import_powerlink(address_space);
    import_powertrain(address_space);
    import_profinet(address_space);
    import_pumps(address_space);
    import_robotics(address_space);
    import_rsl(address_space);
    import_safety(address_space);
    import_scales(address_space);
    import_sercos(address_space);
    import_shotblasting(address_space);
    import_scheduler(address_space);
    import_tmc(address_space);
    import_weihenstephan(address_space);
    import_wmtp(address_space);
    import_woodworking(address_space);
    import_wot(address_space);
    import_packml(address_space);
}
