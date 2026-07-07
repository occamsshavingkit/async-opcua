# Tasks: Bundle All Public Companion Specifications

**Note**: The original plan assumed generated Rust code per spec (T001-T004). The implemented approach uses runtime XML NodeSet import via the `companion!()` macro in `async-opcua-server/src/companion/mod.rs`. These tasks are superseded and replaced below.

## Phase 1: Infrastructure — Runtime XML Import (supersedes T001-T004)

- [x] T001 Define per-spec import functions via the `companion!()` macro in `async-opcua-server/src/companion/mod.rs`
- [x] T002 Declare each `companion-{name}` Cargo feature in `async-opcua-server/Cargo.toml`

## Phase 2: Feature Gates & Registration (supersedes T005-T007)

- [x] T003 Create `companion` meta-feature that enables all specs
- [x] T004 Define `import_all_companions()` function that calls each spec importer
- [ ] T005 Wire `import_all_companions` into `ServerBuilder` during address-space initialization

## Phase 3: Polish

- [ ] T006 Build and test `cargo test --all-features`
- [ ] T007 Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] T008 Run `tools/ci-playbook.sh --ci`
- [ ] T009 Verify companion features match importer gates (see `tools/check-companion-features.sh`)
- [ ] T010 Update TODO.md
