# Data Model: GDS Pull Model Client-Side Fix (Run 2)

## GdsRegistrationClient (rewritten)

`async-opcua-client/src/gds/registration.rs`

| Field | Before | After |
|---|---|---|
| `directory_object_id` | `NodeId::new(0, 22384)` via hardcoded `Default`/`new()` | Real, discovered `NodeId`, passed explicitly to a new `new(directory_object_id, register_method_id)` constructor |
| `register_method_id` | `NodeId::new(0, 22385)` | Real, discovered `NodeId` |

`register_application(...)` — unchanged signature and body (already just
dispatches a `CallMethodRequest` against `self.directory_object_id`/
`self.register_method_id`).

`Default`/hardcoded `new()` **removed** — a client with fabricated NodeIds
was never valid to construct; only `GdsClient::discover` (or explicit,
caller-supplied real NodeIds via the new constructor) produces a usable one.

## GdsCsrClient (rewritten)

`async-opcua-client/src/gds/csr.rs`

| Field | Before | After |
|---|---|---|
| `certificate_manager_id` | `NodeId::new(0, 22388)`, wrongly modeled as a separate "CertificateManager" object | **Renamed** `directory_object_id`; the same real Directory `NodeId` `GdsRegistrationClient` holds (one object, not two — see research.md) |
| `start_signing_request_id` | `NodeId::new(0, 22400)` | Real, discovered `NodeId` |
| `finish_signing_request_id` | `NodeId::new(0, 22402)`, calling a method the client itself names "FinishSigningRequest" | Real, discovered `NodeId` for the actual method, `FinishRequest` (§7.9.5) — field name unchanged (`finish_signing_request_id`) since it's this client's own internal naming and `poll_signing_request`'s public name is unaffected by this fix (not requested, avoids unnecessary churn) |

`start_signing_request(...)`/`finish_signing_request(...)` — unchanged
signatures and bodies (already just dispatch `CallMethodRequest`s against
the held NodeId fields).

`Default`/hardcoded `new()` **removed**, same reasoning as
`GdsRegistrationClient`.

## GdsClient (extended)

`async-opcua-client/src/gds/gds_client.rs`

New:

```rust
impl GdsClient {
    /// Discovers the real GDS Directory object and its RegisterApplication/
    /// StartSigningRequest/FinishRequest method NodeIds against `session`,
    /// via the target server's own namespace array and TranslateBrowsePathsToNodeIds
    /// (OPC UA Part 4 §5.8.4) -- the standard mechanism for resolving a
    /// well-known node whose namespace index isn't known in advance.
    ///
    /// Fails closed (a specific `Error`, never a panic) if the server doesn't
    /// expose the GDS companion namespace, or if any expected node is missing.
    pub async fn discover(session: &Session) -> Result<Self, Error>;
}
```

`Default`/`new()` (the old hardcoded-NodeId constructor) **removed** — a
`GdsClient` can no longer be constructed with fabricated defaults.
`from_parts(registration, csr)` **retained** (explicit construction from
already-known-real NodeIds remains a legitimate use case — e.g. a caller
that already discovered/cached NodeIds out-of-band).

`register_application`/`request_signing_csr`/`poll_signing_request` —
unchanged signatures and bodies (delegate to the held sub-clients, per
FR-006).

## No new entities

Discovery does not introduce a new public struct — the four resolved
`NodeId`s flow directly into the existing `GdsRegistrationClient`/
`GdsCsrClient` fields via their new explicit constructors.
