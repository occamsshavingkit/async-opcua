# Research: GDS Pull Directory Singleton Correction (Run 1 rework)

## Root cause: how feature 103's research missed the real object

Feature 103's `research.md` stated: "confirmed via exhaustive grep: no UAObject anywhere has a
`HasTypeDefinition` reference pointing at `CertificateDirectoryType`'s NodeId." This claim is false.
The most plausible explanation, given how `HasTypeDefinition` references are encoded in the NodeSet
XML: a `<Reference>` element only ever contains the target's bare NodeId (e.g. `ns=1;i=63`), never
the type's display name. A search for the literal string `CertificateDirectoryType` near a
`HasTypeDefinition` reference — rather than first resolving `CertificateDirectoryType`'s own NodeId
(`ns=1;i=63`) and then searching for `HasTypeDefinition">ns=1;i=63<` — would structurally never find
a match even when the reference exists, because the reference never repeats the type's name. This
session's independent re-verification used the second (correct) approach and found the real object
in seconds. Lesson applied going forward: when grounding "does X reference exist" against a NodeSet
XML, resolve the target's NodeId first, then search for that NodeId's numeric value inside a
`<Reference ReferenceType="...">` element — never search for a type/node's *display name* inside a
`<Reference>` body, since references never encode names.

## Re-verified findings (independently re-checked this session against the local
`schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml`)

**Decision**: Resolve the real, pre-instantiated "Directory" object and its real child methods
instead of constructing a parallel object.

**Evidence** (each grep re-run fresh for this feature, not copied from feature 103's notes):

- `ns=1;i=141` is a real `<UAObject BrowseName="1:Directory">`, with:
  - `HasTypeDefinition -> ns=1;i=63` (confirmed `ns=1;i=63` is `<UAObjectType BrowseName="1:CertificateDirectoryType">`)
  - `Organizes` inverse reference from bare `i=85` — the core namespace's `ObjectsFolder` — meaning
    this object is already organized under the standard Objects folder in the source data, matching
    the convention feature 103 assumed it had to build by hand.
  - Direct `HasComponent` forward references to all of the following (sufcient for
    `InMemoryNodeManager::resolve_method_node_id`'s existing `find_references(object_id,
    HasComponent, Forward)` lookup — no additional reference-wiring is needed):

| Method | Real instance NodeId | `MethodDeclarationId` (type-level, matches feature 103's original Mandatory/Optional findings) |
|---|---|---|
| StartSigningRequest | `ns=1;i=157` | `ns=1;i=79` (Mandatory) |
| StartNewKeyPairRequest | `ns=1;i=154` | `ns=1;i=76` (Mandatory) |
| FinishRequest | `ns=1;i=163` | `ns=1;i=85` (Mandatory) |
| GetCertificateGroups | `ns=1;i=508` | `ns=1;i=369` (Mandatory) |
| GetTrustList | `ns=1;i=204` | `ns=1;i=197` (Mandatory) |
| GetCertificateStatus | `ns=1;i=225` | `ns=1;i=222` (Mandatory) |
| RevokeCertificate | `ns=1;i=15005` | `ns=1;i=15003` (Optional) |
| GetCertificates | `ns=1;i=174` | `ns=1;i=89` (Optional) |
| CheckRevocationStatus | `ns=1;i=177` | `ns=1;i=126` (Optional) |

  Also present on the same real object (`DirectoryType`-inherited, out of scope for this fix):
  `RegisterApplication` (`ns=1;i=146`), `QueryServers` (`ns=1;i=151`), `QueryApplications`
  (`ns=1;i=992`), `FindApplications` (`ns=1;i=143`), `UpdateApplication` (`ns=1;i=200`),
  `UnregisterApplication` (`ns=1;i=149`), `GetApplication` (`ns=1;i=216`), `Applications` folder
  object (`ns=1;i=142`).

- The real `CertificateGroups`/`DefaultApplicationGroup`/`TrustList` subtree is also already real,
  confirmed via the same object's own `References`:
  - `CertificateGroups`: `ns=1;i=614` — `Organizes` child of `ns=1;i=141`, `HasTypeDefinition ->
    i=13813` (core `CertificateGroupFolderType`, bare `i=` confirming namespace 0).
  - `DefaultApplicationGroup`: `ns=1;i=615` — `HasComponent` child of `i=614`, `HasTypeDefinition ->
    i=12555` (core `CertificateGroupType`).
  - `DefaultApplicationGroup.TrustList`: `ns=1;i=616` — `HasComponent` child of `i=615`,
    `HasTypeDefinition -> i=12522` (core `TrustListType`).
  - Siblings `DefaultHttpsGroup` (`ns=1;i=649`) and `DefaultUserTokenGroup` (`ns=1;i=683`) also exist
    under `CertificateGroups` — out of scope for this fix (not requested by CU 3582's Mandatory
    surface), noted for the record only.

**Import mechanics** (confirmed via `AddressSpace::import_node_set`, `async-opcua-server/src/
address_space/mod.rs:72`): the importer performs an unconditional 1:1 transcription of every node
and reference present in a `NodeSetImport`, with no filtering for "types only" — so `import_gds`
already places all of the above real objects/methods/references into the address space today
(re-mapped only in namespace index, per `NodeSetNamespaceMapper`'s existing, previously-verified
identifier-preserving behavior). Nothing about the import path needs to change; only
`directory_instance.rs`'s own logic (which currently ignores these real nodes and builds a parallel
set) needs to change to resolve them instead.

**Alternatives considered**:
- *Keep the hand-built object, deprecate later*: rejected — Constitution Principle II (Do It Right
  Once) and V (Leave It Better Than You Found It) both argue against knowingly shipping a
  non-conformant duplicate object when the fix is a net code deletion, not a net addition.
- *Generalize into a "resolve first, build only if missing" hybrid*: rejected as unnecessary
  complexity — this specific companion NodeSet always ships the real object (it is the GDS
  reference implementation's canonical instance data, matching the documentation's own citation of
  `https://reference.opcfoundation.org/GDS/docs/6.5.2`), so a fail-closed pure-resolution model is
  sufficient and simpler; if a future companion spec genuinely lacks a singleton, that would be a
  new, separately-scoped problem to solve when it's real, not speculatively now.

## Optional-method deferral: corrected reasoning

Feature 103 deferred `RevokeCertificate`/`GetCertificates`/`CheckRevocationStatus` with the reason
"each needs new ledger/CRL-mutation infrastructure this run didn't build" — technically true, but
its documented framing implied the *NodeIds themselves* were also unavailable ("no real object to
hang callbacks off of"). That premise is corrected by this feature: the NodeIds resolve
successfully today. The remaining, accurate reason to keep them deferred is purely business-logic
infrastructure (an issuance ledger to know what to revoke/list, and real CRL mutation) — this
feature updates the documented reasoning accordingly without attempting to build that
infrastructure (unchanged scope decision, corrected justification).
