# Research: Client Kerberos SSO

## GSSAPI Client Context

### Decision: Use `libgssapi::context::ClientCtx` for service ticket acquisition

**Rationale**: The same `libgssapi` crate used by the server-side validator provides `ClientCtx` for initiating GSSAPI contexts. The client:
1. Calls `ClientCtx::new()` with the target SPN
2. Calls `step()` to get the initial GSSAPI token
3. Base64-encodes it with `GSSAPI ` prefix
4. Wraps it in `IdentityToken::IssuedToken`

Kerberos tickets are single-step — the `step()` call returns the token immediately.

**Alternatives considered**:
- **Shell out to `kinit`/`kvno`**: Fragile, platform-specific, requires system binaries.
- **New `gssapi` feature on the client**: Unnecessary — the existing `kerberos` feature already forwards to `async-opcua-crypto/kerberos`. Just add `kerberos` to the client feature list.

### Token Format

The client produces the same token format the server expects:
```
GSSAPI <base64-encoded GSSAPI context token>
```

The server strips the `GSSAPI ` prefix, decodes base64, and passes to `ServerCtx::step()`.

## Session Integration

### Decision: Store SPN in ClientBuilder, acquire ticket at session creation

**Rationale**: The client connects to a known server endpoint. The SPN is known at build time (`OPCUA/hostname@REALM`). The GSSAPI context is short-lived — acquire during `Session::connect()`, wrap as `IdentityToken::IssuedToken`, and pass to `ActivateSession`.

Users who prefer manual control can call `acquire_kerberos_token()` directly and pass the resulting `IdentityToken` to the session builder.

## Thread Safety

GSSAPI calls may block (DNS, KDC communication). Run in `std::thread::spawn` with a 5-second timeout, same as the server validator.
