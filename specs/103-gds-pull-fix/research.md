# Research: GDS Pull Model Fix (Run 1)

> **Correction (feature 104, 2026-07-17)**: The "CertificateDirectoryType has no pre-built
> instance" section below is **wrong**. The real GDS companion NodeSet2.xml ships a fully
> pre-instantiated "Directory" object (source `ns=1;i=141`, `HasTypeDefinition -> ns=1;i=63`)
> with real instance methods at every Mandatory/Optional NodeId this section lists as
> "template-only". This feature's own "exhaustive grep" evidently searched for the literal
> string `CertificateDirectoryType` inside `<Reference>` elements — but a `<Reference>` only
> ever encodes a target's bare NodeId (`ns=1;i=63`), never the type's display name, so that
> search could never match even though the reference exists. See
> `specs/104-gds-pull-directory-fix/research.md` for the corrected, independently re-verified
> findings and the fix that replaced the hand-built object this wrong conclusion led to.

## The real bug found (verified, not assumed)

`gds/pull_methods.rs` defines `GET_REJECTED_LIST_METHOD_ID = 22407` and
`UPDATE_CERTIFICATE_METHOD_ID = 22402` and implements methods named
`GetRejectedList`/`UpdateCertificate` -- but these are Push-model
(`ServerConfigurationType`, Part 12 §7.10) concepts, not Pull-model ones.
Neither NodeId resolves to anything: `grep` against
`async-opcua-types/src/generated/node_ids.rs` for `22407`/`22402` (as
constant values) returns no match. This mirrors the exact fabricated-constant
defect Run 1 (feature 101) found and fixed in `push_methods.rs`.

The real Pull-model type, `CertificateDirectoryType` (Part 12 §7.9.2, "the
TypeDefinition for the root of the CertificateManager AddressSpace"), does
not exist anywhere in this project's generated core nodeset (zero matches
for `CertificateDirectoryType` or any of its method names in
`node_ids.rs`). It is namespace-2 in the spec text
(`2:CertificateDirectoryType`) -- part of the GDS companion specification,
not the core namespace-0 types this project's code generator consumes.

## The companion-gds subsystem is entirely dormant

`async-opcua-server/src/companion/mod.rs` already defines `import_gds`
(via the `companion!("companion-gds", import_gds, "GDS/Opc.Ua.Gds.NodeSet2.xml")`
macro invocation) and ~60 sibling `import_<spec>` functions for other
companion specs. But:

- `mod companion;` is declared *private* in `async-opcua-server/src/lib.rs`
  -- not reachable from outside the crate.
- A repo-wide grep for `import_gds`, `import_adi`, `import_di`,
  `import_all_companions`, or `companion::` finds zero callers anywhere
  outside `companion/mod.rs` itself. No server, sample, or test imports any
  companion spec today.

This matches the class of bug Run 1 found in `push_methods.rs` (registered
callbacks that nothing ever wires up) but at the scale of an entire
subsystem rather than one file.

## CertificateDirectoryType has no pre-built instance (unlike ServerConfigurationType)

Inspecting the actual GDS companion NodeSet2.xml (fetched locally to
`/tmp/UA-Nodeset/GDS/Opc.Ua.Gds.NodeSet2.xml` for this research; the
project's own copy must be cloned by the operator into
`schemas/companion/GDS/` per `schemas/companion/README.md` -- never
committed to the repo):

- `CertificateDirectoryType` (`ns=1;i=63`) is declared as a `UAObjectType`
  with its Mandatory/Optional methods as `HasComponent` references *on the
  type itself*, each carrying `HasModellingRule` (Mandatory/Optional) --
  this is the standard OPC UA pattern for defining an ObjectType's
  instantiation template, not a live instance.
- Exhaustive grep of the file confirms no `UAObject` anywhere has a
  `HasTypeDefinition` reference pointing at `ns=1;i=63` -- there is no
  pre-instantiated "Directory" singleton in the companion XML at all. This
  differs fundamentally from Run 1/2's `ServerConfigurationType`, which
  ships as a ready-instantiated singleton in the *core* nodeset.
- Confirmed via a dedicated research pass: no code anywhere in
  `async-opcua-nodes`/`async-opcua-server` walks `HasModellingRule`
  references to materialize a live instance from an ObjectType. This
  capability does not exist and must be built (scoped narrowly to what
  `CertificateDirectoryType` needs -- see Assumptions).

