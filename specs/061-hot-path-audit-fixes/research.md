# Research: Hot Path Audit Fixes

## US1 — DecodingOptions Arc

### Decision: Store `DecodingOptions` in `Arc` inside `Context`

**Rationale**: The audit confirmed `DecodingOptions` is immutable after construction (set in `SecureChannel::set_decoding_options()` once during channel setup). The `context()` method clones it on every encode/decode. `Arc::clone()` is a refcount increment (~5 ns) vs full struct clone (~50-100 ns for 104 bytes).

**Implementation**: Change `Context.options` from `DecodingOptions` to `Arc<DecodingOptions>`. `ContextOwned::context()` returns `Arc::clone(&self.options)`. All code that reads options via `ctx.options.field` continues to work through `Arc`'s `Deref`.

**Alternatives considered**:
- `Rc<DecodingOptions>`: Not `Send`, would break async usage.
- Split into mutable/immutable parts: More invasive. Arc is simpler and sufficient.
- `Copy` derive: Struct contains `AtomicU64` and `Duration` — not `Copy`-able.

---

## US2 — Type Tree Build Once

### Decision: Move type tree rebuilds out of per-manager `init()` into the calling loop

**Rationale**: The type tree aggregates all node managers' address spaces. Rebuilding after each individual manager produces intermediate states that are immediately invalidated by the next manager. Building once after all managers is correct because the merged result is identical — no manager depends on a fully-built type tree during its own init.

**Implementation**: Remove `load_into_type_tree()`, `ensure_browse_name_index()`, and `publish_type_tree_snapshot()` from `InMemoryNodeManager::init()`. In `server.rs::initialize_node_managers()`, after the loop, iterate all managers' address spaces to load into the type tree, build the browse name index, and publish the snapshot exactly once.

**Alternatives considered**:
- Partial/incremental type tree updates: Complex, would require tracking which nodes were added per-manager. The full rebuild is already O(nodes) and simple.

---

## US3 — RequestContext Caching

### Decision: Cache `Arc<RequestContextInner>` on `SessionActor` with token-version invalidation

**Rationale**: The `request_context()` method allocates a new `Arc<RequestContextInner>` per Read/Write. The token and user_roles change only during session activation. A cached version avoids the per-request allocation (~200-500 ns).

**Implementation**: Store `Option<Arc<RequestContextInner>>` and a `token_version: u64` on `SessionActor`. In `request_context()`, if the session's token matches the cached version, return `Arc::clone(&cached)`. On session activation/change, increment the version to force a fresh build.

**Alternatives considered**:
- Always-cache: Would require explicit invalidation hooks. Token version is simpler and correct.

---

## US4 — SecurityPolicy Caching

### Decision: Store validated policy on `SecureChannel`, check flag instead of re-matching

**Rationale**: `SecurityPolicy::from_uri()` is called per chunk to resolve a URI string to an enum. `expect_supported_security_policy()` is a 7-arm match per encrypt/decrypt. Both are invariant after channel setup.

**Implementation**: Add `validated_policy: Option<SecurityPolicy>` to `SecureChannel`, set during `set_security_policy()`. Replace `expect_supported_security_policy()` with a pre-validated `bool` flag. In `decode_from_stream()`, compare against cached policy instead of calling `from_uri()`.

**Alternatives considered**:
- Keep match but `#[inline]` it: The cost isn't the call overhead — it's the string comparison in `from_uri()`. Caching eliminates that.

---

## US5 — Parallel Certificate Loading

### Decision: Use `tokio::join!` to parallelize endpoint certificate I/O

**Rationale**: Each endpoint's cert and key are independent files. Reading them sequentially wastes I/O time. The `ServerBuilder::build()` path is synchronous, so the parallelism is within `spawn_blocking` or by making the cert loading async.

**Implementation**: Add `read_cert_async(path) -> Result<X509>` and `read_pkey_async(path) -> Result<PrivateKey>` using `tokio::fs::read`. In `server.rs`, collect cert+key read futures for all endpoints and `tokio::join!` them. If the build path must remain sync, use `tokio::runtime::Handle::current().block_on()`.

**Alternatives considered**:
- `rayon` for parallel sync I/O: Would work but introduces a new dependency. `tokio::fs` is already available.
