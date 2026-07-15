# OPC UA Foundation Profile Roadmap

This roadmap compares the canonical OPC UA Foundation profile snapshot supplied
out-of-tree via `ASYNC_OPCUA_PROFILE_SNAPSHOT_DIR` with evidence currently
present in this repository. The snapshot used for this pass lived at
`/home/quackdcs/micro-opcua/profiles`; copy or generate the same snapshot tree
before regenerating these counts. It is intentionally conservative: code or
generated namespace nodes alone are not treated as certification proof unless a
test, conformance guide, or explicit profile smoke test exercises the behavior.

## Inputs

| Source | Observed content | Use in this roadmap |
|---|---:|---|
| `$ASYNC_OPCUA_PROFILE_SNAPSHOT_DIR/opcua-profile-normalized-snapshot.json` | 76 profiles, 7 facets, 1182 CUs, 4 canonical server profiles | Canonical source of profile-to-CU closures |
| `$ASYNC_OPCUA_PROFILE_SNAPSHOT_DIR/opcua-profile-snapshot.json` | 4 profiles, 9 facets, 6 CUs | Raw/smaller snapshot, not sufficient for roadmap |
| `$ASYNC_OPCUA_PROFILE_SNAPSHOT_DIR/opcua-profile-manifest.yaml` | JSON-formatted manifest with 6 profiles, 178 items, 21 capacities | Curated build/profile metadata |

Canonical active server profile closures from the normalized snapshot:

| Profile OPC ID | Canonical profile | CU closure size |
|---:|---|---:|
| 2266 | Nano Embedded Device 2025 Server Profile | 51 |
| 2267 | Micro Embedded Device 2025 Server Profile | 62 |
| 2268 | Embedded 2025 UA Server Profile | 119 |
| 2269 | Standard 2025 UA Server Profile | 123 |

The closure for Standard includes one relationship ID, `5592`, that is not present
in the snapshot's `conformance_units` list. Treat it as a source-data ambiguity
until the upstream profile extractor is fixed or the OPC Foundation source page is
checked manually.

## Current Repository Understanding

The repo's current conformance understanding is mostly service-area oriented, not
CU-ID oriented.

| Artifact | What it proves |
|---|---|
| `specs/conformance-tester/COVERAGE.md` | CI-runnable conformance coverage by service area, with oracle strength noted |
| `specs/conformance-tester/PLAN.md` | Existing plan to add a CU registry and map checks to CUs |
| `docs/ctt-conformance.md` | Authoritative UACTT run guide and expected pass/fail boundaries |
| `specs/conformance-gap-backlog.md` | Previous gap backlog; current remaining item is authoritative CTT execution |
| `samples/foundation-profile-*-server/tests/profile_smoke.rs` | Profile smoke tests for Nano, Micro, Embedded, and Standard sample builds |

## Evidence Summary

Strong in-repo evidence exists for these areas:

