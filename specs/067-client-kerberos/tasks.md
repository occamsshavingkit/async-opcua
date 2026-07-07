# Tasks: Client Kerberos SSO Support

## Phase 1: Feature Gate & Dependency

- [ ] T001 Forward `kerberos` feature to `async-opcua-crypto/kerberos` in `async-opcua-client/Cargo.toml`
- [ ] T002 Verify `cargo check --features kerberos -p async-opcua-client` compiles

## Phase 2: GssapiTokenSource (US1)

- [ ] T003 [US1] Add `GssapiTokenSource` struct and `pub fn acquire_kerberos_token(spn: &str) -> Result<IdentityToken>` in `async-opcua-client/src/identity_token.rs` behind `#[cfg(feature = "kerberos")]` — OPC-10000-6 §6.4:
  - Use `libgssapi::context::ClientCtx` to acquire service ticket
  - Base64-encode the GSSAPI token with `GSSAPI ` prefix
  - Return `IdentityToken::IssuedToken(ByteString::from(token))`
- [ ] T004 [US1] Build and test `cargo test --features kerberos -p async-opcua-client`

## Phase 3: Builder API (US2)

- [ ] T005 [US2] Add `kerberos_spn(impl Into<String>)` method to `ClientBuilder` in `async-opcua-client/src/builder.rs` that stores the SPN and auto-applies at session creation — OPC-10000-6 §6.4
- [ ] T006 [US2] Wire the stored SPN into `Session::new()` to call `acquire_kerberos_token` when connecting — OPC-10000-4 §5.6.3, OPC-10000-6 §6.4
- [ ] T007 [US2] Build and test `cargo test --all-features`

## Phase 4: Polish

- [ ] T008 Run `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] T009 Run `tools/ci-playbook.sh --ci`
- [ ] T010 Update TODO.md
