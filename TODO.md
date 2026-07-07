# TODO

Ideas that could be implemented.

## Remaining

- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool on Windows. See `docs/ctt-conformance.md`.
- ~~**Technical debt quick wins**: fix companion feature-name drift between `async-opcua-server/Cargo.toml` and `async-opcua-server/src/companion/mod.rs`; remove or narrow the `unexpected_cfgs` allowance; add a consistency check for declared companion features versus importer gates.~~
- ~~**Spec Kit reconciliation**: update `specs/069-companion-specs/tasks.md` and related plan/spec artifacts so they match the current companion-spec implementation strategy, or document the intended migration path if runtime XML import is temporary.~~
- ~~**Documentation cleanup**: update `docs/compatibility.md` so the crypto backend description matches the current `aws-lc-rs`/`rustls`/Rust-crypto implementation instead of stale OpenSSL wording.~~
- **Async lock audit**: audit synchronous lock use in session, subscription, secure-channel, and SQLite history paths; confirm lock guards are not held across `.await`; add targeted high-contention tests where needed.
- **Dependency debt review**: revisit `deny.toml` advisory exceptions and duplicate dependency stacks (`tokio-tungstenite`, `thiserror`, `rand`, `getrandom`, `rustix`, async runtime support crates); collapse versions where upstream compatibility allows.
- **Companion import strategy decision**: decide and document whether companion specs should use generated Rust, runtime XML imports, or a hybrid model; measure build time and footprint before enabling broad companion support.
- **Protocol TODO conversion**: turn high-impact protocol TODOs into grounded implementation tasks with OPC UA section references, including query partial-success behavior, `Variant::set_range_of` completeness, and event filter `RelatedTo` support.
- **Large-module refactoring backlog**: split the largest handwritten modules only around stable domain boundaries, starting with subscription service/client code, memory node manager internals, session manager, secure channel, and subscription state handling.
- **Debt KPI tracking**: track generated-code size, non-generated LOC hotspots, build time, binary footprint, dependency advisories, and feature-lattice failures as recurring technical debt metrics. See `docs/technical-debt-report-2026-07-07.md`.

## Done

- ~~Shrink foundation profile footprints~~ — feature 066: nano 12M→6.8M, micro 13M→7.3M, embedded 26M→17M (43-45% reduction via opt-level=z + LTO + strip).
- ~~Kerberos SSO: keytab path plumbing~~ — feature 065.
- ~~Kerberos SSO: integration test & CI KDC setup~~ — feature 065.
- ~~Kerberos SSO: GssapiIdentityValidator, feature flag, builder API, IssuedToken dispatch, role mapping~~ — feature 064.
- ~~Replace per-request timers with shared deadline queue~~ — feature 063 (US3).
- ~~Cache session Arc in request dispatch context~~ — feature 063 (US2).
- ~~Investigate ArcSwap debt overhead~~ — feature 063 (US4): 3 of 4 ArcSwap instances were startup-only and replaced with plain `Arc<T>`.
- ~~Split AddressSpace hot/cold: expose DashMap directly for reads~~ — feature 063 (US1).
- ~~Flesh out the server and client SDK with tooling for ease of use.~~ — feature 058 (QuickNodeManager builder API).
- ~~Make it even easier to implement custom node managers.~~ — feature 058.
- ~~RSA-KEM encrypted UserName token integration test~~ — feature 058.
- ~~Embedded profile secure channel smoke test~~ — feature 058.
- ~~Standard profile X509/RegisterServer2 tests~~ — feature 058.
- ~~Throughput benchmark regression: investigate and restore performance baseline.~~ — features 060 (compilation optimization, +11%) and 061 (hot-path audit fixes, allocation/caching/validation).