| Canonical area | Evidence |
|---|---|
| Discovery GetEndpoints / FindServers Self / RegisterServer / RegisterServer2 | `specs/conformance-tester/COVERAGE.md`; `async-opcua/tests/integration/discovery.rs`; Standard profile smoke comments and `register_server2_flow` |
| UA TCP / UA Secure Conversation / UA Binary / security-policy negotiation | `specs/conformance-tester/COVERAGE.md`; `docs/ctt-conformance.md`; integration matrix under `async-opcua/tests/integration/conformance.rs` |
| Session Base / multiple sessions / Cancel / ChangeUser-style token matrix | `specs/conformance-tester/COVERAGE.md`; `samples/foundation-profile-standard-server/tests/profile_smoke.rs`; token matrix references in `docs/ctt-conformance.md` |
| Attribute Read / Write | `specs/conformance-tester/COVERAGE.md`; `async-opcua/tests/integration/read.rs`; `async-opcua/tests/integration/write.rs` |
| Browse / BrowseNext / TranslateBrowsePaths / RegisterNodes | `specs/conformance-tester/COVERAGE.md`; Nano profile smoke test |
| Subscriptions and MonitoredItems | `specs/conformance-tester/COVERAGE.md`; Micro and Embedded profile smoke tests; `async-opcua/tests/integration/subscriptions.rs` |
| Security user tokens: Anonymous, Username, X509, invalid-token rejection | `docs/ctt-conformance.md`; Nano and Standard profile smoke tests; conformance coverage matrix |
| NamespaceMetadata / ServerCapabilities / diagnostics nodes | `async-opcua/tests/integration/conformance.rs`; `async-opcua/tests/integration/read.rs`; `async-opcua/tests/integration/browse.rs` |
| NodeManagement optional surface | `async-opcua/tests/integration/node_management.rs`; note that core address space remains read-only unless `clients_can_modify_address_space` is enabled |
| GDS push/pull certificate management methods | `docs/gds.md`; `async-opcua-server/src/gds/pull_methods.rs`; `async-opcua-server/tests/gds_pull_methods.rs` |
| HistoryRead / aggregates | `specs/conformance-tester/COVERAGE.md`; `async-opcua/tests/integration/hda.rs`; `async-opcua/tests/integration/read.rs`; aggregate scope remains partial |

## Roadmap Buckets

### Bucket 1: CU Registry And Evidence Index

Goal: convert current service-area conformance knowledge into a CU-indexed report.

Required work:

| Step | Work item | Acceptance evidence |
|---|---|---|
| 1 | Add an in-repo normalized CU registry generated from the Foundation snapshot | Registry contains the four active server profile closures and preserves OPC IDs/names/descriptions |
| 2 | Tag existing tests with canonical CU IDs | `cargo test` output can be summarized by CU ID, not only by test file |
| 3 | Emit `implemented`, `partial`, `gap`, and `not-applicable` statuses | Generated report lists evidence paths per CU |
| 4 | Add a CI check that fails on stale generated coverage data | CI validates registry/report freshness |

This is already anticipated by `specs/conformance-tester/PLAN.md` phase 3. It is
the highest-leverage next step because the codebase already has substantial test
coverage but lacks an authoritative CU-indexed ledger.

### Bucket 2: Certification-Critical Profile Proof

Goal: make the four canonical server profiles demonstrably match their CU closures.

Priorities:

| Priority | CUs / area | Current state | Next action |
|---:|---|---|---|
| 1 | Time Sync CUs: `2478`, `2479`, `2480`, `2786`, `3802`, `5505`, `5793` | **Resolved (feature 093)**. See `docs/time-synchronization.md`: 2478/3802/5505/5793 claimed (OsClockSource default, configurable clock skew, opt-in UA-based poller); 2479/2480/2786 documented as a user-supplied `TimeSyncSource` extensibility point, grounded in OPC-10000-84 §6.6.3.6. | Closed. |
| 2 | Base Info ServerCapabilities: `3911`, `3912`, `4055` | Nodes and tests exist for many capabilities, but CU-level completeness is not proven. | Add a CU-specific read/browse test that checks every mandatory property named in each CU description. |
| 3 | NamespaceMetadata: `3545` | Tests browse/read NamespaceMetadata properties. | Tie tests to CU `3545` and verify all static namespaces expose required metadata. |
| 4 | Security Administration / Role authorization: `2407`, `2808` | RBAC and security configuration tests exist. | Add profile-level assertions that configured roles, permissions, security policies, and trust behavior match the CU descriptions. |
| 5 | GDS Push Model: `2231` | GDS method implementation and tests exist. | Verify complete Global Certificate/TrustList Management push-model semantics against Part 12 and attach CU `2231`. |
| 6 | Attribute Write extensions: `2936`, `3147`, `2820` | Write tests and NumericRange handling exist, but CU-specific full-array/status/timestamp proof needs confirmation. | Add CU-named write tests for StatusCode/Timestamp writes, IndexRange writes, and AccessLevelEx WriteFullArrayOnly behavior. |
| 7 | Base Info datatype/reference CUs | Generated namespace contains many standard nodes; address-space oracle covers a large standard type surface. | Generate a Base Info CU checklist that maps each CU to NodeIds and required attributes. |
| 8 | Standard-only CUs: `2190`, `2271`, `3125`, `3170` | Standard smoke covers Cancel and RegisterServer2 reachability; broader coverage exists for discovery and X509 token plumbing. | Add profile-level tests that prove RegisterServer registration observability and X509 activation over SignAndEncrypt, not only reachability. |

