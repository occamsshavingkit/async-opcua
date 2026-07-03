# Data Model: Optional Dependencies and Security Hardening (055)

## Feature flags (async-opcua Cargo.toml)

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `pubsub` | Boolean | ON | Enables `async-opcua-pubsub` dependency |
| `history-sqlite` | Boolean | ON | Enables `async-opcua-history-sqlite` dependency |

### Alias compositions (updated)

| Alias | pubsub | history-sqlite | Change from 054 |
|-------|--------|---------------|-----------------|
| `nano` | OFF | OFF | New — explicitly disabled |
| `micro` | OFF | OFF | New — explicitly disabled |
| `embedded` | OFF | OFF | New — explicitly disabled |
| `standard` | OFF | OFF | New — explicitly disabled |
| `base-server` | ON | ON | Unchanged |
| `server` | ON | ON | Unchanged |

## Security Check Registry

### SecurityCheckEntry

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `DateTime` | When the check was performed |
| `category` | `SecurityCheckCategory` | What was being validated |
| `outcome` | `SecurityCheckOutcome` | Pass or Fail |
| `reason` | `StatusCode` | OPC UA status code for the result |
| `identity` | `String` | Affected client/user identifier |

### SecurityCheckCategory (enum)

| Variant | Description |
|---------|-------------|
| `CertificateValidation` | Client certificate trust/expiry/revocation check |
| `UserAuthentication` | UserName/X509/IssuedToken identity verification |
| `ChannelNegotiation` | SecureChannel security policy/mode negotiation |
| `RbacDecision` | Role-based access control result |

### SecurityCheckOutcome (enum)

| Variant | Description |
|---------|-------------|
| `Pass` | Check succeeded |
| `Fail` | Check failed (see reason StatusCode) |

### SecurityCheckRegistry

| Field | Type | Description |
|-------|------|-------------|
| `entries` | `VecDeque<SecurityCheckEntry>` | Ring buffer of recent entries |
| `max_entries` | `usize` | Configurable cap (default 1000) |