### CertificateDirectoryType's real (verified) method NodeIds, source XML

| Method | Source identifier (`ns=1;i=N`) | Modelling Rule |
|---|---|---|
| `StartSigningRequest` | 79 | Mandatory |
| `StartNewKeyPairRequest` | 76 | Mandatory |
| `FinishRequest` | 85 | Mandatory |
| `GetCertificateGroups` | 369 | Mandatory |
| `GetTrustList` | 197 | Mandatory |
| `GetCertificateStatus` | 222 | Mandatory |
| `RevokeCertificate` | 15003 | Optional |
| `GetCertificates` | 89 | Optional |
| `CheckRevocationStatus` | 126 | Optional |

`CertificateDirectoryType`'s own identifier is `ns=1;i=63`; its
`CertificateGroups`/`DefaultApplicationGroup`/`TrustList` Organizes subtree
is `ns=1;i=511`/`512`/`513` respectively (all Mandatory template children).

## Namespace-index remapping preserves numeric identifiers

`NodeSetNamespaceMapper` (`async-opcua-types/src/namespaces.rs`) is the
mechanism `NodeSet2Import` uses during load: `add_namespace(namespace,
index_in_node_set)` records a mapping from the *source file's* local
namespace index to the index this project's `AddressSpace` actually
assigns it (via `NamespaceMap::add_namespace`, which may reuse an existing
index or allocate a new one). Critically, this mapper only ever remaps the
**namespace index** portion of a NodeId -- the numeric (or string)
**identifier** is carried through unchanged. This means, after import,
`CertificateDirectoryType` is reachable at
`NodeId::new(<resolved_gds_ns_index>, 63u32)`, and its methods at the same
resolved index with their source identifiers (79, 76, 85, 369, 197, 222,
...) -- deterministic and computable, not something that needs a
browse-based search.

`import_companion_xml` (`companion/mod.rs`) currently constructs its own
local `NamespaceMap` and discards it after the call -- there is no public
way today to learn the resolved index from the existing function signature.
This feature reads `AddressSpace::namespaces()` (`HashMap<u16, String>`,
populated during import) after calling `import_gds`, and reverse-looks-up
the index for the GDS namespace URI (confirmed from the XML's own
`<NamespaceUris><Uri>http://opcfoundation.org/UA/GDS/</Uri></NamespaceUris>`
declaration) rather than modifying the shared companion-import machinery
used by all ~60 other companion specs.

## Existing infrastructure reused

- `SimpleNodeManager` (`InMemoryNodeManager<SimpleNodeManagerImpl>`)
  exposes `address_space() -> &Arc<RwLock<AddressSpace>>` -- the same
  `&RwLock<AddressSpace>` type `import_companion_xml` expects, and the
  same address space `ObjectBuilder`/`MethodBuilder`/`VariableBuilder`
  target via `.insert(&address_space)`. Both the companion import and the
  new Directory-instance construction target this one shared address
  space, so `SimpleNodeManager::add_method_callback_with_context` can then
  dispatch Call requests against the newly-created instance nodes.
- `ObjectBuilder`/`MethodBuilder`/`VariableBuilder` (`async-opcua-nodes`,
  the `node_builder_impl!` macro family) already support building nodes at
  an arbitrary namespace index with either numeric or string identifiers,
  as proven working in `fota/file_node.rs` (`ObjectBuilder::new(...)
  .has_type_definition(...).component_of(...).insert(...)`,
  `MethodBuilder::new(...).input_args(...).output_args(...)`).
- `X509::create_signing_request` (Run 1, `async-opcua-crypto/src/x509.rs`)
  and `X509::cert_and_pkey` (existing) cover the CSR-signing and
  new-key-pair-generation primitives `StartSigningRequest`/
  `StartNewKeyPairRequest` need; no new cryptographic code is required.
- `push_methods.rs`'s `PushTransaction`/bounded-FIFO registry pattern
  (Run 1) and `GdsPullMethodRegistry`'s existing `push_bounded_fifo` helper
  are the direct precedent for this feature's application registry and
  pending-request registry (bounded, in-memory, no new capacity-limiting
  design needed).