### Bucket 3: Known Partial Areas From Existing Coverage

These areas already have implementation/test evidence but are not fully closed for
canonical-profile certification:

| Area | Evidence | Remaining gap |
|---|---|---|
| JSON/XML encoding | `specs/conformance-tester/COVERAGE.md` marks partial | Cross-stack JSON/XML vector strategy remains deferred because byte-equality is fragile |
| Aggregates | `specs/conformance-tester/COVERAGE.md` marks partial | Only a subset such as TimeAverage/Min/Max/StdDevSample is implemented/tested |
| FindServersOnNetwork mDNS | Coverage doc notes LDS-ME requirement | Needs a live LDS-ME/mDNS counterparty when validating multicast discovery |
| UACTT authority | `docs/ctt-conformance.md` | Full authoritative CTT run must be performed on Windows |

## Profile Deltas

These are the CUs newly added at each canonical profile tier relative to the prior
tier. Status is first-pass and evidence-driven, not a certification claim.

### Nano 2025 Additions

| CU | Name | First-pass status |
|---:|---|---|
| 2317 | View TranslateBrowsePath | Evidenced |
| 2328 | Discovery Get Endpoints | Evidenced |
| 2352 | Discovery Find Servers Self | Evidenced |
| 2371 | Protocol UA TCP | Evidenced |
| 2389 | Attribute Write Values | Evidenced |
| 2400 | Session Change User | Partial |
| 2407 | Security Administration | Partial |
| 2446 | Address Space AddIn Reference | Needs CU-specific proof |
| 2447 | Address Space AddIn DefaultInstanceBrowsename | Needs CU-specific proof |
| 2476 | Base Info LocalTime | Needs CU-specific proof |
| 2478 | Time Sync - OS based support | Evidenced (feature 093; `docs/time-synchronization.md`) |
| 2479 | Time Sync - IEEE 1588 (PTP) | Evidenced (extension point; feature 093) |
| 2480 | Time Sync - IEEE 802.1AS | Evidenced (extension point; feature 093) |
| 2600 | SecurityPolicy Support | Evidenced |
| 2711 | Base Info Selection List | Needs CU-specific proof |
| 2786 | Time Sync - NTP | Evidenced (extension point; feature 093) |
| 2808 | Security Role Server Authorization | Partial |
| 2809 | Address Space Atomicity | Needs CU-specific proof |
| 2820 | Address Space Full Array Only | Partial |
| 2837 | UA Binary Encoding | Evidenced |
| 2853 | UA Secure Conversation | Evidenced |
| 2936 | Attribute Write StatusCode & Timestamp | Partial |
| 2969 | Base Info ValueAsText | Needs CU-specific proof |
| 3072 | Attribute Read | Evidenced |
| 3073 | View RegisterNodes | Evidenced |
| 3080 | Security Default ApplicationInstance Certificate | Evidenced |
| 3127 | Base Info OptionSet | Needs CU-specific proof |
| 3147 | Attribute Write Index | Partial |
| 3175 | Session Base | Evidenced |
| 3184 | Base Info Core Structure 2 | Partial |
| 3186 | Base Info Core Views Folder | Partial |
| 3192 | Base Info Diagnostics | Partial |
| 3198 | Base Info Estimated Return Time | Needs CU-specific proof |
| 3201 | Base Info Custom Type System | Needs CU-specific proof |
| 3530 | View Basic 2 | Evidenced |
| 3545 | Base Info Namespace Metadata | Partial |
| 3554 | Address Space Base | Partial |
| 3560 | Address Space Interfaces | Partial |
| 3721 | Security ECC Policy | Evidenced |
| 3802 | Time Sync - Configure Clock Skew | Evidenced (feature 093; `docs/time-synchronization.md`) |
| 3808 | Documentation - Core Capacities | Partial |
| 3912 | Base Info Server Capabilities 2 | Partial |
| 3983 | Base Services Diagnostics | Partial |
| 3985 | Session General Service Behaviour | Evidenced |
| 4053 | Base Info Locations Object | Partial |
| 4237 | Address Space NonVolatile and Constant | Partial |
| 5240 | Base Info Currency | Partial |
| 5505 | Time Sync - UA based support | Evidenced (opt-in `time-sync-ua` feature; feature 093) |
| 5592 | Missing from normalized CU list | Canonical data issue |
| 5793 | Time Sync - Support | Evidenced (satisfied via 2478/5505; feature 093) |
| 5814 | Security - No Application Authentication | Evidenced |

