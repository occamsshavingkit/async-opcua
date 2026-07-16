# TODO

Ideas that could be implemented.

## Remaining

Conformance backlog below is derived from `docs/conformance-audit-2026-07-15.md`,
a 2026-07-15 ground-truth audit covering all 76 OPC UA Foundation server
profiles/facets (492 CUs), not just the 4 canonical composite profiles
(123 CUs) previously tracked. See that doc for full per-theme evidence;
`specs/conformance-tester/CU-COVERAGE.md` has the per-CU ledger.

- ~~**Alarms & Conditions missing subtypes/properties**~~ — feature 095 (both passes) closed `TransitionTime`/`EffectiveTransitionTime`/`EffectiveDisplayName` on the core sub-states (14 of the 58-CU range — the remaining 44 are per-substate `Effective*` beyond `ActiveState` and per-specific-transition-edge timestamps, a materially larger increment, still open), all 10 Enable/Disable/Suppress/Unsuppress/RemoveFromService/PlaceInService/Silence Methods, A&C auditing for those Methods, and the full missing-alarm-subtype set: Level (`2746`/`3001`), Deviation (`2390`/`2951`, `DeviationAlarm` reusing `LimitAlarm`'s evaluator against `processValue-setpointValue`), RateOfChange (`2323`/`2946`, per-second rate over successive samples), `SystemOffNormalAlarmType` (`2239`, `DiscreteAlarmKind` variant), `CertificateExpirationAlarmType` (`2236`, `ExpirationDate`/`ExpirationLimit`), `DiscrepancyAlarmType` (`2861`, `TargetValueNode`/`ExpectedTime`/`Tolerance`, based directly on `AlarmConditionType` not `LimitAlarmType` per grounding), `OnDelay`/`OffDelay` (`2877`, `ConditionStateMachine::gate_active`), `ReAlarmTime`/`ReAlarmRepeatCount` (`2879`, resets on return-to-normal not acknowledge — corrected a wrong initial assumption via spec grounding), `AudibleSound`/`AudibleEnabled` (`2881`, server-computed from active/acked/silenced; found and fixed a real spec-conformance gap where Acknowledge didn't auto-silence). See `specs/095-ac-completion/tasks.md` T045-T052.
- **GDS Directory / Authorization / KeyCredential services**: `RegisterApplication`/`QueryServers`/`QueryApplications` are explicitly `BadServiceUnsupported` (not missing by accident); Authorization Service (OAuth2 token issuance) and KeyCredential Service don't exist at all; JWT/OAuth2 authority discovery (issuer endpoint URL) is absent. CUs `2232`, `2233`, `2709`, `2817`, `2902`, `3182`, `3581`, `3584`, `3586`, `5292`, `5293`, `5301`, `5302`, `5303`.
- **File Access / Temporary File Access**: `FileType` node metadata exists but no `add_method_cb` callback wired anywhere — Open/Read/Write/Close Methods are inert nodes, not functional I/O. CUs `3210`, `3211`, `3213`, `3810`-`3813`, `5791`.
- **DataAccess variable-type instances**: `TwoState`/`MultiState`/`MultiStateValueDiscrete`/`DiscreteItemType`/`ArrayItemType` family never instantiated beyond `AnalogItemType`. CUs `2361`, `2426`, `2474`, `2776`, `2831`, `2988`, `3323`-`3327`.
- **RBAC RolePermissions Write path**: no live Write-service path sets `RolePermissions`/`DefaultRolePermissions` (only settable at node-creation/config time); `UserWriteMask` is static, never per-user computed. CUs `2806`, `2873`, `2163`, `3026`.
- **Historical `ReadAtTimeDetails`**: zero server-side implementation for any backend — `SimpleNodeManagerImpl` overrides every other `history_read_*` but not this one. CU `3020` (and dependents `2991`).
- **Historical structured-data update/delete**: both backends' `update_structure_data` accept only `Annotation`-typed values, contrary to what several CUs require. CUs `2185`, `2332`, `2664`, `2740`, `2937`, `3015`.
- **Attribute Write remaining gaps**: `WriteFullArrayOnly` bit stored but never enforced against IndexRange array writes (`2820`); Write doesn't have a test proving a value round-trips post-Write (`2936`). (IndexRange writes via `Variant::set_range_of`, CU `3147`, is done.)
- **Subscription Durability**: `SetSubscriptionDurable` is an inert generated nodeset entry; zero "durable" references in the server crate. CUs `3642`, `5795`.
- **Scheduler / Redundancy / Sessionless**: all three are unimplemented beyond generated stubs — Scheduler is not the same as the existing Part-10 `ProgramStateMachine`; Redundancy has no failover/clustering code; sessionless enforcement has an explicit TODO (`rbac/decision.rs:147`). CUs `4500`-`4503`, `2258`, `3027`, `3994`, `2871`.
- **PubSub MQTT config completion**: finish `specs/074-pubsub-gauntlet/tasks.md` T011/T012 by deriving QoS from `RequestedDeliveryGuarantee` and using configured `BrokerDataSetReaderTransportDataType.QueueName` topic filters instead of only writer/reader group IDs.
- **Conformance known partials**: close or document the remaining JSON/XML cross-stack vector strategy, FindServersOnNetwork mDNS counterparty validation, and audit-event coverage from `specs/conformance-tester/COVERAGE.md`. (Part 13 aggregate coverage: confirmed solid — 35/35 standard aggregates implemented and tested; Start/End variants and custom-aggregate extensibility are the only real gaps, see audit doc.)
- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool on Windows. See `docs/ctt-conformance.md`.
- ~~**Technical debt quick wins**: fix companion feature-name drift between `async-opcua-server/Cargo.toml` and `async-opcua-server/src/companion/mod.rs`; remove or narrow the `unexpected_cfgs` allowance; add a consistency check for declared companion features versus importer gates.~~
- ~~**Spec Kit reconciliation**: update `specs/069-companion-specs/tasks.md` and related plan/spec artifacts so they match the current companion-spec implementation strategy, or document the intended migration path if runtime XML import is temporary.~~
- ~~**Documentation cleanup**: update `docs/compatibility.md` so the crypto backend description matches the current `aws-lc-rs`/`rustls`/Rust-crypto implementation instead of stale OpenSSL wording.~~
- **Async lock audit**: audit synchronous lock use in session, subscription, secure-channel, and SQLite history paths; confirm lock guards are not held across `.await`; add targeted high-contention tests where needed.
- **Dependency debt review**: revisit `deny.toml` advisory exceptions and duplicate dependency stacks (`tokio-tungstenite`, `thiserror`, `rand`, `getrandom`, `rustix`, async runtime support crates); collapse versions where upstream compatibility allows.
- **Companion import strategy decision**: decide and document whether companion specs should use generated Rust, runtime XML imports, or a hybrid model; measure build time and footprint before enabling broad companion support.
- **Protocol TODO conversion**: turn high-impact protocol TODOs into grounded implementation tasks with OPC UA section references, including query partial-success behavior, `Variant::set_range_of` completeness, and event filter `RelatedTo` support.
- **Large-module refactoring backlog**: split the largest handwritten modules only around stable domain boundaries, starting with subscription service/client code, memory node manager internals, session manager, secure channel, and subscription state handling.
- **Debt KPI tracking**: track generated-code size, non-generated LOC hotspots, build time, binary footprint, dependency advisories, and feature-lattice failures as recurring technical debt metrics. See `docs/technical-debt-report-2026-07-07.md`.

