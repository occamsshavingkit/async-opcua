# Tasks: Bundle Companion Specifications

## Phase 1: Infrastructure

- [ ] T001 Clone `OPCFoundation/UA-Nodeset` into `schemas/companion/` — OPC 10000-1 Annex A (Companion Specs)
- [ ] T002 Create `companions_codegen_config.yml` with targets for Tier 1 specs (DI, AutoID, CNC, Robotics, MachineTool, PROFINET, ISA-95, PackML) — OPC 10000-1 Annex A
- [ ] T003 Run codegen: `cargo run --bin async-opcua-codegen companions_codegen_config.yml`
- [ ] T004 Add generated modules to `async-opcua-server/src/companion/` directory

## Phase 2: Feature Gates

- [ ] T005 [P] Add `companion-di` Cargo feature for DI types in `async-opcua-server/Cargo.toml`
- [ ] T006 [P] Add `companion-autoid` feature for AutoID types
- [ ] T007 [P] Add `companion-cnc` feature for CNC types
- [ ] T008 [P] Add `companion-robotics` feature for Robotics types
- [ ] T009 [P] Add `companion-machinetool` feature for MachineTool types
- [ ] T010 [P] Add `companion-profinet` feature for PROFINET types
- [ ] T011 [P] Add `companion-isa95` feature for ISA-95 types
- [ ] T012 [P] Add `companion-packml` feature for PackML types

## Phase 3: Registration

- [ ] T013 Register each companion spec namespace in a `CompanionNodeManager` that can import the generated nodesets
- [ ] T014 Wire `CompanionNodeManager` into `ServerBuilder` behind feature gates

## Phase 4: Polish

- [ ] T015 Build and test `cargo test --all-features`
- [ ] T016 Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] T017 Run `tools/ci-playbook.sh --ci`
- [ ] T018 Update TODO.md
