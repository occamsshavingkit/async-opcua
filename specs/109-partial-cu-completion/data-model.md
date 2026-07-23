# Data Model: Complete the 27 Partial Conformance Units

This feature is predominantly test-addition; only the 8 Type-B CUs change data shapes, and all changes are additive/minimal. No new persisted schema, no wire-protocol change.

## Changed / added data

### Ledger (all 27 CUs)
- **`AUDIT_TABLE` rows** (`tools/cu-coverage-report/src/lib.rs`): 27 tuples change their `EvidenceStatus` field `Partial → Implemented` and their evidence string (re-verified file:line + new test name). No id or ordering change.
- **`CU-COVERAGE.md`**: regenerated artifact; 27 rows flip status. Aggregate facet counts recompute (Partial↓, Implemented↑).

### CU 3194 — event-filter limit config
- **New config fields** on the server limits struct (only if none exist): `max_select_clause_parameters: u32`, `max_where_clause_parameters: u32`, defaulting to the server's current effective event-filter caps. These feed the two ServerCapabilities Variable node Values.
- **ServerCapabilities nodes** `MaxSelectClauseParameters` / `MaxWhereClauseParameters`: Value changes from `DataValue::null()` → live `UInt32` from config.

### CU 2814 — FiniteStateMachine instance Properties
- **`AvailableStates`** Property Value: `null` → `NodeId[]` of the instance's State nodes.
- **`AvailableTransitions`** Property Value: `null` → `NodeId[]` of the instance's Transition nodes.
- Values are derived at instantiation from the machine's own definition; no new storage.

### CU 3546 — event field population
- **`BaseEventType.localTime`**: `null`/unset → `TimeZoneDataType` value, populated from the same source as `Server_LocalTime`. Applies to every emitted event.

### CUs 2811 / 2918 — address-space references (edges, not nodes)
- **GeneratesEvent** reference: added from each state-machine instance object → the event type it emits.
- **HasEventSource** reference: added from the notifier/area → the event-source object.
- Both are new reference edges using existing reference types; no new node classes.

### CUs 3542 / 3968 — audit events (transient, not stored)
- **`RoleMappingRuleChangedAuditEventType`** instance: emitted transiently on identity-mapping mutation (not persisted).
- **`AuditHistory*UpdateEventType`** instance: emitted transiently on HistoryUpdate (not persisted).
- Both reuse the existing `dispatch_*` → SubscriptionCache event-delivery path.

### CU 2950 — DataValue timestamp masking (behavior, not shape)
- In-memory `history_read_raw_modified` return: full `DataValue` → `DataValue` with source/server timestamps masked per `TimestampsToReturn`. Same type, corrected content.

## Invariants preserved
- No CU change alters an existing wire encoding or node identity.
- Type-B additions reuse existing machinery (reference types, audit dispatch, config-limits pattern, TimeZoneDataType computation) — no new subsystem.
- The 3 Extensible time-sync CUs and the 141 Gap CUs are untouched.
