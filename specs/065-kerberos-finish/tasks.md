# Tasks: Kerberos SSO Integration Test & Keytab Plumbing

**Input**: Design documents from `/specs/065-kerberos-finish/`
**Prerequisites**: plan.md, spec.md

**Tests**: Integration test is the primary deliverable for US1.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Keytab Path Plumbing (US2 — Priority: P2)

**Goal**: Plumb `keytab_path` through `GssapiIdentityValidator` to the GSSAPI `Cred::acquire` call so it uses the config file path instead of `KRB5_KTNAME` env var.

**Independent Test**: Start server with `kerberos_keytab("/tmp/test.keytab")`. Unset `KRB5_KTNAME`. Verify GSSAPI credential acquisition succeeds.

- [ ] T001 [US2] Research `libgssapi` 0.11 API for keytab-aware credential acquisition (`gss_acquire_cred_from` or `Cred::acquire` with keytab argument). Document finding in `async-opcua-crypto/src/identity/kerberos_validator.rs` as a code comment.
- [ ] T002 [US2] Modify `GssapiIdentityValidator::validate_token()` in `async-opcua-crypto/src/identity/kerberos_validator.rs` to accept an explicit keytab path: if `self.keytab_path` is Some, pass it to GSSAPI credential acquisition; if None, use the system default (current behavior). If the API doesn't support it, set `KRB5_KTNAME` in a child process via `std::env::set_var` before the credential call.
- [ ] T003 [US2] Remove `#[allow(dead_code)]` from `keytab_path` field after it is used — OPC-10000-6 §6.4
- [ ] T004 [US2] Build and run `cargo test --all-features` to verify no regressions

---

## Phase 2: CI KDC Setup (US1 — Priority: P1)

**Goal**: Add `setup_kerberos_kdc()` to `tools/ci-playbook.sh` that starts a local MIT KDC, creates test realm/user/service principal, and exports a keytab.

- [ ] T005 [US1] Add `setup_kerberos_kdc()` function to `tools/ci-playbook.sh` — OPC-10000-6 §6.4:
  - Install `krb5-kdc` and `krb5-admin-server` packages
  - Create realm `PLANT.LOCAL` with `krb5_newrealm` (non-interactive)
  - Create test user `operator1` with password `testpass` via `kadmin.local`
  - Create service principal `OPCUA/localhost` with random key via `kadmin.local`
  - Export keytab to `/tmp/opcua.keytab` via `kadmin.local ktadd`
  - Determine an unused port for the server and pass it through
- [ ] T006 [US1] Add a `job_kerberos_test` function to `tools/ci-playbook.sh` that calls `setup_kerberos_kdc()`, runs the integration test, and cleans up the KDC
- [ ] T007 [US1] Wire `job_kerberos_test` into the CI gate — skip gracefully if `libkrb5-dev` is not installed

---

## Phase 3: Integration Test (US1 — Priority: P1)

**Goal**: Write the end-to-end Kerberos SSO test.

- [ ] T008 [US1] Write integration test in `async-opcua-server/tests/kerberos_sso.rs` — OPC-10000-4 §5.6.3, OPC-10000-6 §6.4:
  - Check for `KRB5_KTNAME` env var or KDC availability; skip if absent
  - Start server with `kerberos_spn("OPCUA/localhost@PLANT.LOCAL")` and `kerberos_keytab("...")` 
  - Use `libgssapi` client API (`ClientCtx`) to acquire a Kerberos service ticket
  - Base64-encode the GSSAPI token with `GSSAPI ` prefix
  - Send as IssuedToken in `ActivateSession`
  - Assert `ActivateSessionResponse` returns Good
  - Assert session identity matches `operator1@PLANT.LOCAL`
  - Call `kdestroy` equivalent or invalidate credentials
  - Assert next connection fails with `BadIdentityTokenRejected`
- [ ] T009 [US1] Build and run `cargo test --all-features` to verify no regressions; run `cargo test --features kerberos` to run the integration test

---

## Phase 4: Polish

- [ ] T010 Run `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] T011 Run `tools/ci-playbook.sh --ci` — all steps must pass
- [ ] T012 Update `TODO.md` to mark the two items as done

---

## Dependencies & Execution Order

- Phase 1 (US2): No dependencies — can start immediately
- Phase 2 (US1 KDC): No dependencies — can run in parallel with Phase 1
- Phase 3 (US1 Test): Depends on Phase 1 (needs keytab plumbing) and Phase 2 (needs KDC setup)
- Phase 4 (Polish): Depends on all

### Parallel Opportunities

- T001 and T005 can run in parallel
