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

## Companion Specs Available (from OPCFoundation/UA-Nodeset)

**Tier 1 — Foundation/frequently used (8)**:
DI, AutoID, CNC, Robotics, MachineTool, PROFINET, ISA-95, PackML

**Tier 2 — Manufacturing (8)**:
PlasticsRubber, MetalForming, Woodworking, SurfaceTechnology, Powertrain, Shotblasting, CuttingTool, AdditiveManufacturing

**Tier 3 — Infrastructure/domain-specific (10+)**:
IEC61850, BACnet, MDIS, GDS, Safety, Sercos, Pumps, Scales, LADS, GPOS, CommercialKitchenEquipment, etc.

**Already done**: FX (Parts 80-84), GDS (partial)

## Functional Requirements

- **FR-001**: Each companion spec MUST have its own Cargo feature flag in `async-opcua-server`.
- **FR-002**: Generated types MUST be in separate modules per companion spec.
- **FR-003**: The codegen tool MUST accept multiple companion spec configs.
- **FR-004**: Tier 1 companion specs MUST be bundled in the generated code.
- **FR-005**: All existing tests MUST continue to pass.

## Success Criteria

- **SC-001**: At least 8 Tier 1 companion specs are available behind Cargo features.
- **SC-002**: `cargo build --features companion-di` includes DI types in the address space.
- **SC-003**: All existing 618+ tests pass.
