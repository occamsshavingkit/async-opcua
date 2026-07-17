# Data Model: GDS Push Model TrustList Completion (Run 2)

## TrustListFileHandle

Session-scoped state for one open TrustList file, cached in a
`moka::sync::Cache<u32, TrustListFileHandle>` with `time_to_live` set to
the TrustList's `ActivityTimeout` (default 60000ms), mirroring
`HistoryContinuationPointCache`.

| Field | Type | Description |
|---|---|---|
| `owning_session_id` | `u32` | The session that called `Open`/`OpenWithMasks`; every subsequent `Read`/`Write`/`Close`/`CloseAndUpdate` on this handle must come from the same session. |
| `mode` | `OpenFileMode` (bitmask) | `Read`, or `Write \| EraseExisting`; validated at Open time, nothing else is accepted. |
| `buffer` | `Vec<u8>` | For read mode: the pre-serialized `TrustListDataType` binary content (built once at Open/OpenWithMasks time from the current store state, filtered by mask). For write mode: accumulates bytes from successive `Write` calls. |
| `position` | `u64` | Current read/write offset, mutated by `Read`/`Write`/`GetPosition`/`SetPosition`. |

Handles are allocated a random, non-zero `u32` (avoiding collision with
any concurrently-open handle) and removed from the cache on `Close`/
`CloseAndUpdate`, on error paths that must not leave a stale handle
(malformed decode, validation failure), or by TTL expiry.

## PushTransaction (extended from Run 1)

Run 1's `PushTransaction` (`async-opcua-server/src/gds/push_methods.rs`)
currently holds only a pending certificate/key. Extended with an
additional optional field:

| Field | Type | Description |
|---|---|---|
| `owning_session_id` | `u32` | *(existing, Run 1)* Session that opened the transaction. |
| `certificate_der` | `Vec<u8>` | *(existing, Run 1)* Pending new application certificate. |
| `private_key_pem` | `Option<Vec<u8>>` | *(existing, Run 1)* Pending new private key, if any. |
| `pending_trust_list` | `Option<TrustListDataType>` | **NEW**: the decoded, mask-annotated pending TrustList change staged by `CloseAndUpdate`, if any. |

Only one `PushTransaction` exists server-wide at a time (Run 1's existing
invariant, unchanged). It may carry a pending certificate/key change, a
pending TrustList change, or (in principle) both if a single session
opened both flows before calling `ApplyChanges` -- `ApplyChanges` commits
whichever pending fields are set; `CancelChanges` discards all of them.
This requires no new mutual-exclusion logic beyond what Run 1 already
enforces (single transaction, single owning session).

## TrustListDataType (existing, generated)

Already defined in
`async-opcua-types/src/generated/types/trust_list_data_type.rs`; reused
as-is as both the wire format (file content) and the in-memory pending
representation:

| Field | Type | Description |
|---|---|---|
| `specified_lists` | `u32` | `TrustListMasks` bitmask: which of the four lists below are present/being replaced. |
| `trusted_certificates` | `Option<Vec<ByteString>>` | DER-encoded trusted application/CA certificates. |
| `trusted_crls` | `Option<Vec<ByteString>>` | DER-encoded CRLs for the trusted list. |
| `issuer_certificates` | `Option<Vec<ByteString>>` | DER-encoded CA certificates needed to validate trusted certificates. |
| `issuer_crls` | `Option<Vec<ByteString>>` | DER-encoded CRLs for the issuer list. |

## Validation rules

- `CloseAndUpdate`: every certificate in the new `trusted_certificates`
  list is validated via `CertificateStore::validate_application_instance_cert`
  (existing OPC-10000-4 validation path); first failure aborts the whole
  update with `Bad_CertificateInvalid` and discards the decoded buffer.
- `AddCertificate`: `is_trusted_certificate=false` is always rejected with
  `Bad_CertificateInvalid` per spec (`AddCertificate` cannot add issuer
  certificates); the certificate must parse as valid DER X.509.
- `RemoveCertificate`: thumbprint must match an existing trusted or issuer
  certificate (`Bad_InvalidArgument` if not); removal is refused with
  `Bad_CertificateChainIncomplete` if the certificate is a CA needed to
  validate another certificate still present in the same list.

## State transitions

```
(no handle) --Open(Read)--> ReadHandle --Read*--> ReadHandle --Close--> (no handle)
(no handle) --Open(Write|EraseExisting)--> WriteHandle --Write*--> WriteHandle
WriteHandle --CloseAndUpdate--> (no handle), PushTransaction.pending_trust_list = Some(..)
WriteHandle --Close--> (no handle), no change
any handle --TTL expiry--> (no handle), pending write buffer discarded

PushTransaction.pending_trust_list = Some(..) --ApplyChanges--> committed to CertificateStore, cleared
PushTransaction.pending_trust_list = Some(..) --CancelChanges--> discarded, cleared
```
