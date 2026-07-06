# Research: Kerberos SSO Authentication

## GSSAPI Crate Selection

### Decision: Use `libgssapi` 0.11 with optional `cross-krb5` for Windows support

**Rationale**: `libgssapi` provides safe Rust bindings to native GSSAPI (MIT Kerberos, Heimdal, Apple GSS Framework). It exposes `ServerCtx::new()` + `step()` wrapping `gss_accept_sec_context`, and `Name::new()` + `name.display()` for principal extraction. The crate is actively maintained (0.11.0 released May 2026) with 836K total downloads.

**Alternatives considered**:
- **`sspi`** (Pure Rust, 0.21.1): Windows-only. No GSSAPI/Unix support. Rejected.
- **`cross-krb5`** (0.5.0): Abstraction over `libgssapi` + SSPI. Same author as `libgssapi`. Consider if Windows support is required. The architecture is identical — just swaps the backend. Keeping `cross-krb5` as the dependency (not `libgssapi` directly) gives us Windows for free if desired. **Decision: use `libgssapi` directly for now; it's simpler and the server runs on Linux. If Windows support is needed later, the API surface is identical and a swap to `cross-krb5` is trivial.**
- **`gss-api`** (Pure Rust, abandoned): Only 247 lines, unfinished parser. Rejected.

### Server-side flow

```
1. Server starts → loads keytab implicitly via GSSAPI (KRB5_KTNAME env or default /etc/krb5.keytab)
2. Client connects with IssuedToken containing base64(GSSAPI token)
3. Server decodes base64 → binary token
4. Server calls ServerCtx::step(&binary_token) → either:
   a. Some(response_token) → continue handshake (send response back)
   b. None → handshake complete, client authenticated
5. Server extracts client principal via server.sender_name()?.display()
6. Server maps principal → UserToken → RBAC roles
```

### Client-side flow

The OPC UA client does NOT need new code. It uses the OS GSSAPI library:
1. Client calls `gss_init_sec_context` with target SPN (e.g., "opcua/hostname@REALM")
2. Client encodes the resulting GSSAPI token as base64
3. Client sends it as `IssuedToken.tokenData`

The OPC UA client SDK does not need changes for token acquisition — that's the client application's responsibility. The `async-opcua-client` crate already supports `IssuedToken` identity.

## Token Format in OPC UA IssuedToken

### Decision: Base64-encoded GSSAPI context-level token

**Rationale**: OPC UA `IssuedToken.tokenData` is a `ByteString` (Part 4 §7.38.7). The GSSAPI token is a binary DER-encoded ASN.1 structure (RFC 2743 §3.1). We base64-encode it for transport in the OPC UA binary protocol.

The `OAuth2IdentityValidator::validate_token(&self, token: &str) -> Result<ClaimProfile, StatusCode>` method receives the token as a string. The GSSAPI implementation will:
1. Decode from base64
2. Pass the raw bytes to `ServerCtx::step()`
3. Extract the client principal
4. Build a `ClaimProfile` with `username = principal_name`

### Single-step vs multi-step handshake

GSSAPI supports multi-step context establishment (client sends token → server responds → client sends next token). In practice, Kerberos tickets are typically single-step. We implement multi-step support (loop until `None`) but assert in tests that Kerberos is single-step.

## Platform Dependencies

### Linux

- System package: `libkrb5-dev` (Debian/Ubuntu), `krb5-devel` (RHEL)
- Build-time: `pkg-config` for `krb5`
- Runtime: `libkrb5.so` (MIT Kerberos) or `libgssapi.so` (Heimdal)
- Keytab: `/etc/krb5.keytab` or `$KRB5_KTNAME`

### macOS

- Built-in: Apple GSS Framework (no extra packages)
- Keytab: `/etc/krb5.keytab`

### Windows (future)

- Would use `cross-krb5` which wraps `sspi` on Windows
- Keytab handled by Active Directory

## Security Considerations

### Decision: Token size limit, spawn_blocking, timeout

**Rationale**: GSSAPI tokens are attacker-controlled network input.

**Key findings**:
- **Token size**: GSSAPI/Kerberos tickets are typically < 8KB. We impose a 64KB maximum before any allocation or FFI call. Beyond that, reject with `BadIdentityTokenRejected`.
- **Blocking calls**: `gss_accept_sec_context` may perform DNS lookups and KDC communication. It MUST be wrapped in `tokio::task::spawn_blocking` to avoid blocking the async runtime.
- **Timeout**: Each GSSAPI step must have a timeout (5 seconds). If the step doesn't complete, the context is cleaned up and the connection rejected.
- **Memory**: GSSAPI internally allocates buffers. The C library manages these; the Rust wrapper frees them via `Drop`. No Rust-side allocation beyond the token copy.
- **Thread safety**: `ServerCtx` is `Send` but `!Sync`. Create one context per connection (natural fit for async).

## Trait Integration

### Decision: Implement `OAuth2IdentityValidator` for GSSAPI

**Rationale**: The trait name is OAuth2-specific but the method signature `fn validate_token(&self, token: &str) -> Result<ClaimProfile, StatusCode>` is general-purpose. Rather than refactor the trait name (which would touch JWT, RSA-KEM, and IssuedToken dispatch), we implement the same trait for GSSAPI. The trait may be renamed to `IdentityTokenValidator` in a future cleanup.

The `KerberosValidator` struct holds:
- The service principal name (for `gss_acquire_cred`)
- A tokio runtime handle (for `spawn_blocking`)

Each `validate_token` call creates a fresh `ServerCtx`, processes the handshake, and drops the context — no persistent state.

**Alternatives considered**:
- **New trait**: Would bifurcate the validation pipeline in `info.rs`. The dispatch code would need to know which trait to call — adding complexity without benefit.
- **Bypass the trait entirely**: Direct GSSAPI calls in `info.rs` — violates separation of concerns.
- **Rename the trait**: Clean but touches 5+ files and complicates review. Defer to a cleanup PR.
