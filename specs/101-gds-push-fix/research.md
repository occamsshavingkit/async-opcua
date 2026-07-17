# Research: GDS Push Model Fix + Completion (Run 1)

## The real bug found

`async-opcua-server/src/gds/push_methods.rs` hardcoded three NodeId
constants that do not correspond to what their names claim:

| Constant | Claimed | Actual generated NodeId meaning |
|---|---|---|
| `CERTIFICATE_MANAGER_OBJECT_ID = 22388` | a `CertificateManager` object | `ServerConfigurationType..._CertificateExpired_SilenceState_Number` (an Alarms & Conditions property, unrelated) |
| `START_SIGNING_REQUEST_METHOD_ID = 22400` | `StartSigningRequest` method | `..._LatchedState_Name` (another A&C property) |
| `CREATE_SIGNING_REQUEST_METHOD_ID = 22403` | `CreateSigningRequest` method | `..._LatchedState_TransitionTime` (another A&C property) |

Empirically confirmed against a live default-built server: reading
`NodeId(0, 22388)`/`22400`/`22403` all return `BadNodeIdUnknown` — they
aren't even reachable instance nodes in the running address space (likely
unpopulated Optional/placeholder Type-level nodes for a different
subsystem entirely). The GDS Push feature has never worked.

There is also a scope error, independent of the wrong numbers:
`StartSigningRequest` is defined in OPC-10000-12 §7.9.3, under
"Information Model for **Pull** Certificate Management"
(`CertificateDirectoryType`) — it is not a Push-model (`ServerConfigurationType`,
§7.10) method at all, and has no place in `push_methods.rs`.

## Real, verified NodeIds (empirical, via live Read against a default server)

| Method | NodeId | Verified? |
|---|---|---|
| `ServerConfiguration` (object) | `ns=0;i=12637` | Confirmed, BrowseName resolves |
| `CreateSigningRequest` | `ns=0;i=12737` | Confirmed |
| `UpdateCertificate` | `ns=0;i=13737` | Confirmed |
| `ApplyChanges` | `ns=0;i=12740` | Confirmed |
| `CancelChanges` | `ns=0;i=25708` | Confirmed |
| `GetRejectedList` | `ns=0;i=12777` | Confirmed |
| `ResetToServerDefaults` | `ns=0;i=25709` | Confirmed |
| `CreateSelfSignedCertificate` | *(no named constant in generated node_ids.rs)* | Absent from generated nodeset entirely |
| `DeleteCertificate` | *(no named constant)* | Absent |
| `GetCertificates` | `ns=0;i=32333` (named constant exists) | Read returns `BadNodeIdUnknown` — instance not actually populated |

The three absent/unreachable methods are Optional per OPC-10000-12 Table
87; the six confirmed methods cover all four Mandatory methods
(`UpdateCertificate`, `ApplyChanges`, `CreateSigningRequest`,
`GetRejectedList`) plus two of the five Optional ones that DO have real
targets (`CancelChanges`, `ResetToServerDefaults`).

## Spec grounding (OPC-10000-12 §7.10)

- **CreateSigningRequest** (§7.10.10): returns a PKCS#10 DER-encoded
  Certificate Request signed with the server's own private key. Requires
  an encrypted SecureChannel + SecurityAdmin role.
- **UpdateCertificate** (§7.10.5): stages a new certificate (+ optional
  new private key); returns `ApplyChangesRequired`. Requires an
  authenticated SecureChannel + SecurityAdmin role; `Bad_TransactionPending`
  if another session already has a transaction open.
- **ApplyChanges** (§7.10.9): commits the pending change; the caller must
  be the session that opened the transaction; `Bad_NothingToDo` if none is
  open.
- **CancelChanges** (§7.10.11): discards the pending change; same
  session-ownership and `Bad_NothingToDo` rules as ApplyChanges.
- **GetRejectedList** (§7.10.12): returns DER bytes of certificates the
  server has rejected. "This Method is a shortcut for the GetRejectedList
  Method (§7.8.3.2) on the DefaultApplicationGroup CertificateGroup" —
  since that CertificateGroup-level method's node was confirmed absent
  from this server's default address space, `ServerConfiguration.GetRejectedList`
  is implemented directly against `CertificateStore`'s rejected-certs
  directory rather than delegating to a node that doesn't exist.
- **ResetToServerDefaults** (§7.10.13): "the Server shall set the
  ServerState to SHUTDOWN and the shutdownReason to a localized message
  that warns Clients." Implemented via the existing shutdown-scheduling
  mechanism (`ShutdownTarget`/`ServerHandle::shutdown_after_with_return_time`,
  built in feature 097) rather than a new mechanism.

## Existing infrastructure reused

- `opcua_crypto::gds_reload::save_new_credentials(store, cert_der, pkey_pem)`
  and `reload_store_from_disk(store)` already exist (built for the
  Pull-model client flow, `async-opcua-client/src/gds/gds_client.rs`) and
  are directly reusable for `ApplyChanges`'s commit-and-reload step.
- `X509::create_from_pkey` (`async-opcua-crypto/src/x509.rs`) already
  demonstrates the `x509_cert::builder::CertificateBuilder` +
  `pkcs1v15::SigningKey<sha2::Sha256>` pattern for building and signing an
  X.509 structure with this server's own RSA key. The same crate
  (`x509_cert` v0.2.5, already a workspace dependency) also exposes
  `x509_cert::builder::RequestBuilder` for PKCS#10 `CertificationRequest`
  construction — `CreateSigningRequest` mirrors the same signer-construction
  pattern with `RequestBuilder` instead of `CertificateBuilder`.
- `CertificateStore::read_trusted_certs`/`read_issuer_certs` already exist;
  added a matching `read_rejected_certs()` for `GetRejectedList`.
- `PrivateKey::from_pem`/`to_pem` (`async-opcua-crypto/src/aes/rsa_private_key.rs`)
  handle the PEM private-key format for `UpdateCertificate`'s optional
  `PrivateKey`/`PrivateKeyFormat` arguments; other formats (PFX, raw
  PKCS#8 DER) return `Bad_NotSupported`, a result code the spec explicitly
  allows for unsupported formats.

## Scope decisions (see spec.md Assumptions for full reasoning)

- Single-pending-change transaction model (not the full multi-method
  queue) — correct per spec since this run has no TrustList methods to
  queue alongside `UpdateCertificate` yet.
- No automatic transaction cancellation on session disconnect this run —
  documented simplification; `CancelChanges` remains available to clear a
  stale transaction.
- `pull_methods.rs` (CU 2230) has the same fabricated-NodeId disease
  (`GET_REJECTED_LIST_METHOD_ID = 22407`, `UPDATE_CERTIFICATE_METHOD_ID = 22402`
  — neither resolves to a named node either) plus its own mis-categorization
  (it implements `UpdateCertificate`/`GetRejectedList`, which are actually
  Push-model concepts, while never implementing the real Pull-model
  `StartSigningRequest`/`StartNewKeyPairRequest`). Out of scope here;
  recorded in TODO.md as a follow-up.
