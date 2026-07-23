# Research: Complete the 27 Partial Conformance Units

All file paths below are **starting anchors verified 2026-07-23**, not line numbers — every task re-locates by grepping the named symbol (the audit's line numbers are stale). Spec sections are cited by Part/§ for the downstream implementer to read via the OPC-UA reference MCP (or the local PDFs at `~/opcua-specs/`).

## R1 — CU 2823 (auth-failure protection): DECISION = TEST-ONLY, no escalating lockout

**Question**: does closing "Security Invalid user token" require an escalating lockout, or is the existing fixed 100 ms tarpit sufficient?

**Resolved against the real spec text (Part 2 Security Model 1.05.06):**
- §6.6 "Rate limiting and flow control": *"OPC UA does not provide rate control mechanisms, however an implementation can incorporate rate control."* → rate control is OPTIONAL, not mandated.
- CR 1.11 "Unsuccessful login attempts": *"OPC does not provide temporary lock out for repeated user access failure, but an AuthenticationService could. OPC does monitor SecureChannel connection and could block secure channel connection for repeated user login failure."* → temporary lockout is explicitly **not** an OPC UA conformance requirement.

**Decision**: CU 2823 is **TEST-ONLY**. Do NOT implement escalating lockout. The conformance requirement is that the server (a) rejects an invalid user token with the correct status code and (b) does not leak timing information distinguishing "bad user" from "bad password" — which the existing constant fixed-100 ms tarpit (`tarpit_identity_token_validation_failure` / `tarpit_authentication_failure` in `async-opcua-server/src/session/negotiate.rs`) already provides. An escalating per-source lockout map would additionally be a **Principle IV liability**: an attacker forging many distinct source identities could grow that map unboundedly (a DoS vector), which is exactly the kind of attacker-influenced allocation the constitution forbids on a network-reachable path.

**Task**: add a test (in `async-opcua-server/tests/security_tests.rs`, near the existing tarpit test) that (1) submits an invalid user token and asserts the correct rejection status, and (2) asserts the failure path is delayed by the tarpit (the timing-mitigation behavior), without asserting any escalation. Read Part 2 §6.6 + CR 1.11 first.

## R2 — Type-A vs Type-B classification (confirmed)

**Type-B — needs a small fix + test (8 CUs)**: 2811, 2814, 2918, 2950, 3542, 3968, 3546, 3194. (2950 also fixes a real bug — see R3.)

**Type-A — test-only (19 CUs)**: 2275, 4466, 2289, 2422, 3224, 3539, 3540, 3541, 2318, 2818, 3142, 5208, 3544, 2203, 2454, 3605, 2476, 3201, 2823. (2823 confirmed test-only by R1.)

8 + 19 = 27.

## R3 — CU 2950 (distinct server/source timestamps): the in-memory backend bug

`async-opcua-server/src/node_manager/memory/simple.rs`'s `history_read_raw_modified` takes `_timestamps_to_return` (underscore-prefixed = deliberately ignored) and always returns the full stored `DataValue`. The sqlite backend honors the parameter. **Fix**: make the in-memory backend honor `timestamps_to_return` by masking the returned `DataValue`'s source/server timestamps to only those requested (Source / Server / Both / Neither), matching sqlite. Read Part 4 §7.7 (DataValue) + Part 11 read semantics. **Test** (both backends): store a value with a source timestamp deliberately distinct from its server timestamp, read back with `TimestampsToReturn::Both` and assert both distinct values survive; read back with `Source` only and assert the server timestamp is absent. Test files: `async-opcua-server/tests/history_data_inmemory.rs` + the sqlite history test in the `async-opcua-history-sqlite` crate.

## R4 — CU 2811 (GeneratesEvent): minimal wiring

`ReferenceTypeId::GeneratesEvent` already exists (`async-opcua-server/src/address_space/mod.rs`). ProgramStateMachine is instantiated in `async-opcua-server/src/programs/` (`state.rs`/`methods.rs`/`engine.rs`); ShelvingStateMachine in `async-opcua-server/src/alarms/state_machine.rs` / `registry.rs`. **Fix**: at each machine's instantiation, add a `GeneratesEvent` reference from the owning object to the event type(s) it emits (ProgramTransitionEventType for programs; the relevant condition/alarm event type for shelving). Read Part 10 (Programs) §for ProgramStateMachine GeneratesEvent, Part 16 (State Machines) §, Part 3 §7 (GeneratesEvent semantics). **Test**: browse the instance's GeneratesEvent references and assert the expected target type is present.

## R5 — CU 2814 (AvailableStates/AvailableTransitions): populate Properties

Same instantiation sites as R4 (plus any other FiniteStateMachine instances). The FiniteStateMachineType (Part 16 §, Part 5 §B.4.5) defines that a concrete instance exposes `AvailableStates` (NodeId[] of its State nodes) and `AvailableTransitions` (NodeId[] of its Transition nodes). Currently these Property nodes are not populated. **Fix**: at instantiation, set each Property's Value from the machine's actual state/transition node set. **Test**: read AvailableStates and AvailableTransitions and assert non-empty arrays matching the machine's declared states/transitions.

## R6 — CU 2918 (HasEventSource): wire the source hierarchy

`ObjectBuilder::has_event_source` exists (`async-opcua-nodes/src/object.rs`) with zero call sites. Alarms already wire `HasCondition` (see `write_has_condition_reference` in `alarms/deviation.rs`, `alarms/rate_of_change.rs`, `alarms/limit.rs`). **Fix**: mirror that pattern to add a `HasEventSource` reference so the notifier→event-source hierarchy (Server / area → source object) is navigable per Part 3 §7 (HasEventSource, HasNotifier). Add the reference where a source object first becomes an event source (alarm-source binding). Do NOT restructure the notifier tree — add the single missing reference type edge. **Test**: assert the HasEventSource reference exists from the notifier to the source and is browsable.

## R7 — CU 3542 (RoleMappingRuleChangedAuditEventType): emit on mapping change

`async-opcua-server/src/rbac/role_management.rs` registers `add_identity` / `remove_identity` for every well-known role's `*_AddIdentity` / `*_RemoveIdentity` method. The `RoleMappingRuleChangedAuditEventType` exists in the generated nodeset but nothing raises it. **Fix**: in `add_identity` and `remove_identity` (after a successful mutation), emit `RoleMappingRuleChangedAuditEventType` via the subscription/event machinery the other audit events use (see R8's `dispatch_*` pattern in `session/audit.rs`). Read Part 18 § 4 (role management) + the audit event type definition (Part 3/Part 5). **Test**: call AddIdentity/RemoveIdentity and assert the audit event is emitted.

