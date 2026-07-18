# Phase 0 Research: GDS Directory Application-Registry Services

## R1: Real NodeIds re-verified directly against the XML (not trusted from any summary)

Confirmed via direct grep against `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` (all children of
the real Directory object at `ns=1;i=141`, matching `directory_instance.rs`'s existing 9 fields'
provenance exactly):

| Method/Node | Real NodeId | MethodDeclarationId |
|---|---|---|
| RegisterApplication | `ns=1;i=146` | `ns=1;i=18` |
| QueryServers | `ns=1;i=151` | `ns=1;i=23` |
| QueryApplications | `ns=1;i=992` | `ns=1;i=868` |
| FindApplications | `ns=1;i=143` | `ns=1;i=15` |
| UpdateApplication | `ns=1;i=200` | `ns=1;i=188` |
| UnregisterApplication | `ns=1;i=149` | `ns=1;i=21` |
| GetApplication | `ns=1;i=216` | `ns=1;i=210` |
| Applications folder | `ns=1;i=142` | -- |

`RegisterApplication`/`UpdateApplication`/`UnregisterApplication` (the three write methods) each
carry `AccessRestrictions="1"` in the XML -- an address-space-level attribute (Part 3
`AccessRestrictionsType`), independent of this project's own RBAC role check; no special handling
needed beyond what the existing method-dispatch/session security-mode enforcement already does.

## R2: Real spec text (Part 12 v1.05.07, re-verified via `pdftotext -layout`)

- **RegisterApplication (§6.5.6)**: `RegisterApplication([in] ApplicationRecordDataType
  Application, [out] NodeId ApplicationId)`. "Shall be called from an authenticated SecureChannel
  and from a Client that has access to the DiscoveryAdmin Role or the ApplicationAdmin Privilege."
  "Shall not create duplicate records. If the ApplicationUri already exists the Method returns
  Bad_EntryExists." On success, added to the QueryApplications/FindApplications result set. Errors:
  `Bad_InvalidArgument`, `Bad_EntryExists`, `Bad_UserAccessDenied`, `Bad_SecurityModeInsufficient`.
- **UpdateApplication (§6.5.7)**: `UpdateApplication([in] ApplicationRecordDataType Application)`.
  Requires DiscoveryAdmin Role, ApplicationSelfAdmin Privilege, or ApplicationAdmin Privilege.
  "When updating an existing application the ApplicationUri cannot be changed. If it is changed
  the Method returns Bad_WriteNotSupported." Errors: `Bad_NotFound`, `Bad_InvalidArgument`,
  `Bad_WriteNotSupported`, `Bad_UserAccessDenied`, `Bad_SecurityModeInsufficient`.
- **UnregisterApplication (§6.5.8)**: `UnregisterApplication([in] NodeId ApplicationId)`. Same
  three-way permission requirement as UpdateApplication. "If an application has Certificates issued
  by the CertificateManager, these Certificates shall be revoked when this Method is called." (see
  R6 -- deferred). Errors: `Bad_NotFound`, `Bad_UserAccessDenied`, `Bad_SecurityModeInsufficient`.
