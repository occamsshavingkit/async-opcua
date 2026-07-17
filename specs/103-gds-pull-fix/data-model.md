# Data Model: GDS Pull Model Fix (Run 1)

## DirectoryInstanceNodeIds

Resolved once, at `companion-gds` server-startup wiring time, and held by
the registration function for the lifetime of the server.

| Field | Type | Description |
|---|---|---|
| `directory_object_id` | `NodeId` | The instantiated `CertificateDirectoryType` "Directory" object. |
| `start_signing_request_id` | `NodeId` | Instance method NodeId. |
| `start_new_key_pair_request_id` | `NodeId` | Instance method NodeId. |
| `finish_request_id` | `NodeId` | Instance method NodeId. |
| `get_certificate_groups_id` | `NodeId` | Instance method NodeId. |
| `get_trust_list_id` | `NodeId` | Instance method NodeId. |
| `get_certificate_status_id` | `NodeId` | Instance method NodeId. |
| `default_application_group_id` | `NodeId` | The `DefaultApplicationGroup` object instantiated under `CertificateGroups`. |
| `default_application_group_trust_list_id` | `NodeId` | The TrustList object under `DefaultApplicationGroup` -- what `GetTrustList` returns. |

Built by `gds/directory_instance.rs`: resolve the GDS namespace index (via
`AddressSpace::namespaces()` reverse lookup against
`"http://opcfoundation.org/UA/GDS/"`), confirm `CertificateDirectoryType`
exists at `NodeId::new(gds_ns, 63)` (fail closed -- log and skip wiring
if the companion XML wasn't actually present/importable, matching
`import_companion_xml`'s existing "warn and return" behavior for a missing
file), then construct fresh instance nodes (new namespace-scoped string
identifiers, e.g. `NodeId::new(gds_ns, "Directory")`,
`NodeId::new(gds_ns, "Directory.StartSigningRequest")`, ...) with
`HasTypeDefinition` back to the corresponding imported type/method nodes,
`InputArguments`/`OutputArguments` properties authored directly from the
spec's signatures (§7.9.3-§7.9.10), and `Organizes`/`HasComponent`
references matching Table 74's shape.

## GdsApplicationRegistry

In-memory, bounded (reusing the existing `push_bounded_fifo`/
`GDS_REGISTRY_CAPACITY` pattern from `pull_methods.rs`).

| Field | Type | Description |
|---|---|---|
| `application_id` | `NodeId` | Key. Freshly allocated when an application is registered (this feature's minimal registration helper, not the full `RegisterApplication` Method -- see research.md). |
| `certificate_group_ids` | `Vec<NodeId>` | Certificate groups this application belongs to. For this run, always exactly `[default_application_group_id]` (no additional groups modeled). |
| `application_uri` | `String` | Used to validate `StartSigningRequest`'s CSR's `ApplicationUri` matches (`Bad_CertificateUriInvalid` per §7.9.3 otherwise). |

## GdsPullRequest

| Field | Type | Description |
|---|---|---|
| `request_id` | `NodeId` | Key. Freshly generated per `StartSigningRequest`/`StartNewKeyPairRequest` call. |
| `application_id` | `NodeId` | Owning application; `FinishRequest` must be called with the same `ApplicationId`. |
| `state` | `PullRequestState` | `Pending` or `Completed { certificate_der, private_key, issuer_certificates }`. |

```
enum PullRequestState {
    Pending,
    Completed {
        certificate_der: Vec<u8>,
        private_key: Option<Vec<u8>>,   // Some only for StartNewKeyPairRequest
        issuer_certificates: Vec<Vec<u8>>,
    },
}
```

## State transitions

```
StartSigningRequest / StartNewKeyPairRequest:
    (no request) --create--> Pending
                  --sign immediately (auto-approve, see research.md)--> Completed

FinishRequest(application_id, request_id):
    Pending    --> Bad_NothingToDo
    Completed  --> (Certificate, PrivateKey, IssuerCertificates), request removed from registry
    not found  --> Bad_InvalidArgument
    wrong application_id --> Bad_InvalidArgument
```

Because `Start*` resolves synchronously in this run (no separate
human-approval queue -- see research.md Assumptions), the `Pending` state
is real and part of the protocol contract, but only reachable in tests via
directly constructing a `GdsPullRequest` in that state (the same technique
already used for `PushTransaction` in `push_methods.rs`'s tests) rather
than through the normal `Start*` → `FinishRequest` happy path.

## Validation rules

- `StartSigningRequest`/`StartNewKeyPairRequest`/`GetCertificateGroups`/
  `GetTrustList`/`GetCertificateStatus`: `Bad_NotFound` if `ApplicationId`
  isn't in `GdsApplicationRegistry`.
- `StartSigningRequest`: the supplied CSR's `ApplicationUri` must match the
  application's registered `application_uri` (`Bad_CertificateUriInvalid`
  otherwise, per §7.9.3).
- `FinishRequest`: `Bad_InvalidArgument` if `RequestId` isn't found or
  doesn't belong to the calling `ApplicationId`; `Bad_NothingToDo` if
  `Pending`.
- All methods: `Bad_SecurityModeInsufficient`/`Bad_UserAccessDenied` per
  each method's spec-mandated channel/role requirement (see research.md).