## R8 — CU 3968 (HistoryUpdate audit): new dispatch arm

`async-opcua-server/src/session/audit.rs` has `dispatch_write_audit` (AuditWriteUpdateEventType), `dispatch_method_audit`, `dispatch_*` for add/delete nodes — but **zero** references to history (confirmed: `grep -ci history` = 0). **Fix**: add `dispatch_history_update_audit` mirroring `dispatch_write_audit`, emitting the correct `AuditHistory*UpdateEventType` subtype (AuditHistoryValueUpdate / AuditHistoryEventUpdate / AuditHistoryDelete as appropriate). Wire the call at the HistoryUpdate service handler (`async-opcua-server/src/session/services/attribute.rs` or the node-manager history-update path — re-grep `HistoryUpdate`). Read Part 11 (audit events) + Part 3/Part 5 AuditHistoryUpdateEventType. **Test**: perform a HistoryUpdate and assert the audit event is emitted (test in `session/audit.rs` `#[cfg(test)]` module or a history test).

## R9 — CU 3546 (event localTime) reuses the CU 2476 source

`Server_LocalTime` is computed at `async-opcua-server/src/node_manager/memory/core.rs` (`VariableId::Server_LocalTime => ExtensionObject::from_message(TimeZoneDataType { ... })`). BaseEventType's `local_time` field is read by `get_value` but never assigned. **Fix**: when an event is constructed/emitted, populate its `local_time` from the **same** `TimeZoneDataType` computation Server_LocalTime uses (extract that computation into a small shared helper if needed, so the two agree). Read Part 5 §6.4.2 (BaseEventType localTime) + §6.3.3 (ServerStatusDataType). **Test (3546)**: emit an event, read its localTime, assert non-null/plausible. **Test (2476, Type-A)**: read Server_LocalTime attribute and assert a plausible TimeZoneDataType.

## R10 — CU 3194 (MaxSelectClauseParameters/MaxWhereClauseParameters): populate node values

