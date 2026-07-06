# Quickstart: Kerberos SSO Authentication

## Prerequisites

- Rust toolchain (stable)
- MIT Kerberos development libraries:
  ```bash
  sudo apt-get install -y krb5-kdc krb5-admin-server libkrb5-dev
  ```
- A Kerberos realm (can be set up locally via `krb5_newrealm`)
- Git checkout on branch `064-kerberos-sso`

## Quick Local Test Setup

### 1. Set up a test Kerberos realm

```bash
# Create realm (interactive, use PLANT.LOCAL as realm name)
sudo krb5_newrealm

# Create a test user principal
sudo kadmin.local -q "addprinc -pw testpass operator1"

# Create a service principal and export keytab
sudo kadmin.local -q "addprinc -randkey OPCUA/localhost"
sudo kadmin.local -q "ktadd -k /tmp/opcua.keytab OPCUA/localhost"
sudo chmod 644 /tmp/opcua.keytab

# Set keytab env var
export KRB5_KTNAME=/tmp/opcua.keytab
```

### 2. Build the server

```bash
cargo build --features kerberos --bin async-opcua-demo-server
```

### 3. Run the server

```bash
KRB5_KTNAME=/tmp/opcua.keytab \
  cargo run --features kerberos --bin async-opcua-demo-server
```

### 4. Test with a Kerberos ticket

```bash
# Get a TGT
echo "testpass" | kinit operator1@PLANT.LOCAL

# Generate a service ticket (via GSSAPI) and send as IssuedToken
# (test client TBD — see integration test below)

# Destroy tickets
kdestroy
```

## Run Tests

```bash
# Full test suite (must pass)
cargo test --all-features

# Kerberos-specific tests (require local KDC)
KRB5_KTNAME=/tmp/opcua.keytab cargo test --features kerberos -- --test-threads=1
```

## Configuration Reference

### Server config (YAML / code)

```yaml
kerberos:
  spn: "OPCUA/hostname.example.com@PLANT.LOCAL"
  keytab: "/etc/opcua.keytab"
  roles:
    "engineer3@PLANT.LOCAL": ["Engineer"]
    "supervisor1@PLANT.LOCAL": ["Supervisor"]
    "operator1@PLANT.LOCAL": ["Operator"]
```

### Builder API

```rust
use async_opcua_server::ServerBuilder;

let server = ServerBuilder::new()
    .kerberos_spn("OPCUA/hostname.example.com@PLANT.LOCAL")
    .kerberos_keytab("/etc/opcua.keytab")
    .kerberos_principal_role("engineer3@PLANT.LOCAL", "Engineer")
    .build()?;
```

## Implementation Order

1. **KerberosValidator**: Implement `OAuth2IdentityValidator` for GSSAPI token validation.
2. **Server config**: Add `KerberosConfig` to `ServerBuilder` and `ServerInfo`.
3. **IssuedToken dispatch**: Wire `KerberosValidator` into `info.rs` IssuedToken handling.
4. **Integration test**: Set up a KDC in CI, run end-to-end Kerberos SSO test.
5. **CI playbook**: Add `libkrb5-dev` installation and KDC setup steps.

## Rollback

Kerberos support is behind the `kerberos` Cargo feature. To disable:
1. Remove the `kerberos` feature from the build
2. The server falls back to existing auth methods (Anonymous, UserName, X509, JWT IssuedToken)
3. No code or config changes needed on the client side
