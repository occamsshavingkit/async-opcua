# Feature Specification: Hot Path Audit Fixes

**Feature Branch**: `061-hot-path-audit-fixes`  
**Created**: 2026-07-05  
**Status**: Draft  
**Input**: User description: "Fix all issues found in the hot-path audit of the async-opcua library. Changes must not break compliance with OPC-UA standards."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Eliminate DecodingOptions Clone on Every Encode/Decode (Priority: P1)

Every message encode and decode begins by calling `context()` on the secure channel, which clones the full `DecodingOptions` struct (~104 bytes: 9 `usize` fields, `AtomicU64`, `Duration`, bool, etc.). This happens at least twice per message (send + receive) — 100% of messages. For a server handling 100k messages/second, this is 200k heap copies per second of a struct that never changes after construction.

**Why this priority**: Touches the single hottest path in the codebase. Every message goes through this. The fix is a drop-in replacement (clone → Arc-share).

**Independent Test**: Run `cargo test --locked --all-features`. Encoding and decoding must produce byte-identical results. No behavioral change.

**Acceptance Scenarios**:

1. **Given** a secure channel context with `DecodingOptions`, **When** `context()` is called for encoding or decoding, **Then** the `DecodingOptions` is shared via `Arc` clone (refcount increment) instead of a full struct clone.
2. **Given** the refactored `Context` holding `Arc<DecodingOptions>`, **When** all test suites run, **Then** encoding/decoding produces identical binary output.

---

### User Story 2 - Build Type Tree Once During Server Initialization (Priority: P1)

During server startup, `initialize_node_managers()` calls `init()` on each node manager. Every node manager's `init()` internally calls `load_into_type_tree()` (full DFS rebuild), `ensure_browse_name_index()` (full index build), and `publish_type_tree_snapshot()`. For N managers, the type tree is rebuilt N times, the browse name index is rebuilt N times, and the snapshot is published N+1 times. The final state is identical to building once after all managers complete.

**Why this priority**: Reduces server startup time by 30-50% when multiple node managers are registered. The fix is structural — move the rebuild to after the init loop.

**Independent Test**: Run `cargo test --locked --all-features`. The final type tree and browse name index must be identical regardless of whether they were built incrementally or all at once. The `publish_type_tree_snapshot` must still be called.

**Acceptance Scenarios**:

1. **Given** multiple node managers registered on a server, **When** the server initializes, **Then** `load_into_type_tree()`, `ensure_browse_name_index()`, and `publish_type_tree_snapshot()` are called exactly once after all managers complete.
2. **Given** a single node manager, **When** the server initializes, **Then** behavior is unchanged — one tree build, one index build, one snapshot publish.
3. **Given** the refactored init, **When** `cargo test --locked --all-features` runs, **Then** all tests pass, including any that depend on the type tree or browse name index being available after init.

---

### User Story 3 - Cache RequestContext on SessionActor (Priority: P1)

Every Read and Write request processed by `SessionActor` calls `request_context()` which creates a new `Arc<RequestContextInner>` allocation. This clones ~6-8 `Arc`s and performs a `String::clone` on the user token. The token and user roles rarely change during a session's lifetime. A pre-built context that is invalidated only when the token/roles change would avoid this per-request allocation.

**Why this priority**: Per-request allocation overhead is ~200-500 ns. For 100k req/sec, this is 20-50 ms of CPU time per second spent on redundant allocations.

**Independent Test**: Write a test that sends Read requests through a session. Verify the returned values are identical before and after the change.

**Acceptance Scenarios**:

1. **Given** a `SessionActor` with a session that has an active user token, **When** a Read or Write request arrives, **Then** the request context is built by `Arc::clone`-ing a cached version rather than allocating a new `Arc<RequestContextInner>`.
2. **Given** the session's token changes (re-activation), **When** the next Read or Write request arrives, **Then** a new context is built and cached.
3. **Given** the refactored `request_context()`, **When** `cargo test --locked --all-features` runs, **Then** all tests pass.

