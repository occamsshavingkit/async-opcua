# SESSION HANDOFF — 2026-07-03

## PR status

- https://github.com/occamsshavingkit/async-opcua/pull/259 — merged. All green. Clean `master`.

## What shipped

### Feature 054 (profile minimal builds)
- 15 subsystem cfg gates, 4 profile aliases (nano/micro/embedded/standard)
- Profile sizes: nano 6.45 MiB, micro 6.87 MiB, embedded 9.44 MiB, standard 15.97 MiB
- Profile behavior tests (isolated `cargo test -p <pkg> --features profile-tests`)

### Feature 055 (optional deps + RSA-KEM + security checks)
- `pubsub` and `history` default ON, profile aliases exclude both, `base-server` excludes both
- RSA-KEM identity token decryption (`crypto/src/identity/rsa_kem.rs` + RFC 3394 key wrap)
- `SecurityCheckRegistry` — bounded ring buffer on `ServerInfo`, exposed via `ServerHandle`

### #34 (controller.rs split)
- `controller.rs` 1351→1232 lines. Extracted `SessionStarter`, `SecureChannelState`, `ControllerCommand`.

### #240 (perf audit)
- Lock-tracing already optimized (049), audit events required by Part 4 §6.5.8. Closed.

## Backlog

### TODO.md active
- SDK tooling / easier custom node managers
- Sophisticated server with persistent store
- "Bad ideas" servers

### Deferred integration tests
- RSA-KEM encrypted token test (055, T008-T009) — needs two-phase client connect
- Embedded secure channel test (054, `#[ignore]`d) — same
- Standard X509/RegisterServer2 tests (054, `#[ignore]`d) — needs LDS peer

### GitHub
- #32 — Revisit `sad-rsa` for opt-in C-free RSA backend

### Profile-size-report (7 future-feature suggestions)
- `docs/profile-size-report.md`

## Next work: complexity cuts (2a/2b/6/7/8 from specs/complexity-cuts-backlog.md)

### 2a — is_subtype_of memoization with `moka`
- **File**: `async-opcua-nodes/src/type_tree.rs:97`
- **Caller**: `async-opcua-server/src/node_manager/view.rs:288`
- **Current**: O(R·T) per browse request (R = references, T = type-tree depth)
- **Fix**: cache `(parent, child) → bool` in `moka::sync::Cache`. Types are loaded at startup and immutable — no invalidation needed. Key insight: use `moka` (already a dependency) instead of hand-rolled.
- **At 1000 sessions with 100 monitored items each**: browse type-filtering walks the type tree once per reference.

### 2b — TranslateBrowsePaths `(parent, BrowseName)` index
- **File**: `async-opcua-server/src/node_manager/view.rs:759`, `node_manager/memory/mod.rs:366`
- **Current**: O(D·M·R) — nested scan per path element
- **Fix**: build `HashMap<(NodeId, String), Vec<NodeId>>` index. Invalidate on AddNodes/DeleteNodes/AddReferences/DeleteReferences (rare, only when `node-management` feature is enabled).
- **At 1000 sessions**: paths resolve in O(D) instead of O(D·M·R).

### 6 — CreateSession per-channel counter
- **File**: `async-opcua-server/src/session/manager.rs:~196`
- **Current**: O(sessions) scan per CreateSession handshake (capped by `max_sessions`)
- **Fix**: maintain `HashMap<u32, AtomicUsize>` (channel_id → count). Increment on activate, decrement on close/expiry.
- **At 1000 sessions**: still trivial. Value is at 50+ concurrent create/activate floods.

### 7 — subscription priority sort cache
- **File**: `async-opcua-server/src/subscriptions/session_subscriptions.rs:832`
- **Current**: O(S log S) re-sort every publish tick (100ms)
- **Fix**: keep a `BTreeSet<(priority, subscription_id)>` that updates incrementally on create/modify/delete. Only resort on priority change.
- **At 1000 subscriptions**: 1000 log 1000 = ~10k comparisons every 100ms. Not huge, but needless work.

### 8 — chunk header re-parse reuse
- **File**: `async-opcua-core/src/comms/chunker.rs:352, 506`
- **Current**: ChunkInfo parsed twice per chunk (validate pass + decode pass)
- **Fix**: parse once, reuse across both passes within the same message lifetime (no invalidation needed).
- **Risk**: zero — chunk data is immutable for the lifetime of a single message.

## Commands

```bash
# Local CI equivalent before pushing
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
RUSTFLAGS="-D warnings" cargo check --workspace
cargo test -p async-opcua-server --lib

# Profile verification
cargo tree -p async-opcua --no-default-features --features nano -e normal | grep -E 'pubsub|history-sqlite'  # must be empty
cargo tree -p async-opcua-minimal-server -e normal | grep 'core-namespace'  # must be empty

# Do NOT set auto-merge on PRs. Wait for all green, then merge manually.
```
