# Implementation Plan: Client Kerberos SSO Support

**Branch**: `067-client-kerberos` | **Date**: 2026-07-07 | **Spec**: [spec.md](./spec.md)

## Summary

Add a `GssapiTokenSource` helper to `async-opcua-client` behind the `kerberos` feature that acquires a Kerberos service ticket via GSSAPI `ClientCtx` and returns it as an `IdentityToken::IssuedToken` with the `GSSAPI ` prefix. Wire it into `ClientBuilder` so users can call `kerberos_spn("OPCUA/hostname@REALM")` and get auto-authentication.

## Technical Context

**Language/Version**: Rust (edition 2021)
**Primary Dependencies**: `libgssapi` 0.11 (already pulled by async-opcua-crypto/kerberos)
**Storage**: N/A (in-memory token acquisition)
**Testing**: `cargo test --features kerberos -p async-opcua-client`
**Target Platform**: Linux
**Project Type**: library (workspace crate)
**Scope**: ~80 lines new code in `async-opcua-client/src/identity_token.rs`

## Constitution Check

| Principle | Assessment | Status |
|-----------|-----------|--------|
| I. Correctness Over Completion | GSSAPI call uses same patterns as server-side validator. Token size bounded. | PASS |
| II. Do It Right Once | Reuses existing `IdentityToken::IssuedToken` type. No new abstractions. | PASS |
| IV. Security Is Paramount | GSSAPI token is attacker-controlled only in server context; client only acquires. No new attack surface. | PASS |
| V. Leave It Better Than You Found It | ~80 lines, behind feature flag, zero impact on non-Kerberos clients. | PASS |

## Project Structure

```text
async-opcua-client/
├── Cargo.toml                    # Forward kerberos to async-opcua-crypto/kerberos
├── src/
│   ├── identity_token.rs         # + GssapiTokenSource (acquire + encode)
│   └── builder.rs                # + kerberos_spn() method
```
