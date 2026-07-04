# Quickstart: Completeness Closeout

**Feature**: 057-completeness-closeout
**Date**: 2026-07-04

## Running the Example Servers

### Chat Server

```bash
cd samples/chat-server
cargo run
# Browse ChatLogs, call Post("Alice", "Hello"), see PostCount increment
```

### Chaos Server

```bash
cd samples/chaos-server
cargo run
# Browse for nodes that randomly change type/value/status
```

### Filesystem Bridge

```bash
cd samples/filesystem-bridge
cargo run -- --root /tmp
# Browse /tmp as an OPC UA hierarchy
```

### Reverse Bridge

```bash
# Start a source OPC UA server (e.g., demo-server)
cd samples/demo-server && cargo run &

# Start the reverse bridge pointing at the demo server
cd samples/reverse-bridge
cargo run -- --source opc.tcp://localhost:4855
# Browse for mirrored data
```

## Verifying Multi-Cert Support

```bash
# Start server with RSA + ECC endpoints
cargo run -- --config samples/server.conf
# RSA client connects to Basic256Sha256 endpoint
# ECC client connects to EccNistP256 endpoint
# Both succeed simultaneously
```

## Verifying OCSP Live Fetch

```bash
# Start server with OCSP strict mode
# (requires a certificate with AIA OCSP URL pointing to a live responder)
cargo run -- --config samples/server-ocsp.conf
# Test with valid cert → connection accepted
# Test with revoked cert → connection rejected
```

## Verifying LegacyCall Removal

```bash
# Confirm zero LegacyCall references
rgrep LegacyCall async-opcua-server/src/

# All subscription tests pass
cargo test -p async-opcua-server --lib
```

## CI Commands

```bash
# Full verification
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --all-features --locked
cargo test -p async-opcua-crypto --lib
cargo test -p async-opcua-server --lib
cargo check -p samples-chat-server
cargo check -p samples-chaos-server
cargo check -p samples-filesystem-bridge
cargo check -p samples-reverse-bridge
```
