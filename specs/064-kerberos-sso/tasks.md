# Tasks: Kerberos SSO Authentication

**Input**: Design documents from `/specs/064-kerberos-sso/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Integration test with local KDC required for US1. New tests are needed to verify Kerberos token validation and principal mapping.

**Organization**: Tasks are grouped by user story for independent implementation. US2 (config) must be done before US1 (auto-auth) because the validator needs to be constructed from config. US3 (RBAC mapping) is independent of US1/US2 and can proceed in parallel after US2.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Feature Flag & Dependencies)

**Purpose**: Add the `kerberos` Cargo feature and GSSAPI dependency.

- [ ] T001 Add `kerberos` feature flag and `libgssapi` dependency to `async-opcua-crypto/Cargo.toml`
- [ ] T002 Add `kerberos` feature forwarding to `async-opcua-server/Cargo.toml` (forward from `async-opcua-crypto/kerberos`)
- [ ] T003 Verify `cargo check --features kerberos -p async-opcua-server` builds without errors

---

## Phase 2: Foundational — Kerberos Validator & Config (US2)

**Purpose**: Core types that all user stories depend on. Must be complete before US1 or US3.

**Goal**: Define `KerberosConfig` and implement `KerberosValidator` (GSSAPI-backed `OAuth2IdentityValidator`).

**Independent Test**: Unit test verifies `KerberosValidator::validate_token()` rejects invalid base64, validates a real GSSAPI token against a local KDC, and extracts the correct principal.

**Spec reference**: OPC-10000-6 §6.4 (IssuedToken identity), RFC 2743 (GSSAPI), RFC 4120 (Kerberos V5).

### Implementation for Phase 2

- [ ] T004 Create `KerberosConfig` struct with `spn`, `keytab_path`, and `principal_roles` fields in `async-opcua-server/src/config.rs` behind `#[cfg(feature = "kerberos")]` — OPC-10000-6 §6.4
- [ ] T005 [P] Implement `GssapiIdentityValidator` in `async-opcua-crypto/src/identity/kerberos_validator.rs` behind `#[cfg(feature = "kerberos")]` — RFC 2743 §3.1, RFC 4120 §5.3
- [ ] T006 [P] Implement `OAuth2IdentityValidator` trait for `GssapiIdentityValidator` in `async-opcua-crypto/src/identity/kerberos_validator.rs`:
  - Decode base64 token → raw GSSAPI binary
  - Enforce 64KB maximum token size (reject oversized tokens with `BadIdentityTokenRejected`)
  - Call `ServerCtx::step()` inside `tokio::task::spawn_blocking` with 5-second timeout
  - Extract client principal via `sender_name().display()`
  - Return `ClaimProfile { username: principal, roles: [], permissions: [] }` — RFC 2743 §1.1.1, OPC-10000-6 §6.4.1
- [ ] T007 Re-export `GssapiIdentityValidator` from `async-opcua-crypto/src/identity/mod.rs` behind feature flag
- [ ] T008 Build and run `cargo test --features kerberos -p async-opcua-crypto` to verify compilation

**Checkpoint**: `KerberosConfig` defined. `GssapiIdentityValidator` validates tokens against a live GSSAPI context. Ready to wire into server.

---

## Phase 3: User Story 1 — Auto-Authenticate Operator via Kerberos (Priority: P1)

**Goal**: Server wires `GssapiIdentityValidator` into the IssuedToken dispatch path. A domain-joined operator connects with a Kerberos service ticket as an IssuedToken and is authenticated without a password prompt.

**Independent Test**: Start server with Kerberos config. From a domain-joined client, `kinit` to get a TGT, acquire a service ticket via GSSAPI, send as base64-encoded IssuedToken, and verify `ActivateSessionResponse` is Good with the correct session identity.

**Spec reference**: OPC-10000-4 §5.6.3 (ActivateSession), OPC-10000-6 §6.4 (IssuedToken identity).

### Integration Test for User Story 1

