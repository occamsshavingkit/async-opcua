# Quickstart: Optional Dependencies and Security Hardening (055)

## Build with optional deps disabled

```bash
cargo build -p async-opcua-foundation-profile-nano-server
cargo tree -p async-opcua-foundation-profile-nano-server -e normal | grep -E 'pubsub|history-sqlite'
# Should produce NO output — neither crate is in the dependency tree
```

## Verify RSA-DH token encryption (after implementation)

```bash
cargo test -p async-opcua --test integration rsa_dh_username_token
```

## Inspect security check registry (after implementation)

```bash
# In a test or via ServerHandle:
let checks = handle.security_checks();
assert!(!checks.is_empty());
```
