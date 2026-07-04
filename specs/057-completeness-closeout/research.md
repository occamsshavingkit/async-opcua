# Research: Completeness Closeout

**Feature**: 057-completeness-closeout
**Date**: 2026-07-04

## US1 — Live OCSP Revocation

### Decision: Build OCSP client on `ureq` (sync HTTP) + hand-rolled OCSP codec

**Rationale**: The certificate validation pipeline in `certificate_store.rs` runs synchronously during connection setup. Adding `reqwest` (async, tokio-dependent) would require threading the async runtime through the crypto crate — a disproportionate change. `ureq` is a minimal, pure-Rust sync HTTP client (~200KB) without async dependencies. OCSP requests/responses are ASN.1 DER-encoded structures already parseable with the existing `x509-cert` / `der` crates in the dependency tree.

**Alternatives considered**:
- `reqwest` (async): Requires tokio runtime in crypto crate — rejected as over-engineered.
- `ocsp-rs` crate: Smaller but wraps openssl which conflicts with aws-lc-rs — rejected.
- Custom OCSP codec: Acceptable because OCSP is a simple request/response protocol (SHA-1 hash of issuer cert + serial number → HTTP POST → signed response). RFC 6960 specifies the ASN.1 structures; `der` crate can handle them.

### Decision: Per-CertificateStore instance fetch policy, not per-check

**Rationale**: The OCSP fetch policy (off/soft/strict) is a server-level operational concern, not per-certificate. Adding it to the existing `CertificateStore` keeps configuration simple.

### Decision: Cache OCSP responses for their validity window

**Rationale**: RFC 6960 §2.2 mandates that OCSP responses include `thisUpdate` and `nextUpdate` fields defining their validity window. Caching within this window prevents hammering the responder. The cache is a `HashMap<CertId, CachedResponse>` with TTL-based eviction.

## US2 — Multi-Cert Mixed Server

### Decision: Add `certificate_path` and `private_key_path` fields to `ServerEndpoint`

**Rationale**: Per Part 4 §5.5.4.1, the certificate is a per-endpoint property. Adding optional fields to the existing `ServerEndpoint` struct is the minimal change. If an endpoint has no cert fields, it inherits from the server-level defaults (backward compatible).

**Current code state**:
- `ServerConfig` has `certificate_path: Option<PathBuf>` and `private_key_path: Option<PathBuf>` (single cert)
- `ServerEndpoint` has: path, security_policy, security_mode, security_level, password_security_policy, user_token_ids
- `ServerInfo` stores `server_certificate: RwLock<Option<X509>>` (single cert)

**Changes required**:
1. `ServerEndpoint` gains `certificate_path: Option<PathBuf>`, `private_key_path: Option<PathBuf>`
2. `ServerInfo` replaces `server_certificate: RwLock<Option<X509>>` with `endpoint_certificates: HashMap<EndpointIdentifier, Option<X509>>`
3. At startup (`server.rs`), load certs per endpoint; fall back to server-level defaults
4. At secure channel creation, look up cert by endpoint identifier
5. Validation: fail startup if any security-policy endpoint lacks a compatible cert

### Decision: No client crate changes

**Rationale**: The client already specifies which endpoint to connect to (by URL). The server's cert selection is transparent to the client. Confirmed by spec assumption.

## US3 — Delete LegacyCall

### Decision: Migrate all 27 `legacy()` call sites to dedicated enum variants

**Rationale**: Each call site follows the same pattern: capture inputs in a closure, send via `legacy(f)`, await the oneshot reply. Converting each to a dedicated `SubscriptionCommand` variant is mechanical. The code audit confirmed no borrow/lifetime blockers — all closures are `FnOnce + Send + 'static`.

**Current code state**:
- `SubscriptionCommand` enum has 3 variants: `LegacyCall`, `EnqueuePublish`, `Stop`
- `legacy()` method on `SubscriptionActorHandle` boxes the closure and sends it
- 27 call sites in `mod.rs`, 2 in `core.rs`
- Return types vary: `()`, `Result<_, StatusCode>`, `Option<_>`, `Vec<_>`, tuples

**Pattern for each variant**:
```rust
// Before:
handle.legacy(move |subs| subs.create_subscription(&request, &info)).await

// After:
CreateSubscription { request, info, response: oneshot::Sender<Result<u32, StatusCode>> }
```
The `response` oneshot channel carries the typed return value.

**Migration order**: Group by return type to minimize enum variant count. Operations returning the same type can share a variant if input data is the same, but per Principle III, each operation gets its own variant for clarity.

### Decision: Delete `legacy()` helper method entirely

**Rationale**: Once all call sites use dedicated variants, the `legacy()` method and the `LegacyCall` variant have zero callers. Removing them is a pure deletion with no behavioral change.

## US4 — Bad Ideas Example Servers

### Decision: Chat server uses hand-coded `ChatLog` structure type

**Rationale**: The `cactuaroid/OpcUaChatServer` model defines a `ChatLog` Structure (At/DateTime, Name/String, Content/String). async-opcua's XML importer can load NodeSet2 files, but generating the full NodeSet2 from the design XML requires the UA-ModelCompiler. Instead, we register the custom types manually using the existing `InMemoryNodeManager` APIs:
- `add_object_type` for `ChatLogsType` (extends BaseObjectType, SupportsEvents)
- `add_method` for `Post` (inputs: Name/String, Content/String)
- `add_variable` for `PostCount` (UInt32)
- `add_event_type` for `ChatLogEventType` (extends BaseEventType)
- Register `ChatLog` as a custom DataType/Structure using the existing type system

This demonstrates the SDK's extensibility without requiring external tooling.

### Decision: Each example server is a standalone crate in `samples/`

**Rationale**: Follows the pattern of `samples/persistent-store/` and `samples/demo-server/`. Each is a self-contained binary with its own `Cargo.toml` and `README.md`.

### Decision: No CI gate beyond compilation for example servers

**Rationale**: Per spec assumption — example servers are not production code. They may panic on edge cases. CI should verify `cargo check` pass but not run integration tests against them (they require client infrastructure, filesystem access, or network access).

### Decision: Filesystem bridge uses `notify` crate for live monitoring

**Rationale**: To be useful, the filesystem bridge should reflect changes in real time. The `notify` crate is the standard cross-platform filesystem watcher for Rust. It's already in the project's dependency tree (for other watchers).

### Decision: Reverse bridge uses async-opcua-client as a dependency

**Rationale**: The reverse bridge connects to another OPC UA server, creates subscriptions, and mirrors data. This exercises the client crate's subscription API and demonstrates SDK integration between the client and server halves.