- [ ] T009 [US1] Set up local MIT Kerberos KDC in `tools/ci-playbook.sh` (install `krb5-kdc`, create test realm, create test user and service principal, export keytab)
- [ ] T010 [US1] Write integration test in `async-opcua-server/tests/kerberos_sso.rs` that:
  - Starts server with Kerberos config pointing at test KDC keytab
  - Gets a Kerberos TGT for a test user via `kinit` (shell out or use `libgssapi` client API)
  - Acquires a service ticket via GSSAPI `ClientCtx`
  - Sends it as OPC UA IssuedToken in ActivateSession
  - Asserts `ActivateSessionResponse` returns Good
  - Asserts session identity matches the test user principal
  - Destroys tickets and verifies next attempt fails with `BadIdentityTokenRejected`

### Implementation for User Story 1

- [ ] T011 [US1] Add `kerberos_validator: Option<GssapiIdentityValidator>` field to `ServerInfo` in `async-opcua-server/src/info.rs` behind feature flag — OPC-10000-6 §6.4.1
- [ ] T012 [US1] Modify IssuedToken validation in `ServerInfo::authenticate_endpoint_with_ecc_ctx()` in `async-opcua-server/src/info.rs` to dispatch to `KerberosValidator` when the token prefix matches "GSSAPI " — OPC-10000-4 §5.6.3, OPC-10000-6 §6.4.1
- [ ] T013 [US1] Add token prefix detection logic: if IssuedToken `tokenData` starts with `GSSAPI ` prefix, route to `GssapiIdentityValidator`; otherwise fall through to existing JWT/`LocalOAuth2Validator` flow — OPC-10000-6 §6.4.1
- [ ] T014 [US1] Build and run `cargo test --all-features` to verify no regressions; fix any compilation errors
- [ ] T015 [US1] Run the Kerberos integration test and verify it passes

**Checkpoint**: Operator can authenticate via Kerberos. Server accepts GSSAPI tokens as IssuedToken and maps principal to identity.

---

## Phase 4: User Story 2 — Administrator Configures Kerberos (Priority: P2)

**Goal**: ServerBuilder exposes `kerberos_spn()`, `kerberos_keytab()`, and `kerberos_principal_role()` methods. Server validates keytab at startup and fails cleanly if missing.

**Independent Test**: Start server with valid Kerberos config — server boots successfully. Start server with missing keytab — server fails with a clear error message. Start server without Kerberos config — boots normally with no Kerberos path active.

**Spec reference**: OPC-10000-4 §7.1 (Server configuration), OPC-10000-6 §6.4 (IssuedToken configuration).

### Implementation for User Story 2

- [ ] T016 [P] [US2] Add `kerberos_spn(impl Into<String>)` builder method to `ServerBuilder` in `async-opcua-server/src/builder.rs` behind feature flag
- [ ] T017 [P] [US2] Add `kerberos_keytab(impl Into<PathBuf>)` builder method to `ServerBuilder` in `async-opcua-server/src/builder.rs` behind feature flag
- [ ] T018 [P] [US2] Add `kerberos_principal_role(principal, role)` builder method to `ServerBuilder` in `async-opcua-server/src/builder.rs` behind feature flag
- [ ] T019 [US2] In `ServerBuilder::build()`, if Kerberos config is present, construct `GssapiIdentityValidator` and populate `ServerInfo.kerberos_validator` in `async-opcua-server/src/server.rs` — OPC-10000-6 §6.4
- [ ] T020 [US2] In `ServerBuilder::build()`, validate that the keytab is accessible: if `keytab_path` is set, check `File::open()` and fail with a clear error if missing. If `keytab_path` is None, set `KRB5_KTNAME` to the default and let GSSAPI validate at first use — OPC-10000-6 §6.4
- [ ] T021 [US2] Build and run `cargo test --all-features` to verify no regressions

**Checkpoint**: Administrator can configure Kerberos via code or config. Server validates keytab existence at startup.

---

## Phase 5: User Story 3 — RBAC Role Assignment from Principal (Priority: P3)

**Goal**: The `principal_roles` map in `KerberosConfig` is used by `GssapiIdentityValidator` to populate `ClaimProfile.roles`. The existing RBAC resolver maps those roles to permissions.

