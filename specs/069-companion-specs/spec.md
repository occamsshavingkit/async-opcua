# Feature Specification: Bundle Public Companion Specifications

**Feature Branch**: `069-companion-specs`  
**Created**: 2026-07-07  
**Status**: Draft  
**Input**: User description: "Bundle publicly available OPC UA companion specifications from OPCFoundation/UA-Nodeset into the async-opcua server."

## User Scenarios

### User Story 1 — A developer enables companion spec types via Cargo feature (Priority: P1)

A developer building an OPC UA server for a factory wants to expose standard CNC and Robotics data types without writing custom code. They add `features = ["companion-cnc", "companion-robotics"]` to their Cargo.toml and the types are automatically registered in the address space.

### User Story 2 — All companion specs are gated behind individual features (Priority: P2)

A developer only needs the DI (Device Integration) types for their application. They don't want to bloat their binary with CNC or Robotics types. Each companion spec is behind its own Cargo feature flag.

## Companion Specs (from OPCFoundation/UA-Nodeset)

All ~70 publicly available companion specifications from the OPC Foundation's public UA-Nodeset repository. Each spec gets its own Cargo feature flag under `companion-{name}`.

Includes: ADI, AMB, AML, AdditiveManufacturing, AutoID, BACnet, CAS, CNC, CSPPlusForMachine, CommercialKitchenEquipment, CranesHoists, CuttingTool, DEXPI, DI, ECM, FDI, FDT, GDS, GMS, GPOS, Glass/Flat, I4AAS, IA, IEC61850, IJT, IOLink, IREDES, ISA-95, ISA95-JOBCONTROL, LADS, LaserSystems, MDIS, MTConnect, MachineTool, MachineVision, Machinery, MetalForming, Mining, OpenSCS, PADIM, PAEFS, PLCopen, PNDRV, PNEM, PNENC, PNGSDGM, PNRIO, POWERLINK, PROFINET, PackML, PlasticsRubber, Powertrain, Pumps, RSL, Robotics, Safety, Scales, Scheduler, Sercos, Shotblasting, SurfaceTechnology, TMC, TTD, UAFX, WMTP, Weihenstephan, WireHarness, WoT, Woodworking, and others.

## Functional Requirements

- **FR-001**: Each companion spec MUST have its own Cargo feature flag.
- **FR-002**: Generated types MUST be in separate modules per companion spec.
- **FR-003**: A `companion` meta-feature MUST enable all specs at once.
- **FR-004**: All existing tests MUST continue to pass.

## Success Criteria

- **SC-001**: All ~70 publicly available companion specs have Cargo features.
- **SC-002**: All existing 618+ tests pass.
- **SC-003**: `cargo check --features companion` (or with any single feature) compiles.
