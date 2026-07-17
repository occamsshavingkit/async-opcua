# Research: GDS Push Model TrustList Completion (Run 2)

## NodeId verification (live Read against a running default-built server)

Same methodology as Run 1 (which caught the fabricated-NodeId bug):
`ReadValueId::from(NodeId)` defaults to the `Value` attribute, so
`Bad_AttributeIdInvalid` on an Object/Method node confirms the node
*exists* (just has no Value attribute), while `Bad_NodeIdUnknown` confirms
it does not exist. A temporary probe test (`probe_trustlist_certificate
group_nodeids` in `async-opcua/tests/integration/core_tests.rs`, added
and removed during this research phase) confirmed:

| Node | NodeId | Result |
|---|---|---|
| `DefaultApplicationGroup` (object) | `ns=0;i=14156` | `Bad_AttributeIdInvalid` -- exists |
| `DefaultApplicationGroup.TrustList` (object) | `ns=0;i=12642` | `Bad_AttributeIdInvalid` -- exists |
| `DefaultApplicationGroup.CertificateTypes` (variable) | `ns=0;i=14161` | `Good` -- exists |
| `DefaultApplicationGroup.GetRejectedList` (method) | `ns=0;i=23550` | `Bad_NodeIdUnknown` -- absent |
| `TrustList.Open` | `ns=0;i=12647` | exists |
| `TrustList.Close` | `ns=0;i=12650` | exists |
| `TrustList.Read` | `ns=0;i=12652` | exists |
| `TrustList.Write` | `ns=0;i=12655` | exists |
| `TrustList.GetPosition` | `ns=0;i=12657` | exists |
| `TrustList.SetPosition` | `ns=0;i=12660` | exists |
| `TrustList.OpenWithMasks` | `ns=0;i=12663` | exists |
| `TrustList.CloseAndUpdate` | `ns=0;i=12666` | exists |
| `TrustList.AddCertificate` | `ns=0;i=12668` | exists |
| `TrustList.RemoveCertificate` | `ns=0;i=12670` | exists |

Unlike Run 1's `CreateSelfSignedCertificate`/`DeleteCertificate`/
`GetCertificates`, every node this feature needs is real and reachable.
`GetRejectedList` at the CertificateGroup level is absent, matching Run
1's research.md finding -- already satisfied by
`ServerConfiguration.GetRejectedList`, no new work needed.

`DefaultHttpsGroup` (`ns=0;i=14088`) and `DefaultUserTokenGroup` instances
also exist as named constants with their own TrustList surfaces, but were
not live-verified this pass -- out of scope per spec.md Assumptions.

## Spec grounding (OPC-10000-12 §7.8.2 TrustLists, §7.8.3 CertificateGroups)

- **TrustListType** (§7.8.2.1): a `FileType` (OPC-10000-5) subtype. File
  content is a UA-Binary-encoded `TrustListDataType`, not wrapped in an
  `ExtensionObject`. `ActivityTimeout` defaults to 60000ms; the Server
  auto-closes and discards changes if this elapses between calls.
- **Open** (§7.8.2.2, mode semantics per the generated `OpenFileMode` enum
  in `async-opcua-types/src/generated/types/enums.rs`: `Read=1`,
  `Write=2`, `EraseExisting=4`, `Append=8`): only `Read` (0x01) and
  `Write|EraseExisting` (0x06) are supported; anything else is
  `Bad_NotSupported`. `Bad_TransactionPending` if Write-mode Open is
  requested while a transaction (shared with Run 1's certificate-rotation
  transaction) is already open on another session.
  `Bad_SecurityModeInsufficient` if the channel isn't authenticated.
- **OpenWithMasks** (§7.8.2.3): read-only; caller supplies a
  `TrustListMasks` bitmask (`TrustedCertificates=1`, `TrustedCrls=2`,
  `IssuerCertificates=4`, `IssuerCrls=8`, `All=15`) selecting which lists
  to include in the exported `TrustListDataType`.
