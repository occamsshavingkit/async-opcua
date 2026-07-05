# TODO

Ideas that could be implemented.

## Remaining

- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool on Windows. See `docs/ctt-conformance.md`.

- **Replace per-request timers with shared deadline queue**: each inflight request spawns a `tokio::time::sleep_until` that costs ~2.8% CPU (`TimerEntry::drop` + `TimerEntry::reset`). Replace with a single shared deadline queue checked once per event loop tick in `controller.rs::run()`. Grounding: perf profile on single-core read benchmark (81.7k ops/sec), feature 062.

- **Cache session Arc in request dispatch context**: `SessionManager::find_by_token` costs ~2.4% CPU per request. The token→session mapping doesn't change during a request's lifetime. Cache the looked-up `Arc<RwLock<Session>>` in the request dispatch path so subsequent operations on the same request reuse it. Grounding: perf profile, feature 062.

- **Investigate ArcSwap debt overhead**: `arc_swap::Debt::pay_all` at ~2.5% CPU. ArcSwap wraps hot-path shared state (likely `Arc<ServerInfo>` or diagnostics config). If writes are rare, consider replacing with a plain `Arc` plus an occasional atomic reload, or batch reads across requests. Grounding: perf profile, feature 062.

- **Split AddressSpace hot/cold: expose DashMap directly for reads** (lock audit finding 1): `AddressSpace` in `async-opcua-server/src/address_space/mod.rs` is wrapped in `Arc<RwLock<>>` but its core `node_map` is already a `DashMap` (lock-free sharded concurrent map). Every Read acquires the outer `parking_lot::RwLock::read()` just to call `DashMap::get()` underneath — pure overhead that causes cross-core cache-line bouncing of the lock state word. `namespaces` is read-only after server startup; `references` and `browse_name_index` aren't touched by simple Read. Fix: expose `NodeMap` directly as `Arc<DashMap<NodeId, NodeType>>` for the read path, keep cold fields behind `RwLock<AddressSpaceCold>`. ~3 lines changed per read site in `async-opcua-server/src/node_manager/memory/mod.rs`. Estimated 2–3% per-core win, compounding with concurrent connections. Grounding: lock audit + perf profile, feature 062.

## Done

- ~~Flesh out the server and client SDK with tooling for ease of use.~~ — feature 058 (QuickNodeManager builder API).
- ~~Make it even easier to implement custom node managers.~~ — feature 058.
- ~~RSA-KEM encrypted UserName token integration test~~ — feature 058.
- ~~Embedded profile secure channel smoke test~~ — feature 058.
- ~~Standard profile X509/RegisterServer2 tests~~ — feature 058.
- ~~Throughput benchmark regression: investigate and restore performance baseline.~~ — features 060 (compilation optimization, +11%) and 061 (hot-path audit fixes, allocation/caching/validation).
