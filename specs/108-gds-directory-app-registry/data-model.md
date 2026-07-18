# Data Model: GDS Directory Application-Registry Services

## Registered Application (extends existing `GdsApplicationRecord`)

The existing `pull_methods/mod.rs::GdsApplicationRecord` gains the real `ApplicationRecordDataType`
fields (research.md R2, R5). Existing fields (`certificate_group_ids`, `application_uri`) are kept
unchanged so the Pull-model's own internal `register_application(application_uri,
default_application_group_id)` convenience path continues to work without modification.

```text
GdsApplicationRecord {
    record_id: u64,                          // NEW -- monotonically increasing, assigned once at
                                              // create/update time (research.md R2, R9)
    certificate_group_ids: Vec<NodeId>,       // unchanged
    application_uri: String,                  // unchanged (also the duplicate-detection key,
                                              // FR-002/RegisterApplication)
    application_type: ApplicationType,        // NEW, default Server (matches generated enum's own
                                              // #[opcua(default)] Server = 0)
    application_names: Vec<LocalizedText>,    // NEW, default empty
    product_uri: String,                      // NEW, default empty
    discovery_urls: Vec<String>,              // NEW, default empty
    server_capabilities: Vec<String>,         // NEW, default empty
}
```

`ApplicationId` (the spec's own field name) is NOT stored inside the record -- it IS the registry
key (`NodeId`) the record is stored under, exactly matching the existing `applications:
moka::sync::Cache<NodeId, GdsApplicationRecord>` shape.

## `ApplicationRecordDataType` (new, hand-authored wire type)

Mirrors the real Part 12 §6.5.5 structure exactly (research.md R2, R8):

```text
ApplicationRecordDataType {
    application_id: NodeId,
    application_uri: UAString,
    application_type: ApplicationType,        // reused generated enum
    application_names: Option<Vec<LocalizedText>>,
    product_uri: UAString,
    discovery_urls: Option<Vec<UAString>>,
    server_capabilities: Option<Vec<UAString>>,
}
```

This is the wire-level ExtensionObject representation used for RegisterApplication/
UpdateApplication's input argument and GetApplication/FindApplications' output argument --
distinct from (and converted to/from) the in-memory `GdsApplicationRecord` above, which uses plain
`NodeId`/`String`/`Vec` rather than the wire types (`UAString`/`Option<Vec<_>>`), matching this
project's existing convention of a thin conversion layer between wire types and internal registry
state (see how `push_methods.rs`/other Pull methods already convert between `Variant`-decoded
arguments and their own internal representations).

## Per-record pagination state (shared by QueryApplications and QueryServers)

- **`record_id: u64`**: assigned once, monotonically increasing, at `RegisterApplication`/
  `UpdateApplication` time (research.md R2). Used as `StartingRecordId`'s exclusive lower bound and
  reported back as `NextRecordId`.
- **`registry_created_at: DateTime`** (new field on the registry itself, not per-record): captured
  once when the registry is constructed; reported verbatim as `LastCounterResetTime` (research.md
  R9) since this registry never resets mid-run (only a full server restart produces a new, later
  value, which is exactly the signal §6.5.10/§6.5.11 describe).

## Result projections (no new types needed)

- **QueryApplications' output**: `ApplicationDescription` (already generated,
  `async-opcua-types/src/generated/types/application_description.rs`). Field mapping from
  `GdsApplicationRecord` per Table 13 (research.md R2): `application_uri`, `application_type`,
  `product_uri`, `discovery_urls` map directly; `application_names` -> single locale-matched
  `application_name`; `gateway_server_uri`/`discovery_profile_uri` set null; `server_capabilities`
  ignored; `application_id`/`record_id` ignored (record_id only used for the outer
  `NextRecordId`/pagination cursor, not per-item).
- **QueryServers' output**: `ServerOnNetwork` (already generated,
  `async-opcua-types/src/generated/types/server_on_network.rs`), ONE row per `discovery_url` in the
  matching record (research.md R2 Table 15) -- `server_name` from locale-matched
  `application_names`, `discovery_url` from the individual URL, `server_capabilities` copied,
  `record_id` set to the record's own `record_id`.
- **FindApplications' output**: `Vec<ApplicationRecordDataType>` (the hand-authored type above),
  size 0 or 1.
- **GetApplication's output**: single `ApplicationRecordDataType`.

No schema/persistence changes; everything above is in-memory, scoped to one running server
instance (spec.md Assumptions).
