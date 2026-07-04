# Contract: Per-Endpoint Certificate Configuration

**Feature**: 057-completeness-closeout / US2
**Part of**: `async-opcua-server::config`

## Public API

### ServerEndpoint (extended fields)

```rust
pub struct ServerEndpoint {
    // existing fields unchanged
    pub path: String,
    pub security_policy: String,
    pub security_mode: String,
    pub security_level: u8,
    pub password_security_policy: Option<String>,
    pub user_token_ids: BTreeSet<String>,

    // NEW: per-endpoint certificate overrides
    #[serde(default)]
    pub certificate_path: Option<PathBuf>,
    #[serde(default)]
    pub private_key_path: Option<PathBuf>,
}
```

### Endpoint Certificate Resolution

```
Resolution order:
1. If endpoint.certificate_path is Some → use it
2. Else if server_config.certificate_path is Some → use it (backward compatible)
3. Else → no certificate for this endpoint
```

### ServerConfig (unchanged)

```rust
pub struct ServerConfig {
    // These serve as defaults for endpoints without explicit certs
    pub certificate_path: Option<PathBuf>,       // UNCHANGED
    pub private_key_path: Option<PathBuf>,       // UNCHANGED
    // ...
}
```

## Invariants

- **Single-cert backward compat**: A config with `ServerConfig::certificate_path` set and no `ServerEndpoint::certificate_path` fields behaves identically to the current release.
- **Startup validation**: Before binding, iterate all security-policy endpoints. If any endpoint has `security_policy != "None"` and no valid certificate resolves (per the resolution order above), panic at startup with a diagnostic message: `"Endpoint {path} uses security policy {policy} but no compatible certificate is configured."`
- **Key type validation**: An RSA cert assigned to an ECC policy endpoint (or vice versa) is detected at startup. The cert's key algorithm (RSA vs EC) must match the policy's required algorithm.
- **WSS parity**: The opc.wss transport uses the same per-endpoint cert resolution — the transport layer reads `EndpointIdentifier` and looks up the cert map.

## Internal Changes

### ServerInfo

```rust
// BEFORE (current):
pub server_certificate: RwLock<Option<X509>>,

// AFTER (target):
pub endpoint_certificates: RwLock<HashMap<EndpointIdentifier, Option<X509>>>,
```

### Secure Channel Creation

At `create_secure_channel` in `session/manager.rs`, instead of:
```rust
let server_cert = info.server_certificate.read().clone();
```
Use:
```rust
let endpoint_id = EndpointIdentifier { path, security_policy, security_mode };
let cert_map = info.endpoint_certificates.read();
let server_cert = cert_map.get(&endpoint_id).cloned().flatten();
```

### Test Fixtures

All existing test fixtures that set `*handle.info().server_certificate.write() = Some(cert)` must be updated to insert into the `endpoint_certificates` map. The test helper `make_cert_and_key` already generates valid X509 — it just needs to be associated with the correct endpoint identifier.