- **Read**/**Write** (inherited from FileType): standard chunked
  byte-range operations against the open handle's buffer.
- **CloseAndUpdate** (§7.8.2.5): closes the file and stages the change.
  This project does not implement the separate CA/PullManagement
  transaction path, so (matching Run 1's precedent) it always sets
  `ApplyChangesRequired=TRUE` and stages the decoded `TrustListDataType`
  as a pending change on the shared transaction. The uploaded structure's
  `SpecifiedLists` mask controls which of the four lists are replaced
  (unset bits leave that list unchanged). Every certificate in the new
  `TrustedCertificates` list is validated before acceptance; on failure,
  `Bad_CertificateInvalid` and the whole update is discarded.
- **AddCertificate** (§7.8.2.6) / **RemoveCertificate** (§7.8.2.7):
  immediate, non-transactional (except `Bad_TransactionPending` if a
  write-mode transaction is already open elsewhere). `RemoveCertificate`
  returns `Bad_CertificateChainIncomplete` if the certificate is a CA
  still needed to validate another certificate in the list.
- All eight methods require an authenticated channel + SecurityAdmin role,
  matching every Push-model method from Run 1.

## Existing infrastructure reused

- `async-opcua-types/src/generated/types/trust_list_data_type.rs`:
  `TrustListDataType` struct (`specified_lists: u32` +
  `trusted_certificates`/`trusted_crls`/`issuer_certificates`/
  `issuer_crls: Option<Vec<ByteString>>`) already has generated
  `BinaryEncodable`/`BinaryDecodable` impls -- the file's byte content is
  exactly this type's binary encoding, no new serialization code needed.
- `async-opcua-types/src/generated/types/enums.rs`: `OpenFileMode` and
  (for `OpenWithMasks`) `TrustListMasks` enums already generated with
  their spec-correct bit values.
- `async-opcua-crypto/src/certificate_store.rs`: `trusted_certs_dir()`/
  `issuer_certs_dir()`/`trusted_crls_dir()`/`issuer_crls_dir()` and their
  read-side (`read_trusted_certs`/`read_issuer_certs`/`read_trusted_crls`/
  `read_issuer_crls`) already exist. No write-side helpers
  (`store_trusted_cert`, `remove_trusted_cert`, equivalents for issuer
  certs and CRLs) exist yet -- added this feature, following the existing
  `store_rejected_cert` pattern (`CertificateStore::cert_file_name` for
  naming, filesystem write, no new directory-layout decisions needed).
- `async-opcua-crypto/src/x509.rs`: `X509::thumbprint()` already computes
  the SHA1 thumbprint used to identify certificates -- directly reusable
  for `RemoveCertificate`'s thumbprint matching.
- `async-opcua-crypto`'s existing `CertificateList` (`x509_cert::crl::
  CertificateList`, used by `read_trusted_crls`/`read_issuer_crls`) already
  supports `der::Decode` (read side, existing) and `der::Encode`
  (`.to_der()`, the same pattern already used throughout this crate for
  `X509`) -- no new CRL parsing/encoding logic needed, only file I/O.
- `CertificateStore::validate_application_instance_cert`/
  `validate_or_reject_application_instance_cert` (already used
  server-wide for incoming peer certificates) implements the OPC-10000-4
  validation process this section requires for `CloseAndUpdate`'s
  per-certificate validation -- reused as-is rather than building a
  second validation path.
- `async-opcua-server/src/history/continuation.rs`'s
  `HistoryContinuationPointCache` is a `moka::sync::Cache` with
  `time_to_live` (already a workspace dependency, `moka = { version =
  "0.12", features = ["future", "sync"] }` in `async-opcua-server/
  Cargo.toml`) -- moka's TTL eviction is automatic (no separate timer
  task needed), directly reusable as the pattern for the new TrustList
  file-handle cache (keyed by a `u32` file handle instead of a
  `ByteString` continuation id), satisfying the `ActivityTimeout`
  requirement with no new infrastructure.
- Run 1's `PushTransaction`/`GdsPushRegistry`
  (`async-opcua-server/src/gds/push_methods.rs`) already implements the
  single-transaction-server-wide model, session-ownership checks, and
  `ApplyChanges`/`CancelChanges` dispatch -- extended in place to carry an
  optional pending TrustList change (a new field alongside the existing
  certificate/key fields), rather than building a second transaction
  mechanism.
- No existing generic `FileType` Open/Read/Write/Close *callback*
  dispatcher exists anywhere in this codebase.
  `async-opcua-server/src/fota/file_node.rs` only wires the AddressSpace
  nodes (Object/Variable/Method placeholders) for a separate,
  not-yet-wired FOTA use case -- it registers no callbacks and implements
  no Open/Read/Write semantics, so it is not coupled to or reused here;
  the TrustList file-handle state machine is built fresh in a new
  `gds/trust_list.rs` module.

## Scope decisions (see spec.md Assumptions for full reasoning)

- Scoped to `DefaultApplicationGroup` only; `DefaultHttpsGroup`/
  `DefaultUserTokenGroup` recorded as a follow-up (their nodes exist but
  were not live-verified this pass, and this project's `CertificateStore`
  does not currently model separate per-group trust stores for them).
- `CloseAndUpdate` validation failure discards the whole pending update
  (no partial acceptance), matching the specification's stated minimum
  behavior.
- Re-evaluating already-open Sessions/SecureChannels against a newly
  applied TrustList (closing connections whose certificate is no longer
  trusted) is spec-mandated (§7.8.2.5) but may be deferred if it requires
  session-manager changes beyond this feature's boundaries; documented as
  a simplification if taken, not silently dropped.