## Done

- ~~Server Capabilities & Diagnostics conformance completion~~ — feature 096: wired the remaining unwired `ServerCapabilities` Max* nodes (`MaxSessions` CU `3912`; `MaxMonitoredItemsQueueSize`/`MaxMonitoredItemsPerSubscription`/`MaxSubscriptionsPerSession`/server-wide `MaxSubscriptions`+`MaxMonitoredItems`=0 CU `3911`) to their existing config fields; documented (not built) why `SamplingIntervalDiagnosticsArray` is correctly absent — CU `3196` is conditional on a fixed sampling-interval set per OPC-10000-5 §7.9/§12.8, and this server negotiates continuously-variable intervals, so exposing the array would itself be non-conformant; added `docs/server-capacity-limits.md` (CU `3808`); proved the already-wired `Locations` object resolves via Browse (CU `4053`, no code fix needed). See `specs/096-server-capabilities-diagnostics/`.
- ~~Time Sync profile decision~~ — feature 093: `TimeSyncSource` extensibility trait; built-in `OsClockSource` default (CU 2478) + configurable `max_acceptable_clock_skew` (CU 3802) + opt-in `UaHeaderTimeSyncSource` poller (CU 5505, `time-sync-ua` feature); CU 5793 closes as a consequence. PTP/gPTP/NTP (CU 2479/2480/2786) documented as a user-supplied `TimeSyncSource` extension point rather than implemented in-library, grounded in OPC-10000-84 §6.6.3.6. See `docs/time-synchronization.md`.
- ~~CU registry and evidence report~~ — `tools/cu-coverage-report` + `specs/conformance-tester/CU-COVERAGE.md`: in-repo CU-indexed registry from the OPC UA Foundation profile snapshot, covering Nano/Micro/Embedded/Standard 2025 server profile closures with `implemented`/`partial`/`needs-proof`/`extensible`/`source-issue` evidence labels (PR #294; `extensible` added in feature 093).
- ~~Shrink foundation profile footprints~~ — feature 066: nano 12M→6.8M, micro 13M→7.3M, embedded 26M→17M (43-45% reduction via opt-level=z + LTO + strip).
- ~~Kerberos SSO: keytab path plumbing~~ — feature 065.
- ~~Kerberos SSO: integration test & CI KDC setup~~ — feature 065.
- ~~Kerberos SSO: GssapiIdentityValidator, feature flag, builder API, IssuedToken dispatch, role mapping~~ — feature 064.
- ~~Replace per-request timers with shared deadline queue~~ — feature 063 (US3).
- ~~Cache session Arc in request dispatch context~~ — feature 063 (US2).
- ~~Investigate ArcSwap debt overhead~~ — feature 063 (US4): 3 of 4 ArcSwap instances were startup-only and replaced with plain `Arc<T>`.
- ~~Split AddressSpace hot/cold: expose DashMap directly for reads~~ — feature 063 (US1).
- ~~Flesh out the server and client SDK with tooling for ease of use.~~ — feature 058 (QuickNodeManager builder API).
- ~~Make it even easier to implement custom node managers.~~ — feature 058.
- ~~RSA-KEM encrypted UserName token integration test~~ — feature 058.
- ~~Embedded profile secure channel smoke test~~ — feature 058.
- ~~Standard profile X509/RegisterServer2 tests~~ — feature 058.
- ~~Throughput benchmark regression: investigate and restore performance baseline.~~ — features 060 (compilation optimization, +11%) and 061 (hot-path audit fixes, allocation/caching/validation).