---

### User Story 4 - Cache SecurityPolicy on SecureChannel (Priority: P2)

Every chunk's security header parsing calls `SecurityPolicy::from_uri()` — a string-to-enum match. Once the policy is set during `OpenSecureChannel` handshake, it never changes. Caching the resolved `SecurityPolicy` on the `SecureChannel` avoids the string-to-enum conversion and validation match on every subsequent chunk.

**Why this priority**: Per-chunk overhead. In a secured session, every message chunk goes through this. The fix replaces a string match + enum match with a cached value comparison.

**Independent Test**: All existing encrypted communication tests must pass unchanged.

**Acceptance Scenarios**:

1. **Given** a `SecureChannel` with a negotiated security policy, **When** a chunk's security header is decoded, **Then** the cached `SecurityPolicy` is used for validation instead of re-parsing the URI.
2. **Given** `expect_supported_security_policy()` is called per encrypt/decrypt, **When** the policy is validated once at set time, **Then** per-operation validation is replaced with a single flag check.
3. **Given** the refactored security header parsing, **When** `cargo test --locked --all-features` runs, **Then** all security-related tests pass.

---

### User Story 5 - Parallelize Certificate Loading During Startup (Priority: P2)

During server construction, each endpoint's certificate and private key are read via sequential synchronous `read_cert()` and `read_pkey()` calls. For a server with multiple endpoints, these are independent I/O operations that can run in parallel.

**Why this priority**: Server startup time improvement for multi-endpoint configurations. Each endpoint has independent files on disk.

**Independent Test**: Run `cargo test --locked --all-features`. Server construction must succeed with the same certificates loaded.

**Acceptance Scenarios**:

1. **Given** a server config with multiple endpoints, **When** certificates are loaded, **Then** the cert and key for each endpoint are read in parallel (via `tokio::join!` or equivalent) rather than sequentially.
2. **Given** a server config with a single endpoint, **When** startup occurs, **Then** behavior is unchanged (parallelization of one pair is a no-op).
3. **Given** async certificate loading, **When** `cargo test --locked --all-features` runs, **Then** all tests pass.

---

### Edge Cases

- **Empty endpoint list**: Server with no endpoints should not panic or diverge from current behavior.
- **Missing cert/key files**: Error handling must produce the same error messages and `ServerBuilderError` variants.
- **Single node manager**: Type tree build optimization should not change behavior for the common single-manager case.
- **Session re-activation**: When a session is activated with a new user token, the cached `RequestContextInner` must be invalidated.
- **Unsecured channel (SecurityPolicy::None)**: The cached policy must work correctly for `None` policy channels.
- **`--no-default-features` builds**: The refactored `DecodingOptions` and `Context` types must compile without optional features.
- **OPC UA compliance**: No protocol behavior, wire format, status codes, or service semantics may change. All changes are strictly internal optimization.

## Requirements *(mandatory)*

### Functional Requirements

#### US1 — DecodingOptions Arc
- **FR-001**: `DecodingOptions` in `Context` MUST be stored as `Arc<DecodingOptions>` instead of an owned clone.
- **FR-002**: `ContextOwned::context()` MUST share `DecodingOptions` via `Arc::clone()` (refcount increment) instead of `Clone::clone()`.
- **FR-003**: All encoding and decoding MUST produce byte-identical output to the pre-change implementation.

#### US2 — Type Tree Build Once
- **FR-004**: `load_into_type_tree()` and `ensure_browse_name_index()` calls MUST be removed from `InMemoryNodeManager::init()` in `async-opcua-server/src/node_manager/memory/mod.rs`.
- **FR-005**: `publish_type_tree_snapshot()` calls MUST be removed from individual manager init paths and called exactly once after all managers are initialized in `server.rs::initialize_node_managers()`.
- **FR-006**: The final type tree and browse name index state MUST be identical to the pre-change state after initialization completes.