### Micro 2025 Additions

| CU | Name | First-pass status |
|---:|---|---|
| 2963 | Monitor Basic | Evidenced |
| 3143 | Subscription PublishRequest Queue Overflow | Evidenced |
| 3196 | Base Info Fixed SamplingInterval | Needs CU-specific proof |
| 3727 | Subscription Basic | Evidenced |
| 3911 | Base Info Server Capabilities Subscriptions | Partial |
| 3913 | Subscription Publish Basic | Evidenced |
| 3922 | Base Info SemanticChange Bit | Needs CU-specific proof |
| 3923 | Session Multiple | Evidenced |
| 4055 | Base Info Server Capabilities MaxMonitoredItemsQueueSize | Needs CU-specific proof |
| 5207 | Monitor Items 2 | Evidenced |
| 5208 | Monitor Value Change V2 | Evidenced |

### Embedded 2025 Additions

| CU | Name | First-pass status |
|---:|---|---|
| 2231 | Push Model for Global Certificate and TrustList Management | Partial |
| 2423 | Base Info Rational Number | Needs CU-specific proof |
| 2481 | Base Info NormalizedString DataType | Needs CU-specific proof |
| 2482 | Base Info DecimalString DataType | Needs CU-specific proof |
| 2483 | Base Info Date DataTypes | Needs CU-specific proof |
| 2484 | Base Info BitFieldMaskDataType | Needs CU-specific proof |
| 2485 | Base Info KeyValuePair | Needs CU-specific proof |
| 2490 | Base Info Subvariables of Structures | Needs CU-specific proof |
| 2491 | Base Info AssociatedWith | Needs CU-specific proof |
| 2500 | Base Info EUInformation | Needs CU-specific proof |
| 2512 | Base Info OrderedList | Needs CU-specific proof |
| 2513 | Base Info Audio Type | Needs CU-specific proof |
| 2514 | Base Info Spatial Data | Needs CU-specific proof |
| 2516 | Base Info HasOrderedComponent | Needs CU-specific proof |
| 2517 | Base Info Deprecated Information | Needs CU-specific proof |
| 2518 | Base Info Image DataTypes | Needs CU-specific proof |
| 2536 | Base Info ContentFilter | Needs CU-specific proof |
| 2823 | Security Invalid user token | Evidenced |
| 2863 | Security Policy Required | Evidenced |
| 2928 | Monitored Items Deadband Filter | Evidenced |
| 2940 | Base Info GetMonitoredItems Method | Evidenced |
| 3146 | Monitor Triggering | Evidenced |
| 3185 | Base Info Core Types Folders | Needs CU-specific proof |
| 3188 | Base Info Base Types | Needs CU-specific proof |
| 3189 | Base Info ServerType | Needs CU-specific proof |
| 3207 | Base Info OptionSet DataType | Needs CU-specific proof |
| 3214 | Base Info Range DataType | Needs CU-specific proof |
| 3532 | Monitor Queueing | Evidenced |
| 3534 | Subscription Multiple | Evidenced |
| 3535 | Subscription Retransmission Queue | Evidenced |
| 3536 | Security User Name Password 2 | Evidenced |
| 3544 | Base Info ResendData Method | Evidenced |
| 3547 | Base Info UaBinary File | Needs CU-specific proof |
| 3550 | Base Info StatusResult DataType | Needs CU-specific proof |
| 3551 | Base Info UriString | Needs CU-specific proof |
| 3641 | Base Info Method Argument DataType | Needs CU-specific proof |
| 3644 | Base Info SemanticVersionString | Needs CU-specific proof |
| 3645 | Security User Token Unencrypted | Evidenced |
| 3747 | Base Info IsExecutableOn | Needs CU-specific proof |
| 3748 | Base Info IsExecutingOn | Needs CU-specific proof |
| 3749 | Base Info Controls | Needs CU-specific proof |
| 3750 | Base Info Utilizes | Needs CU-specific proof |
| 3751 | Base Info Requires | Needs CU-specific proof |
| 3752 | Base Info IsPhysicallyConnectedTo | Needs CU-specific proof |
| 3753 | Base Info RepresentsSameEntityAs | Needs CU-specific proof |
| 3754 | Base Info RepresentsSameHardwareAs | Needs CU-specific proof |
| 3755 | Base Info RepresentsSameFunctionalityAs | Needs CU-specific proof |
| 3756 | Base Info IsHostedBy | Needs CU-specific proof |
| 3757 | Base Info HasPhysicalComponent | Needs CU-specific proof |
| 3758 | Base Info HasContainedComponent | Needs CU-specific proof |
| 3759 | Base Info HasAttachedComponent | Needs CU-specific proof |
| 3996 | Base Info ReferenceDescription | Needs CU-specific proof |
| 4052 | Base Info TrimmedString | Needs CU-specific proof |
| 4054 | Base Info Handle DataType | Needs CU-specific proof |
| 4426 | Base Info Decimal DataType | Needs CU-specific proof |
| 5801 | Base Info Type Information | Partial |
| 5868 | Base Info Portable IDs | Needs CU-specific proof |

