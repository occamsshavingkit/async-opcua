# Contract: Feature flags (055)

## 1. Facade feature flags (`async-opcua`)

Two new Boolean feature flags, both default ON for backward compatibility:

```toml
[dependencies.async-opcua]
default-features = false
features = ["nano"]   # pubsub and history-sqlite are NOT pulled in
```

Guarantees:
1. `pubsub` and `history-sqlite` default ON — `cargo add async-opcua` behavior unchanged.
2. Profile aliases (`nano`, `micro`, `embedded`, `standard`) do NOT enable either flag.
3. `server` and `base-server` continue to enable both flags.
4. Neither flag has required dependencies on other flags; they are independently toggleable.
5. With `pubsub` off, types from `async-opcua-pubsub` are absent from the dependency tree
   (verified by `cargo tree`), not merely LTO-dead-stripped.

## 2. RSA-DH UserTokenPolicy

The server advertises an additional `UserTokenPolicy` variant when the endpoint's
certificate uses an RSA key:

- `tokenType`: USERNAME
- `securityPolicyUri`: `http://opcfoundation.org/UA/SecurityPolicy#Basic256Sha256`
  (or the matching RSA-based policy)

The server MUST NOT advertise RSA-DH policies on EC-only certificate endpoints.

## 3. SecurityCheckRegistry API

```rust
// On ServerHandle (pub):
fn security_checks(&self) -> Vec<SecurityCheckEntry>  // snapshot
fn security_check_count(&self) -> usize

// On ServerConfig (new field):
security_check_max_entries: usize  // default 1000
```

Recording is done internally by the server — no public mutation API.
