# Implementation Plan: Kerberos SSO Authentication

**Branch**: `064-kerberos-sso` | **Date**: 2026-07-06 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/064-kerberos-sso/spec.md`

## Summary

Add Kerberos single sign-on to the OPC UA server so domain-joined operators can authenticate silently using their existing Windows/Linux session credentials — no password prompts — in air-gapped industrial environments.

A new `GssapiIdentityValidator` implements `OAuth2IdentityValidator`, validating Kerberos service tickets carried as OPC UA IssuedToken data. The server accepts GSSAPI tokens via `gss_accept_sec_context`, extracts the client principal, and maps it to a `UserToken` for the existing RBAC system.

Gated behind a `kerberos` Cargo feature to keep native GSSAPI dependencies opt-in.

## Technical Context

**Language/Version**: Rust (edition 2021, workspace resolver = "2")
**Primary Dependencies**: `libgssapi` 0.11 (native GSSAPI binding for MIT/Heimdal Kerberos on Linux/macOS); `cross-krb5` 0.5 if Windows support needed
**Storage**: Keytab file on disk (provisioned by domain admin); optional principal-to-role mapping file
**Testing**: `cargo test --all-features`; integration tests require a local Kerberos KDC (MIT `krb5-kdc` in CI)
**Target Platform**: Linux server (primary); macOS optional; Windows via SSPI if `cross-krb5` is used
**Project Type**: library (workspace crate consumed as a server)
**Performance Goals**: GSSAPI ticket validation < 50ms; no measurable overhead on non-Kerberos auth paths
**Constraints**: Must not block async runtime (wrap GSSAPI in `spawn_blocking`); must fail-closed; must not pull native deps unless `kerberos` feature enabled
**Scale/Scope**: ~500 lines of new code across 3 crates (`async-opcua-crypto`, `async-opcua-server`); new optional dependency

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Status |
|-----------|-----------|--------|
| I. Correctness Over Completion | GSSAPI tokens are attacker-controlled network input. Token size limits, timeouts, and fail-closed error handling must be implemented before the feature is usable. No unwrap/expect on the valIdation path. | PASS |
| II. Do It Right Once | The `OAuth2IdentityValidator` trait is reused (not recreated). GSSAPI calls are isolated behind a clean trait impl. No copy-paste from the JWT validator. | PASS |
| III. Individual Task Discipline | Three independent user stories, each verifiable standalone. Tasks will be one per story with clear acceptance criteria. | PASS |
| IV. Security Is Paramount | Network-facing. Token size must be bounded before allocation. GSSAPI runs in spawn_blocking to prevent async runtime starvation. Secrets never logged. Fail-closed: any validation failure → `BadIdentityTokenRejected`. | PASS |
| V. Leave It Better Than You Found It | Reuses existing trait (no new abstraction overhead). Adds a feature flag that is opt-in (no impact on existing functionality). May improve the trait name from `OAuth2IdentityValidator` to `IdentityTokenValidator` for clarity. | PASS |

**Gate Result**: All principles pass. No violations to justify.

## Project Structure

### Documentation (this feature)

```text
specs/064-kerberos-sso/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── spec.md              # Feature specification
└── tasks.md             # Phase 2 output (speckit.tasks)
```

### Source Code (repository root)

```text
async-opcua-crypto/src/
├── identity/
│   ├── mod.rs               # OAuth2IdentityValidator trait (may rename to IdentityTokenValidator)
│   ├── jwt_validator.rs     # Existing JWT validator
│   ├── kerberos_validator.rs # NEW: GssapiIdentityValidator
│   └── ...
└── Cargo.toml               # + libgssapi (optional, feature "kerberos")

async-opcua-server/src/
├── authenticator.rs          # AuthManager trait (existing, unchanged)
├── info.rs                   # authenticate_endpoint_with_ecc_ctx — IssuedToken dispatch
├── builder.rs                # + kerberos_principal(), kerberos_keytab_path()
├── config.rs                 # + kerberos SPN, keytab path, principal-role mapping
└── Cargo.toml               # + async-opcua-crypto/kerberos

tools/ci-playbook.sh          # + MIT Kerberos KDC installation for CI tests
```

**Structure Decision**: Single workspace project. New code is isolated in `async-opcua-crypto/src/identity/kerberos_validator.rs` behind a feature flag. Server configuration additions in `builder.rs` and `config.rs`. No new crates needed.

## Complexity Tracking

> No constitutional violations to justify.