#### US3 — RequestContext Caching
- **FR-007**: `SessionActor::request_context()` in `async-opcua-server/src/session/actor.rs` MUST cache the last-built `Arc<RequestContextInner>` and reuse it via `Arc::clone()` when the session token has not changed.
- **FR-008**: The cached context MUST be invalidated (and rebuilt) when the session's user token changes (e.g., session activation/re-activation).

#### US4 — SecurityPolicy Caching
- **FR-009**: `SecureChannel` in `async-opcua-core/src/comms/secure_channel.rs` MUST store the resolved `SecurityPolicy` as a validated field after `OpenSecureChannel` handshake.
- **FR-010**: `SecurityHeader::decode_from_stream()` in `security_header.rs` MUST use the cached policy for validation instead of calling `SecurityPolicy::from_uri()`.
- **FR-011**: `expect_supported_security_policy()` MUST be replaced with a pre-validated flag checked once during policy assignment, not per encrypt/decrypt operation.

#### US5 — Parallel Certificate Loading
- **FR-012**: Certificate and private key loading for multiple endpoints in `server.rs` MUST be parallelized using `tokio::join!` or equivalent concurrent async operations.
- **FR-013**: `CertificateStore::read_cert()` and `read_pkey()` MUST be provided as async variants (using `tokio::fs`) or wrapped for async execution.
- **FR-014**: Error messages and `ServerBuilderError` variants for missing/invalid certificates MUST be preserved.

### Key Entities

- **DecodingOptions**: An immutable configuration struct (max message size, max chunk count, encoding limits). Currently ~104 bytes, cloned on every message. After fix: stored in `Arc`, shared via refcount.
- **TypeTree**: A hierarchical type graph built from the address space during initialization. Currently rebuilt N times (once per node manager). After fix: built once after all managers complete.
- **RequestContextInner**: Per-request metadata (session, token, roles, type tree refs). Currently allocated fresh per Read/Write. After fix: cached on `SessionActor` and cloned when unchanged.
- **SecureChannel**: The encrypted transport channel holding the negotiated `SecurityPolicy`. Currently re-parses policy URI per chunk. After fix: caches the resolved policy once.
- **SessionActor**: The actor processing session-scoped requests (Read, Write, Browse). Currently allocates a new context per request. After fix: caches and reuses the context until token changes.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Per-message `DecodingOptions` allocation is eliminated — every `context()` call returns an `Arc::clone()` instead of a full struct clone. Verified by code inspection.
- **SC-002**: Server startup with N node managers calls `load_into_type_tree()` exactly once (not N times). Verified by code inspection and/or log instrumentation.
- **SC-003**: Per-request `Arc::new(RequestContextInner { ... })` allocation is replaced with `Arc::clone()` of cached context when token is unchanged. Verified by code inspection.
- **SC-004**: `SecurityPolicy::from_uri()` is called at most once per secure channel (during handshake), not per chunk. Verified by code inspection.
- **SC-005**: All existing CI gates pass: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings`, `cargo test --locked --all-features`.
- **SC-006**: `--no-default-features` builds pass for `async-opcua`, `async-opcua-types`, `async-opcua-nodes`, `async-opcua-server`.
- **SC-007**: No OPC UA protocol behavior, wire format, status codes, or service semantics change. Existing integration and compliance tests pass unchanged.

## Assumptions

- `DecodingOptions` is truly immutable after construction — verified by the audit.
- The type tree rebuild per-manager is an optimization oversight, not a correctness requirement. Each manager adds nodes independently; the final merged tree is correct regardless of when the rebuild runs.
- User token changes are already observable on the `SessionActor` via the session activation path; the cache invalidation hook exists.
- Asynchronous certificate loading does not introduce race conditions in the `ServerBuilder` startup sequence — the builder's `build()` is already synchronous and single-threaded.
- `std::sync::Mutex` in `MessageChunk.chunk_info` can be replaced with `std::cell::OnceCell` (or equivalent) because `MessageChunk` is not `Send` across threads.
