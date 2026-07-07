# Tasks: Bundle All Public Companion Specifications

## Phase 1: Infrastructure

- [ ] T001 Clone `OPCFoundation/UA-Nodeset` as a submodule into `schemas/companion/` — OPC 10000-1 Annex A
- [ ] T002 Generate a codegen config that targets all ~70 companion specs from the cloned repo, one output module per spec
- [ ] T003 Run codegen: `cargo run --bin async-opcua-codegen companions_codegen_config.yml`
- [ ] T004 Add generated modules under `async-opcua-server/src/companion/`

## Phase 2: Feature Gates & Registration

- [ ] T005 Script generation of a `companion-{name}` Cargo feature for each spec in `async-opcua-server/Cargo.toml`, each gating the corresponding generated module and its `import_node_set` call
- [ ] T006 Register all companion namespaces in a unified `import_companion_nodesets` function behind a `companion` meta-feature that enables all specs — OPC 10000-1 Annex A
- [ ] T007 Wire the unified import into `ServerBuilder` so any enabled companion spec is auto-registered

## Phase 3: Polish

- [ ] T008 Build and test `cargo test --all-features`
- [ ] T009 Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] T010 Run `tools/ci-playbook.sh --ci`
- [ ] T011 Update TODO.md
