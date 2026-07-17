# Data Model: GDS Push Model Fix + Completion (Run 1)

## `PushTransaction` (new, `push_methods.rs`)

The single staged-but-not-yet-applied certificate change created by
`UpdateCertificate`, resolved by `ApplyChanges`/`CancelChanges`.

```rust
struct PushTransaction {
    owning_session_id: u32,
    certificate_group_id: NodeId,
    certificate_type_id: NodeId,
    certificate_der: Vec<u8>,
    private_key_pem: Option<Vec<u8>>,
}
```

Held as `RwLock<Option<PushTransaction>>` on the push-method registry —
at most one active transaction server-wide, matching the "Servers... are
expected to support exactly one active transaction" statement in
OPC-10000-12 §7.10.2.

## `GdsSigningRequestRegistry` (existing, repurposed)

The existing `signing_requests`/`created_requests` fields tracked mock
CSR bookkeeping for the (removed) `StartSigningRequest` and the
previously-mocked `CreateSigningRequest`. Replaced with:

```rust
pub struct GdsPushRegistry {
    transaction: RwLock<Option<PushTransaction>>,
}
```

(The old `GdsSigningRequestRegistry`/`GdsSigningRequest`/
`GdsCreatedSigningRequest` types are removed — they existed only to
support the mocked/misplaced methods being removed.)

## Method NodeIds (verified, `push_methods.rs` constants)

```rust
const SERVER_CONFIGURATION_OBJECT_ID: u32 = 12637;
const CREATE_SIGNING_REQUEST_METHOD_ID: u32 = 12737;
const UPDATE_CERTIFICATE_METHOD_ID: u32 = 13737;
const APPLY_CHANGES_METHOD_ID: u32 = 12740;
const CANCEL_CHANGES_METHOD_ID: u32 = 25708;
const GET_REJECTED_LIST_METHOD_ID: u32 = 12777;
const RESET_TO_SERVER_DEFAULTS_METHOD_ID: u32 = 25709;
```

## `CertificateStore::read_rejected_certs` (new, `async-opcua-crypto`)

```rust
pub fn read_rejected_certs(&self) -> Vec<X509>
```

Mirrors `read_trusted_certs`/`read_issuer_certs` exactly, reading
`self.rejected_certs_dir()`.
