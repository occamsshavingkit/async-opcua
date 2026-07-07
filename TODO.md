# TODO

Ideas that could be implemented.

## Remaining

- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool on Windows. See `docs/ctt-conformance.md`.

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