## Spec grounding (OPC-10000-12 §7.9, exact signatures)

- **StartSigningRequest** (§7.9.3): `(ApplicationId, CertificateGroupId,
  CertificateTypeId, CertificateRequest) -> RequestId`. Caller supplies a
  DER PKCS#10 CSR; `Bad_CertificateUriInvalid` if the CSR's ApplicationUri
  doesn't match the registered application. Encrypted channel +
  `CertificateAuthorityAdmin`/`ApplicationAdmin`/`ApplicationSelfAdmin`.
- **StartNewKeyPairRequest** (§7.9.4): `(ApplicationId, CertificateGroupId,
  CertificateTypeId, SubjectName, DomainNames, PrivateKeyFormat,
  PrivateKeyPassword) -> RequestId`. Same auth as above.
- **FinishRequest** (§7.9.5): `(ApplicationId, RequestId) -> (Certificate,
  PrivateKey, IssuerCertificates)`. `Bad_NothingToDo` "There is nothing to
  do because request has not yet completed" -- the pending/completed
  distinction this feature's request registry must model.
  `Bad_InvalidArgument` for an unrecognized RequestId. Same auth.
- **GetCertificateGroups** (§7.9.7): `(ApplicationId) -> CertificateGroupIds[]`.
  Authenticated channel + same roles (not necessarily encrypted).
- **GetTrustList** (§7.9.9): `(ApplicationId, CertificateGroupId) ->
  TrustListId`. Same auth.
- **GetCertificateStatus** (§7.9.10): `(ApplicationId, CertificateGroupId,
  CertificateTypeId) -> UpdateRequired: Boolean`. Same auth.
- `RevokeCertificate`/`GetCertificates`/`CheckRevocationStatus` are
  Optional (Table 74); implemented if unambiguous without new
  infrastructure (see data-model.md), otherwise deferred and documented,
  matching Run 1's precedent for `CreateSelfSignedCertificate`/
  `DeleteCertificate`/`GetCertificates`.

## Scope decisions (see spec.md Assumptions for full reasoning)

- **Application registration**: every Pull-model method takes an
  `ApplicationId` and returns `Bad_NotFound` if it "does not refer to a
  registered application." Full `RegisterApplication`/Application Directory
  support is a separate, larger, already-tracked TODO item (CUs 2232/2233
  etc.) and out of scope here. This feature builds a minimal, self-contained
  in-memory application registry (just enough for the Bad_NotFound path and
  CertificateGroup assignment to be meaningfully correct and testable) --
  not the full Directory.RegisterApplication Method.
- **Request approval model**: the spec describes an async workflow where a
  human/tool with the `RegistrationAuthorityAdmin` Role approves a pending
  request before `FinishRequest` returns real material. No such approval-
  queue product exists in this SDK and building one is out of scope. This
  feature models the Pending/Completed distinction faithfully in its data
  model (so `Bad_NothingToDo` is a real, testable state, not hardcoded
  away), but resolves requests synchronously within the `Start*` handler
  itself (auto-approve) since there is no separate approval workflow to
  wait on. Documented as a deliberate simplification.
- **Instantiation scope**: the object-instantiation logic is written for
  exactly what `CertificateDirectoryType` needs (its six Mandatory methods
  plus the `CertificateGroups`/`DefaultApplicationGroup`/`TrustList`
  subtree), not a fully generic "instantiate any ObjectType from its
  Mandatory modelling rules" engine -- that would be substantially larger
  scope and isn't needed by any other feature today.
- **Client-side sibling defect**: `async-opcua-client/src/gds/gds_client.rs`,
  `csr.rs`, `registration.rs` were found during this investigation to
  hardcode the *exact same fabricated NodeIds* Run 1 fixed on the server
  side (`certificate_manager_id=22388`, `start_signing_request_id=22400`,
  `finish_signing_request_id=22402`, plus `directory_object_id=22384`,
  `register_method_id=22385`, untouched by Run 1). Fixing these correctly
  requires dynamic NodeId discovery (Browse/TranslateBrowsePath against
  whatever external, real GDS product the client connects to -- every real
  deployment assigns its own namespace index), not hardcoded constants.
  Out of scope for this run; recorded in TODO.md as Run 2.
