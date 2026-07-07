# OPC UA Companion Specifications

These NodeSet files are **not distributed with async-opcua**. They are published by the
OPC Foundation at:

> https://github.com/OPCFoundation/UA-Nodeset

## Quick Start

1. Clone the repository:
   ```bash
   git clone https://github.com/OPCFoundation/UA-Nodeset.git schemas/companion
   ```

2. Enable the companion specs you need in `Cargo.toml`:
   ```toml
   [dependencies.async-opcua-server]
   features = ["companion-robotics", "companion-we ihenstephan"]
   ```

3. Import at server startup:
   ```rust
   async_opcua_server::companion::import_robotics(&address_space);
   async_opcua_server::companion::import_we ihenstephan(&address_space);
   ```

## Included Companion Specifications

Each spec has a `companion-{name}` Cargo feature and an `import_{name}()` function:

| Feature | Spec Description | XML File |
|---------|-----------------|----------|
| `companion-adi` | Analyzer Device Integration | `ADI/Opc.Ua.Adi.NodeSet2.xml` |
| `companion-amb` | AMB | `AMB/Opc.Ua.AMB.NodeSet2.xml` |
| `companion-aml` | AutomationML | `AML/Opc.Ua.AMLBaseTypes.NodeSet2.xml` |
| `companion-autoid` | Auto-ID (RFID, barcode) | `AutoID/Opc.Ua.AutoID.NodeSet2.xml` |
| `companion-bacnet` | BACnet | `BACnet/Opc.Ua.BACnet.NodeSet2.xml` |
| `companion-cas` | CAS | `CAS/Opc.Ua.CAS.NodeSet2.xml` |
| `companion-cnc` | CNC Systems | `CNC/Opc.Ua.CNC.NodeSet.xml` |
| `companion-cspplusformachine` | CSP+ for Machine | `CSPPlusForMachine/Opc.Ua.CSPPlusForMachine.NodeSet2.xml` |
| `companion-commercialkitchenequipment` | Commercial Kitchen Equipment | `CommercialKitchenEquipment/Opc.Ua.CommercialKitchenEquipment.NodeSet2.xml` |
| `companion-craneshoists` | Cranes & Hoists | `CranesHoists/Opc.Ua.CranesHoists.NodeSet2.xml` |
| `companion-cuttingtool` | Cutting Tools | `CuttingTool/Opc.Ua.CuttingTool.NodeSet2.xml` |
| `companion-dexpi` | DEXPI | `DEXPI/Opc.Ua.DEXPI.NodeSet2.xml` |
| `companion-di` | Device Integration (base for many others) | `DI/Opc.Ua.Di.NodeSet2.xml` |
| `companion-ecm` | ECM | `ECM/Opc.Ua.ECM.NodeSet2.xml` |
| `companion-fdi` | Field Device Integration | `FDI/Opc.Ua.Fdi5.NodeSet2.xml` |
| `companion-fdt` | Field Device Tool | `FDT/Opc.Ua.FDT.NodeSet.xml` |
| `companion-gds` | Global Discovery Server | `GDS/Opc.Ua.Gds.NodeSet2.xml` |
| `companion-gms` | GMS | `GMS/opc.ua.gms.nodeset2.xml` |
| `companion-gpos` | GPOS | `GPOS/Opc.Ua.GPOS.NodeSet2.xml` |
| `companion-i4aas` | Industry 4.0 Asset Administration Shell | `I4AAS/Opc.Ua.I4AAS.NodeSet2.xml` |
| `companion-ia` | IA | `IA/Opc.Ua.IA.NodeSet2.xml` |
| `companion-iolink` | IO-Link | `IOLink/Opc.Ua.IOLink.NodeSet2.xml` |
| `companion-iredes` | IREDES | `IREDES/Opc.Ua.IREDES.NodeSet2.xml` |
| `companion-isa95` | ISA-95 Manufacturing | `ISA-95/Opc.ISA95.NodeSet2.xml` |
| `companion-isa95_jobcontrol` | ISA-95 Job Control | `ISA95-JOBCONTROL/opc.ua.isa95-jobcontrol.nodeset2.xml` |
| `companion-lads` | LADS | `LADS/Opc.Ua.LADS.NodeSet2.xml` |
| `companion-lasersystems` | Laser Systems | `LaserSystems/Opc.Ua.LaserSystems.NodeSet2.xml` |
| `companion-machinery` | Machinery | `Machinery/Opc.Ua.Machinery.NodeSet2.xml` |
| `companion-machinetool` | Machine Tools | `MachineTool/Opc.Ua.MachineTool.NodeSet2.xml` |
| `companion-machinevision` | Machine Vision | `MachineVision/Opc.Ua.MachineVision.NodeSet2.xml` |
| `companion-mdis` | MDIS | `MDIS/Opc.MDIS.NodeSet2.xml` |
| `companion-metalforming` | Metal Forming | `MetalForming/Opc.Ua.MetalForming.NodeSet2.xml` |
| `companion-mtconnect` | MTConnect | `MTConnect/Opc.Ua.MTConnect.NodeSet2.xml` |
| `companion-openscs` | OpenSCS | `OpenSCS/Opc.Ua.OPENSCS.NodeSet2.xml` |
| `companion-padim` | PADIM | `PADIM/Opc.Ua.PADIM.NodeSet2.xml` |
| `companion-paefs` | PAEFS | `PAEFS/Opc.Ua.PAEFS.NodeSet2.xml` |
| `companion-plcopen` | PLCopen | `PLCopen/Opc.Ua.PLCopen.NodeSet2_V1.02.xml` |
| `companion-pndrv` | PROFINET Drive | `PNDRV/Opc.Ua.PNDRV.Nodeset2.xml` |
| `companion-pnem` | PROFINET Energy Management | `PNEM/Opc.Ua.PnEm.NodeSet2.xml` |
| `companion-pnenc` | PROFINET Encoder | `PNENC/Opc.Ua.PnEnc.Nodeset2.xml` |
| `companion-pngsdgm` | PROFINET GSDGM | `PNGSDGM/opc.ua.pngsdgm.Nodeset2.xml` |
| `companion-pnrio` | PROFINET RIO | `PNRIO/Opc.Ua.PnRio.Nodeset2.xml` |
| `companion-powerlink` | POWERLINK | `POWERLINK/Opc.Ua.POWERLINK.NodeSet2.xml` |
| `companion-powertrain` | Powertrain | `Powertrain/Opc.Ua.Powertrain.NodeSet2.xml` |
| `companion-profinet` | PROFINET | `PROFINET/Opc.Ua.Pn.NodeSet2.xml` |
| `companion-pumps` | Pumps | `Pumps/Opc.Ua.Pumps.NodeSet2.xml` |
| `companion-robotics` | Robotics | `Robotics/Opc.Ua.Robotics.NodeSet2.xml` |
| `companion-rsl` | RSL | `RSL/Opc.Ua.RSL.NodeSet2.xml` |
| `companion-safety` | Functional Safety | `Safety/Opc.Ua.Safety.NodeSet2.xml` |
| `companion-scales` | Scales | `Scales/Opc.Ua.Scales.NodeSet2.xml` |
| `companion-sercos` | Sercos | `Sercos/Sercos.NodeSet2.xml` |
| `companion-shotblasting` | Shotblasting | `Shotblasting/Opc.Ua.Shotblasting.NodeSet2.xml` |
| `companion-scheduler` | Scheduler | `Scheduler/Opc.Ua.Scheduler.NodeSet2.xml` |
| `companion-tmc` | TMC | `TMC/Opc.Ua.TMC.NodeSet2.xml` |
| `companion-we ihenstephan` | Weihenstephan (Brewing) | `Weihenstephan/Opc.Ua.Weihenstephan.NodeSet2.xml` |
| `companion-wmtp` | WMTP | `WMTP/Opc.Ua.WMTP.NodeSet2.xml` |  |
| `companion-woodworking` | Woodworking | `Woodworking/Opc.Ua.Woodworking.NodeSet2.xml` |
| `companion-wot` | Web of Things | `WoT/Opc.Ua.WotCon.NodeSet2.xml` |
| `companion-packml` | PackML (Packaging) | `PackML/Opc.Ua.PackML.NodeSet2.xml` |

## License

These NodeSet files are published by the OPC Foundation under their respective specification
licenses. They are NOT part of async-opcua. See the UA-Nodeset repository for details.

## Adding More Specs

To add a companion spec that isn't listed above, add a `companion!()` macro invocation to
`async-opcua-server/src/companion/mod.rs` following the existing pattern.
