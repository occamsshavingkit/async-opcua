# Quickstart: GDS Directory Application-Registry Services

## Before

```text
Call(RegisterApplication, ...) -> Bad_ServiceUnsupported   // no callback registered at all
Call(QueryApplications, ...)   -> Bad_ServiceUnsupported
```

## After

An operator registers a new client application, finds it, updates it, and removes it -- all
through the real DirectoryType methods on the same real "Directory" object features 103/104
already resolve:

```text
Call(RegisterApplication, Application={
    ApplicationUri: "urn:example:my-client",
    ApplicationType: Client,
    ApplicationNames: ["My Client"],
    ProductUri: "urn:example:products:my-client",
    DiscoveryUrls: [],
    ServerCapabilities: [],
})
-> ApplicationId = ns=2;s=Application.7          // newly assigned

// Duplicate registration is rejected, not silently duplicated:
Call(RegisterApplication, Application={ApplicationUri: "urn:example:my-client", ...})
-> Bad_EntryExists

Call(QueryApplications, ApplicationUri="urn:example:my-*", StartingRecordId=0, MaxRecordsToReturn=0)
-> Applications: [ApplicationDescription{ApplicationUri: "urn:example:my-client", ...}]

Call(GetApplication, ApplicationId=ns=2;s=Application.7)
-> Application: ApplicationRecordDataType{ApplicationUri: "urn:example:my-client", ...}

Call(UpdateApplication, Application={ApplicationId: ns=2;s=Application.7, ApplicationUri: "urn:example:my-client", ApplicationNames: ["My Client v2"], ...})
-> Good

Call(UnregisterApplication, ApplicationId=ns=2;s=Application.7)
-> Good

Call(QueryApplications, ApplicationUri="urn:example:my-*", ...)
-> Applications: []   // no longer found
```

A caller without `SecurityAdmin` gets rejected for the write methods, but read-only lookups
(FindApplications/GetApplication/QueryApplications/QueryServers) remain open to any authenticated
client, matching the real spec text's own "can be called by any Client" wording:

```text
Call(RegisterApplication, ...)   [non-admin session] -> Bad_UserAccessDenied
Call(QueryApplications, ...)     [non-admin session] -> Good (results, possibly restricted)
```

The deprecated `QueryServers` still works, drawing from the same registry, fanned out one row per
discovery URL:

```text
Call(QueryServers, ApplicationUri="urn:example:my-*", ...)
-> Servers: [ServerOnNetwork{ServerName: "My Client v2", DiscoveryUrl: <one per URL>, ...}]
```

## Unchanged

- The existing CertificateDirectoryType Pull-model methods (StartSigningRequest/
  StartNewKeyPairRequest/FinishRequest/GetCertificateGroups/GetTrustList/GetCertificateStatus,
  features 103/104) behave identically.
- The Pull-model's own internal `register_application` convenience path (used when a client starts
  a certificate request without an explicit prior `RegisterApplication` call) still works
  unchanged -- it now also makes that application visible to `QueryApplications`/
  `FindApplications`, which is the spec-correct behavior (research.md R5), not a regression.

## Known, documented limitations (not silently glossed over)

- `UnregisterApplication` does not revoke certificates issued to the application (Part 12 §6.5.8
  requires this) -- the ledger/CRL infrastructure that would require doesn't exist yet (same gap
  already tracked for CU 3582's `RevokeCertificate`).
- No `ApplicationRegistrationChanged` audit event is emitted -- consistent with every other GDS
  method in this codebase (none currently emit audit events either).
- `ApplicationRecordDataType`'s wire encoding identifier is this project's own convention (the
  DataType's real NodeId, reused as its own encoding id), since the vendored companion NodeSet
  doesn't define separate encoding-object metadata for it -- a fully independent third-party GDS
  client/server expecting a different convention may not interoperate with this choice out of the
  box (research.md R8).