### Standard 2025 Additions

| CU | Name | First-pass status |
|---:|---|---|
| 2190 | Session Cancel | Evidenced |
| 2271 | Discovery Register | Evidenced |
| 3125 | Security User X509 | Partial |
| 3170 | Discovery Register2 | Evidenced |

## Recommended Execution Order

1. Land the CU registry/report generator first. This prevents further manual drift.
2. ~~Resolve Time Sync profile claims before advertising 2025 canonical profile parity.~~ Done — feature 093, see `docs/time-synchronization.md`.
3. Add CU-specific Base Info and ServerCapabilities tests. **Not yet done** — the
   2026-07-15 ground-truth audit (`docs/conformance-audit-2026-07-15.md`) supplies
   evidence and gap classification across all 76 server-side facets (492 CUs,
   not just the 4 canonical ones), which is what makes this item plannable, but
   it does not itself add any tests. See `TODO.md`'s Remaining section for the
   resulting precisely-scoped backlog items (e.g. the Node/Type/Method and
   Historical Access gaps) that still need real test/implementation work.
4. Strengthen Standard smoke tests for X509 user activation over SignAndEncrypt and observable RegisterServer/RegisterServer2 registration.
5. Run the Windows UACTT pass documented in `docs/ctt-conformance.md` after the CU ledger is green.

## Non-Claims

This document does not claim OPC Foundation certification. It is a source-backed
roadmap for reaching a CU-indexed conformance position and identifying the gaps
that are currently hidden by service-area coverage.
