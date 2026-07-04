# OPC UA conformance gap backlog

_Last updated 2026-07-04._

## Closed

All previously identified gaps (Tiers 1-3) are closed:

- **Security/PKI**: certificate validation (chains, CRL, OCSP), ActivateSession channel binding, ECC encrypted secrets — all done.
- **Encoding edges**: NumericRange multi-dimensional, JSON encoding edges, JSON DateTime full precision — all done.
- **Optional facets**: writable address space / NodeManagement, Query (client API + e2e), RegisterServer/RegisterServer2, FindServersOnNetwork / LDS-ME mDNS — all done.
- **Methods / Auditing**: typed method callbacks, full Audit*EventType hierarchy — all done.
- **Security audit round 2**: OAuth2/JWT issuer pinning, PubSub Part-14 message security (AES-CTR + HMAC), safety SPDU fail-safe — all done or verified no-fix.

## Remaining

- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool (CTT) on Windows. The conformance smoke harness (`async-opcua/tests/integration/conformance.rs`) covers the full (security policy × mode × identity-token) matrix as a regression proxy, but the authoritative CTT pass would surface behavioral gaps (status codes, edge cases) that the smoke harness cannot. See `docs/ctt-conformance.md` for the run guide.
- **Live third-party PubSub CTR interop**: ~~Live interop with OPC Foundation .NET stack implemented~~ — Direction 1 (Rust publishes, .NET subscribes) and Direction 2 (.NET publishes, Rust subscribes) both run via `run-dotnet.sh --pubsub` in CI. AES-CTR PubSub message security has been verified with spec-anchored KAT vectors and an independent-implementation interop cross-check.