**Independent Test**: Configure `engineer3@PLANT.LOCAL → ["Engineer"]`. Connect as that principal. Verify session carries Engineer role. Attempt to write a variable restricted to Engineer — succeeds. Connect as unmapped principal — session has default role (no elevated permissions).

**Spec reference**: OPC-10000-18 §8.2 (Role-based security), OPC-10000-3 §4.8 (Role model).

### Implementation for User Story 3

- [ ] T022 [US3] Modify `GssapiIdentityValidator` to look up principal in `KerberosConfig.principal_roles` and populate `ClaimProfile.roles` in `async-opcua-crypto/src/identity/kerberos_validator.rs` — OPC-10000-18 §8.2
- [ ] T023 [US3] Add default role fallback: if principal is not in the role map, set `ClaimProfile.roles = vec![]` (let RBAC assign default/observer permissions) — OPC-10000-18 §8.2
- [ ] T024 [US3] Extend Kerberos integration test to verify role assignment: connect as mapped principal, verify role, attempt restricted write — in `async-opcua-server/tests/kerberos_sso.rs`
- [ ] T025 [US3] Build and run `cargo test --all-features` to verify no regressions

**Checkpoint**: Kerberos principals are mapped to OPC UA roles. Unmapped principals get default access.

---

## Phase 6: Polish & CI Integration

**Purpose**: CI integration, clippy, docs, and final verification.

- [ ] T026 Update `tools/ci-playbook.sh` to install `libkrb5-dev` and set up MIT Kerberos KDC for the Kerberos integration test step
- [ ] T027 Run `cargo clippy --workspace --all-targets --all-features -- -D warnings` and fix any issues
- [ ] T028 Run full CI playbook `tools/ci-playbook.sh --ci` — all steps must pass
- [ ] T029 Update `TODO.md` to mark Kerberos SSO as done

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — can start immediately
- **Phase 2 (Foundational/US2)**: Depends on Phase 1 — provides `KerberosConfig` and `GssapiIdentityValidator`
- **Phase 3 (US1)**: Depends on Phase 2 — wires validator into server dispatch
- **Phase 4 (US2 config wiring)**: Depends on Phase 2 — adds builder methods. Can run in parallel with Phase 3.
- **Phase 5 (US3)**: Depends on Phase 2 — extends validator with role mapping. Can run in parallel with Phases 3 and 4.
- **Phase 6 (Polish)**: Depends on all user stories being complete

### User Story Dependencies

- US2 (Foundational): No dependencies on other stories — provides foundation
- US1 (Auto-auth): Depends on US2 (needs validator from config)
- US3 (RBAC): Depends on US2 (needs config struct). Independent of US1.

### Within Each User Story

- Config struct before builder methods
- Validator implementation before server integration
- Test written before implementation (for US1 integration test)
- Build and test after each sub-step

### Parallel Opportunities

- Phase 2 tasks T004, T005, T006 are all [P] — different files
- Phase 3 (US1) and Phase 4 (US2 wiring) can run in parallel after Phase 2
- Phase 5 (US3) is independent of US1 — can run in parallel
- Phase 6 tasks are mostly [P]

---

## Implementation Strategy

### MVP First (US2 + US1)

1. Complete Phase 1: Setup (feature flag, dependency)
2. Complete Phase 2: Foundational (config + validator)
3. Complete Phase 3: US1 (wire into dispatch, integration test)
4. **STOP and VALIDATE**: End-to-end Kerberos SSO works
5. Proceed to US2 wiring (builder API) and US3 (role mapping)

### Incremental Delivery

1. Setup + Foundational → `KerberosConfig` and `GssapiIdentityValidator` ready
2. US1 → Kerberos SSO works end-to-end → MVP deployable
3. US2 → Administrator-friendly config API
4. US3 → Principal-to-role mapping → full RBAC integration
5. Polish → CI passes, docs updated

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story must be independently verifiable
- All existing tests (618+) must pass after each phase
- Commit after each phase or logical task group
- The `OAuth2IdentityValidator` trait name is kept as-is for this feature; renaming to `IdentityTokenValidator` is deferred