- **GetApplication (§6.5.9)**: `GetApplication([in] NodeId ApplicationId, [out]
  ApplicationRecordDataType Application)`. No explicit role requirement stated in prose (unlike the
  three write methods); result codes include `Bad_UserAccessDenied` ("does not have the rights
  required to read the requested record") but the text never names a specific Role for read access
  -- treated as an open/authenticated-only read, matching FindApplications's explicit "can be
  called by any Client" wording (R4).
- **FindApplications (§6.5.4)**: `FindApplications([in] String ApplicationUri, [out]
  ApplicationRecordDataType[] Applications)`. "Can be called by any Client." Returned array is size
  0 or 1 (found or not). Errors: `Bad_InvalidArgument` (URI too long/invalid).
- **QueryApplications (§6.5.10)**: `QueryApplications([in] UInt32 StartingRecordId, [in] UInt32
  MaxRecordsToReturn, [in] String ApplicationName, [in] String ApplicationUri, [in] UInt32
  ApplicationType, [in] String ProductUri, [in] String[] Capabilities, [out] UtcTime
  LastCounterResetTime, [out] UInt32 NextRecordId, [out] ApplicationDescription[] Applications)`.
  "Any Client is able to call this Method." Filters combine with AND; `Capabilities` requires
  supporting ALL listed values; `ApplicationType` is a bitmask (`0x1`=Servers, `0x2`=Clients, `0`=
  all); string filters (`ApplicationName`/`ApplicationUri`/`ProductUri`) use LIKE-operator syntax
  (see R3), empty string = not applied. Never returns records whose `ServerCapabilities` includes
  `NA`. Each record gets a monotonically increasing identifier assigned at create/update time;
  `StartingRecordId` is an exclusive lower bound, `NextRecordId` is the resume point (0 = no more),
  `LastCounterResetTime` signals a counter reset (e.g. restart) the client must react to by
  restarting from `StartingRecordId=0`. Table 13 defines the exact `ApplicationRecordDataType` ->
  `ApplicationDescription` field mapping: `ApplicationId` ignored; `ApplicationUri`,
  `ApplicationType`, `ProductUri`, `discoveryUrls` map directly; `ApplicationNames` -> single
  `ApplicationName` (locale-matched, first element if no session); `gatewayServerUri`/
  `discoveryProfileUri` set NULL; `ServerCapabilities` ignored.
- **QueryServers (§6.5.11, "(deprecated)")**: `QueryServers([in] UInt32 StartingRecordId, [in]
  UInt32 MaxRecordsToReturn, [in] String ApplicationName, [in] String ApplicationUri, [in] String
  ProductUri, [in] String[] ServerCapabilities, [out] UtcTime LastCounterResetTime, [out]
  ServerOnNetwork[] Servers)`. "Does not require any permissions to call." Same
  filter/pagination/counter-reset semantics as QueryApplications, but NO `ApplicationType` filter
  (Servers implied), and Table 15's mapping is per-`discoveryUrl`, not per-application: **"A
  ServerOnNetwork record is returned for each discoveryUrl in the ApplicationRecord"** -- one
  application with 3 discovery URLs yields 3 `ServerOnNetwork` rows. `recordId` in the output is
  literally the SAME per-record identifier QueryApplications also uses (both methods reference "the
  monotonically increasing identifier" assigned once per record, not two separate counters) --
  confirms QueryServers and QueryApplications share one underlying registry/counter, projected two
  different ways; not an "ill-fitting adapter," a spec-correct shared-counter design.
- **ApplicationRecordDataType (§6.5.5)**: `{ApplicationId: NodeId, ApplicationUri: String,
  ApplicationType: ApplicationType, ApplicationNames: LocalizedText[], ProductUri: String,
  DiscoveryUrls: String[], ServerCapabilities: String[]}`, subtype of `Structure`.

## R3: The LIKE-operator filter needs a small, new, self-contained matcher -- nothing to reuse

Grepped for an existing `FilterOperator::Like` evaluator anywhere in `async-opcua-server`: none
exists (`operand.rs`'s `like()` is a client-side filter-*builder* helper, not a server-side
pattern *matcher*). Part 4 v1.05.07 §Table 120 defines the exact grammar (case-sensitive):
`%` = any string of zero-or-more chars; `_` = any single char; `\` = escape (`\\`, `\%`, `\_`);
`[...]` = match any single char in a list/range (e.g. `[13-68]`, `[c-f]`); `[^...]` = negated char
set. This feature adds one small, dedicated, unit-tested matcher function implementing exactly
this grammar (used by `ApplicationName`/`ApplicationUri`/`ProductUri` filtering in
QueryApplications/QueryServers) -- bounded, new, self-contained; not a reuse of anything existing,
and not a dependency any other feature needs.

## R4: RBAC -- matches this module's own established simplification

`pull_methods/mod.rs`'s own doc comment already documents the precedent: this project's
`WellKnownRole` only models the 8 standard Part 3 roles, not GDS's own `DiscoveryAdmin`/
`ApplicationAdmin`/`ApplicationSelfAdmin`. Following the SAME already-established simplification
(also used by `push_methods.rs`): `RegisterApplication`/`UpdateApplication`/`UnregisterApplication`
require `WellKnownRole::SecurityAdmin` uniformly. `GetApplication`/`FindApplications`/
`QueryApplications`/`QueryServers` (read-only) require no special role -- consistent with their own
spec text ("can be called by any Client" / "does not require any permissions to call" /
`GetApplication`'s prose naming no specific Role).

## R5: Extend `GdsApplicationRecord`, don't build a second registry

`pull_methods/mod.rs`'s existing `GdsApplicationRecord` (`certificate_group_ids`,
`application_uri`) is deliberately minimal, used only internally to auto-register an application
as a side effect of the Pull-model's own `StartSigningRequest`/`StartNewKeyPairRequest` flow (a
client that hasn't gone through a "real" `RegisterApplication` call first still needs *something*
to hang a certificate group off of). This is conceptually distinct from, but must ultimately back
the same underlying storage as, the real Part 12 registry -- per RegisterApplication's own spec
text, an application it registers becomes visible to QueryApplications/FindApplications, and
Pull-model-registered applications should equally be visible/queryable through this feature's new
methods (there's no basis in the spec for two disjoint registries). Extend
`GdsApplicationRecord` with the additional real fields (`application_type`, `application_names`,
`product_uri`, `discovery_urls`, `server_capabilities`, plus a monotonically increasing
`record_id: u64` per R2), defaulting the new fields sensibly when the existing internal
`register_application(application_uri, default_application_group_id)` convenience constructor is
used (unchanged call sites, unchanged behavior for the Pull-model's own use), and add a fuller
constructor for the new real `RegisterApplication` method.

## R6: UnregisterApplication's certificate-revocation requirement -- deferred, matching CU 3582's own precedent

Part 12 §6.5.8: "If an application has Certificates issued by the CertificateManager, these
Certificates shall be revoked when this Method is called." This project's own TODO.md already
documents CU 3582's `RevokeCertificate` as unimplemented, needing "an issuance ledger... real CRL
mutation... none of which this SDK builds yet." `UnregisterApplication`'s revocation requirement
depends on that exact same not-yet-built infrastructure. Rather than half-implementing revocation
here (or silently ignoring the requirement), this feature's `UnregisterApplication` removes the
application record only, and this gap is documented explicitly in TODO.md alongside CU 3582's
existing entry -- consistent handling of the same underlying missing infrastructure, not a new,
separately-tracked gap.

## R7: Audit event emission -- deferred, matching this module's own existing (lack of) precedent

Part 12 says "if auditing is supported, the GDS shall generate the ApplicationRegistrationChanged
AuditEventType" for Register/Update/Unregister -- explicitly conditional wording, not an
unconditional MUST. Grepped `gds/*.rs` broadly: **no GDS method (Push or Pull) currently emits any
audit event**, despite several being conceptually audit-worthy (certificate issuance, trust
changes) already. Adding audit emission to just this feature's three write methods, while every
sibling GDS write method still emits none, would be a one-off inconsistency, not an improvement --
deferred and noted in TODO.md as a broader "GDS audit event coverage" gap, not specific to this
feature.

## R8: `ApplicationRecordDataType` wire encoding -- a real gap in the vendored NodeSet, and the resolution

`ApplicationDescription` (QueryApplications' output) and `ServerOnNetwork` (QueryServers' output)
are both already-generated core types (`async-opcua-types/src/generated/types/
application_description.rs`, `server_on_network.rs`) -- reused directly, no new type needed for
either. `ApplicationRecordDataType` (RegisterApplication/UpdateApplication/GetApplication/
FindApplications' own argument type) is genuinely new and NOT code-generated.

Investigated the mechanics in depth (not assumed): a hand-written type used as a Method
`ExtensionObject` argument does NOT need the heavier `DynamicStructure`/`DataTypeTree` runtime
machinery (`async-opcua-types/src/custom/custom_struct.rs` -- powerful, but zero existing
precedent anywhere in `async-opcua-server`/`async-opcua-client`, and would be a materially larger,
riskier lift for this feature). Instead, `DynEncodable` (needed so `ExtensionObject::new`/
`into_inner_as`/wire (de)serialization work) is a **blanket impl** for any `T: BinaryEncodable +
BinaryDecodable + ExpandedMessageInfo + Debug + Clone + PartialEq + Send + Sync + Any` (+
`JsonEncodable`/`XmlEncodable` when those features are on) -- confirmed at
`async-opcua-types/src/extension_object.rs`'s `blanket_dyn_encodable!` macro. `BinaryEncodable`/
`BinaryDecodable` (and `JsonEncodable`/`JsonDecodable`/`XmlEncodable`/`XmlDecodable`/`XmlType`) are
all `#[proc_macro_derive(...)]`-able via `async-opcua-macros` -- the SAME derives every generated
type uses. `samples/custom-codegen/src/generated/types/mod.rs` is a complete, working, in-repo
blueprint for exactly this "hand-authored custom type for a non-core namespace + its own
`TypeLoader`" pattern (a `TypeLoaderInstance` + `add_binary_type`/`add_json_type`/`add_xml_type`
keyed by `(DataTypeId as u32, EncodingId as u32)`, wrapped in a small `impl TypeLoader` checking the
node's namespace URI first) registered via `ServerBuilder::with_type_loader(Arc::new(...))` (and
the equivalent `ClientBuilder::with_type_loader`, needed since the *client* must encode
`ApplicationRecordDataType` for Register/Update and decode it for Get/FindApplications' output --
verify this exists on `ClientBuilder` during implementation, don't assume symmetry).

**The real gap**: that blueprint's `add_binary_type` calls need a `DataTypeId` AND a distinct
`Encoding_DefaultBinary` `ObjectId` -- i.e. TWO NodeIds per type, the DataType's own and a separate
encoding-object NodeId, linked in a real NodeSet via a `HasEncoding` reference from an explicit
"Default Binary" Object node. **The vendored `Opc.Ua.Gds.NodeSet2.xml` does not define any such
encoding object for `ApplicationRecordDataType`** (confirmed: grepping the full node definition at
`ns=1;i=1` shows only a `HasSubtype` reference and its `Definition`/`Field` list -- no `HasEncoding`
reference, no companion `UAObject ... Encoding_DefaultBinary` node anywhere referencing it). This
is a genuine limitation of the locally-vendored companion spec export, not a decoding mistake on
this project's part.

**Resolution**: since this feature is both the encoder (client, for Register/Update) and decoder
(server, for the same; and the reverse for Get/FindApplications' output) of this exact type within
this same codebase, use the DataType's own real NodeId (`ns=1;i=1`) directly as its own binary/
JSON/XML encoding identifier too -- a documented, self-consistent convention scoped to this
project's own client<->server round-trips. **Limitation, stated plainly**: a genuinely independent
third-party GDS client or server expecting a *different* (more conventional, separately-numbered)
encoding NodeId for this type would not interoperate with this choice out of the box; this is
recorded as a known limitation in TODO.md, not silently glossed over, and is a direct consequence
of the vendored NodeSet's own missing metadata rather than an implementation shortcut.

## R9: Monotonic record-identifier + `LastCounterResetTime`

Both QueryApplications and QueryServers need a per-record, monotonically increasing identifier
assigned once at create/update time (R2), plus a `LastCounterResetTime` the client compares against
its own last-seen value to detect a required restart-from-zero. Since the registry is in-memory
and not persisted across restarts (spec.md's own documented Assumption), `LastCounterResetTime`
is simply the registry's own construction time (equivalently, server start time) -- there is
nothing to "reset" mid-run in this design (no persistence to lose), so a single fixed timestamp
captured once at registry creation satisfies the spec's intent (a restart always produces a fresh
registry, and thus a fresh, later `LastCounterResetTime`, which is exactly the signal a real client
needs to detect it must restart its own pagination from `StartingRecordId=0`).

## R10: Existing GDS integration-test harness to mirror

`gds_pull_companion_integration.rs` (feature 103/104) is the established real client-vs-server
Call-service dispatch harness for this exact companion-namespace/Directory-object setup. This
feature's own end-to-end test extends that same harness (same server/companion-import bootstrap),
not a new one from scratch.

## R11: Client-side type-loader registration confirmed

`Session::add_type_loader(&self, type_loader: Arc<dyn TypeLoader>)` (`async-opcua-client/src/
session/mod.rs:453`) is the client-side equivalent of `ServerBuilder::with_type_loader` -- called
on a connected `Session`, not the builder. The end-to-end test (T0xx) registers the same custom
`GdsApplicationRecordTypeLoader` on both the server (`ServerBuilder::with_type_loader`, at build
time) and the client (`session.add_type_loader(...)`, after connecting), matching
`samples/custom-codegen`'s pattern on both sides.
