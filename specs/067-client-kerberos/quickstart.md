# Quickstart: Client Kerberos SSO

## Prerequisites

- `libkrb5-dev` installed
- Valid Kerberos TGT (`klist` shows a ticket)

## Usage

```rust
use async_opcua_client::{ClientBuilder, IdentityToken};

// Auto-configuration via builder
let client = ClientBuilder::new("opc.tcp://hostname:4840")
    .kerberos_spn("OPCUA/hostname@PLANT.LOCAL")
    .build()
    .await?;

// Or manual: acquire token yourself
let token = async_opcua_client::identity_token::acquire_kerberos_token(
    "OPCUA/hostname@PLANT.LOCAL"
)?;
let client = ClientBuilder::new("opc.tcp://hostname:4840")
    .user_identity_token(token)
    .build()
    .await?;
```

## Build

```bash
cargo build --features kerberos -p async-opcua-client
```
