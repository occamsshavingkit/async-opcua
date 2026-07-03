# Research: Optional Dependencies and Security Hardening (055)

## R1: RSA-DH algorithm for UserToken encryption

**Decision**: Implement RSA-KEM (Key Encapsulation Mechanism) per OPC 10000-6 §6.7.3.

**Rationale**: Part 6 defines two RSA-based encryption algorithms for UserIdentityTokens:
- RSA-OAEP (already implemented) — uses OAEP padding for key transport
- RSA-KEM (not yet implemented) — uses a key encapsulation mechanism

RSA-KEM works as follows:
1. Client generates a random symmetric key (AES-256)
2. Client encrypts the symmetric key with the server's RSA public key using RSAES-OAEP
3. Client wraps the UserIdentityToken with AES-256-KeyWrap using the symmetric key
4. Server decrypts the symmetric key with its RSA private key
5. Server unwraps the UserIdentityToken

The crypto backend (`aws-lc-rs`) already supports RSA-OAEP encryption/decryption. Adding RSA-KEM requires an AES key-wrap implementation which is already present in the crypto crate.

**Alternatives considered**: None — this is a spec-mandated algorithm for interop.

## R2: Feature flag design for optional dependencies

**Decision**: Add `pubsub` and `history-sqlite` as default-ON Boolean features on the `async-opcua` umbrella crate. Profile aliases (`nano`, `micro`, `embedded`, `standard`) do NOT enable them. `server` and `base-server` DO enable them (backward compatibility).

**Rationale**:
- Follows the same pattern as the 15 subsystem gates from feature 054
- Default ON preserves the current surface for existing users
- Profile aliases explicitly opt out — the profiles are minimal-build surfaces
- `cargo add async-opcua` without feature flags still gets pubsub + history-sqlite (unchanged)

**Dependency wiring**:
```toml
# async-opcua/Cargo.toml
pubsub = ["dep:async-opcua-pubsub"]
history-sqlite = ["dep:async-opcua-history-sqlite"]

# aliases
nano = ["dep:async-opcua-server", "dep:async-opcua-nodes"]  # no pubsub/history-sqlite
server = ["base-server", "generated-address-space", "pubsub", "history-sqlite"]
```

## R3: Security check registry design

**Decision**: In-memory ring buffer on `ServerInfo`, exposed through `ServerHandle`, bounded by a configurable max count (default 1000).

**Rationale**:
- Avoids external dependencies (no database, no file I/O)
- Bounded memory — cannot grow unboundedly under attack
- Co-located with existing diagnostics infrastructure
- Queryable via `ServerHandle` for tests and monitoring tools

**Entry structure**:
```
timestamp: DateTime
category: SecurityCheckCategory (CertificateValidation | UserAuthentication | ChannelNegotiation | RbacDecision)
outcome: SecurityCheckOutcome (Pass | Fail)
reason: StatusCode
identity: String (application URI, user name, or session ID)
```

**Alternatives considered**:
- File-based audit log: adds I/O dependency, slower, complicates testing — rejected for v1
- OPC UA AuditEvent emission: already done in the audit module; the registry is a complementary query interface
