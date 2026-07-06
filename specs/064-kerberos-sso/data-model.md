# Data Model: Kerberos SSO Authentication

## Overview

This feature adds one new type and extends three existing types. No new domain entities are persisted; all configuration is in-memory at server startup.

## New Types

### `KerberosConfig` (server config)

Configuration for Kerberos authentication, gated behind `#[cfg(feature = "kerberos")]`.

```rust
pub struct KerberosConfig {
    /// Service principal name, e.g. "OPCUA/hostname.example.com@PLANT.LOCAL"
    pub spn: String,
    /// Path to keytab file. If None, GSSAPI default path is used (/etc/krb5.keytab)
    pub keytab_path: Option<PathBuf>,
    /// Principal-to-role mappings. If empty, all principals get default role.
    pub principal_roles: HashMap<String, Vec<String>>,
}
```

**Relationships**:
- `KerberosConfig` is an optional field on `ServerConfig` (or `ServerBuilder`)
- `spn` is used to acquire GSSAPI acceptor credentials
- `principal_roles` maps `user@REALM` strings to OPC UA role names

**Invariants**:
- `spn` MUST be non-empty when Kerberos is enabled
- `principal_roles` keys are case-sensitive; Kerberos principals are case-sensitive per RFC 4120

### `KerberosValidator` (crypto crate)

Implements `OAuth2IdentityValidator`. Created once at server startup, used for each IssuedToken validation.

```rust
pub struct KerberosValidator {
    spn: String,
    keytab_path: Option<PathBuf>,
}
```

**Operations**:
- `validate_token(&self, token: &str) -> Result<ClaimProfile, StatusCode>`
  - Decode base64 → binary GSSAPI token
  - Validate size (≤ 64KB)
  - Run `gss_accept_sec_context` in `spawn_blocking`
  - Extract principal name via `sender_name().display()`
  - Map principal to roles via `principal_roles` config
  - Build and return `ClaimProfile`

**Relationships**:
- `KerberosValidator` implements `OAuth2IdentityValidator` (existing trait)
- Each `validate_token` call creates a new `ServerCtx`, processes it, and drops it

**Invariants**:
- Token MUST be valid base64 or return `BadIdentityTokenRejected`
- Token size MUST be ≤ 64KB (return error if exceeded)
- GSSAPI step MUST complete within 5 seconds (timeout → error)
- Any GSSAPI error → `BadIdentityTokenRejected`
- Unknown principal with no default mapping → `ClaimProfile { username: principal, roles: vec![], permissions: vec![] }` (let RBAC decide)

## Modified Types

### `ServerBuilder` (builder.rs)

**Before**:
```rust
pub struct ServerBuilder {
    authenticator: Option<Arc<dyn AuthManager>>,
    // ... other fields ...
}
```

**After**:
```rust
pub struct ServerBuilder {
    authenticator: Option<Arc<dyn AuthManager>>,
    #[cfg(feature = "kerberos")]
    kerberos_config: Option<KerberosConfig>,
    // ... other fields ...
}
```

New builder methods:
- `kerberos_spn(impl Into<String>)` — set the SPN
- `kerberos_keytab(impl Into<PathBuf>)` — set keytab path
- `kerberos_principal_role(principal: impl Into<String>, role: impl Into<String>)` — add a mapping

### `ServerInfo` (info.rs)

**After** (kerberos feature only):
```rust
pub struct ServerInfo {
    // ... existing fields ...
    #[cfg(feature = "kerberos")]
    kerberos_validator: Option<KerberosValidator>,
}
```

### `OAuth2IdentityValidator` trait (identity/mod.rs)

No structural change. The `KerberosValidator` implements the existing trait. The trait name `OAuth2IdentityValidator` is noted as a candidate for future renaming to `IdentityTokenValidator` but is not changed in this feature to minimize diff.

## Unchanged Types

### `AuthManager` (authenticator.rs)

No changes. The `authenticate_issued_identity_token` method already receives the validated `ClaimProfile` and maps it to a `UserToken`. The same flow works for Kerberos.

### `IssuedToken` / `ActivateSessionRequest` (opcua-types)

No changes. The `tokenData` field is already a `ByteString` that can carry base64-encoded GSSAPI tokens.

### RBAC Role Resolver (rbac/)

No changes. Kerberos principals are resolved the same way as any other authenticated identity — via the `RoleResolver` using the `ResolvedIdentity` from the `ClaimProfile`.
