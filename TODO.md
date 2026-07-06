# TODO

Ideas that could be implemented.

## Remaining

- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool on Windows. See `docs/ctt-conformance.md`.
- **Kerberos SSO integration test & CI**: write end-to-end integration test with a local KDC, add KDC setup to CI playbook (feature 064, T008-T009 deferred).
- **Kerberos SSO: expose keytab path to GSSAPI**: currently the server relies on `KRB5_KTNAME` env var; the `keytab_path` config field needs to be plumbed through to GSSAPI's `gss_acquire_cred_from` (feature 064).

## Done

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
