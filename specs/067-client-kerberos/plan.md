# Implementation Plan: Client Kerberos SSO Support

**Branch**: `067-client-kerberos` | **Date**: 2026-07-07

## Summary

Add a `GssapiTokenSource` helper to `async-opcua-client` behind the `kerberos` feature that acquires a Kerberos service ticket via GSSAPI `ClientCtx` and returns it as an `IdentityToken::IssuedToken` with the `GSSAPI ` prefix.

## Technical Context

**Language/Version**: Rust (edition 2021)
**Primary Dependencies**: `libgssapi` 0.11 (already pulled by async-opcua-crypto/kerberos)
**Target Platform**: Linux
**Scope**: ~80 lines new code in `async-opcua-client/src/identity_token.rs`

## Project Structure

```text
async-opcua-client/
├── Cargo.toml                         # Forward kerberos to async-opcua-crypto/kerberos
├── src/
│   └── identity_token.rs              # + GssapiTokenSource (acquire + encode)
```