These two ServerCapabilities Variable nodes exist in the generated nodeset but their Value is `DataValue::null()`. There is currently **no** config field for them (grep found none). **Fix**: (1) determine the server's actual effective event-filter limits — look at the event-filter evaluator in `async-opcua-server/src/` (event_filter / where-clause parsing) for any hardcoded cap; if none, define sensible defaults (e.g. matching other operation limits) as config fields on the server limits struct; (2) populate the two ServerCapabilities node Values from those config fields at address-space construction (the same place other ServerCapabilities/OperationLimits nodes get their live values). Read Part 4 (event filter, SelectClause/WhereClause) + Part 5 (ServerCapabilities). **Test**: read both nodes and assert non-null values equal to the configured limits. Test file: `async-opcua-server/tests/event_filter_tests.rs`.

## R11 — Ledger flip + CU-COVERAGE.md regeneration

For each of the 27 CUs, change its `AUDIT_TABLE` row in `tools/cu-coverage-report/src/lib.rs` from `EvidenceStatus::Partial` to `EvidenceStatus::Implemented`, with a **re-verified** evidence string naming the current file:line of the code and the exact new test name. Then regenerate `specs/conformance-tester/CU-COVERAGE.md`:
```
cargo run -p async-opcua-cu-coverage-report --quiet -- \
  /home/quackdcs/micro-opcua/profiles/opcua-profile-normalized-snapshot.json \
  specs/conformance-tester/CU-COVERAGE.md
```
(This is the same snapshot + command the feature-108 close used.) The `cu-coverage-report` crate has its own tests asserting specific rows — update any that assert `2823`/etc. as `partial`.

## R12 — Test locations, feature gates, grounding rule (applies to every task)

- **Test file map** (starting points; a task may add to a `#[cfg(test)]` module in the source file instead when that's where the CU's peers are tested):
  - A&C (2275, 2811, 2814, 2918, 4466): the alarms unit-test module (`async-opcua-server/src/alarms/…` `#[cfg(test)]`) or a dedicated alarms integration test.
  - History (2289, 2950): `tests/history_events_inmemory.rs`, `tests/history_data_inmemory.rs`, `tests/history_tests.rs`, plus the sqlite history tests in `async-opcua-history-sqlite`.
  - Audit (2422, 3224, 3542, 3968): `session/audit.rs` `#[cfg(test)]` + `tests/security_tests.rs`.
  - RBAC (3539, 3540, 3541): the rbac unit-test module (`async-opcua-server/src/rbac/…` `#[cfg(test)]`).
  - Subscriptions/MI (2318, 2818, 3142, 5208, 3544): the subscriptions unit tests / `tests/method_call_tests.rs` / `tests/event_filter_tests.rs`.
  - Read/Write/Call (2203, 2454, 3605): `tests/method_call_tests.rs`, `tests/stateful_tests.rs`.
  - Server meta/event/filter (2476, 3546, 3194): `core.rs` `#[cfg(test)]`, `tests/event_filter_tests.rs`.
  - Custom types (3201): `samples/custom-codegen` test suite.
  - Security (2823): `tests/security_tests.rs`.
- **Feature gates**: history CUs need `history` / the sqlite backend crate; RBAC needs `rbac`; subscriptions need `subscriptions`/`subscriptions-standard`; alarms need `alarms`; method needs `method-call`. Run each CU's test under a feature set that compiles its code; keep the no-default-features workspace build green.
- **Grounding rule (mandatory, every task)**: (1) `grep` for the named symbol to find current code — never trust the audit's line numbers; (2) read the cited Part/§ via the OPC-UA reference MCP before writing code or a test; (3) author the test to assert the spec's observable behavior, not the implementation's shape.

## Decisions summary (all resolved — implementer never chooses)

| CU | Type | Resolution |
|---|---|---|
| 2823 | A (test-only) | No lockout; spec makes it optional (R1). Test rejection status + tarpit delay. |
| 2950 | B | Fix in-memory backend to honor `timestamps_to_return`; test both backends (R3). |
| 2811 | B | Add GeneratesEvent ref at machine instantiation (R4). |
| 2814 | B | Populate AvailableStates/AvailableTransitions Properties (R5). |
| 2918 | B | Add HasEventSource ref mirroring HasCondition wiring (R6). |
| 3542 | B | Emit RoleMappingRuleChanged in add/remove_identity (R7). |
| 3968 | B | New dispatch_history_update_audit arm + wire at HistoryUpdate (R8). |
| 3546 | B | Populate event localTime from Server_LocalTime's source (R9). |
| 3194 | B | Populate MaxSelect/MaxWhereClause node values from config limits (R10). |
| all others (19) | A | Add an independent test asserting the CU's spec behavior. |
