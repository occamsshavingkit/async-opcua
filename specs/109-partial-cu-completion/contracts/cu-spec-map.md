# CU → OPC UA specification map (the authoritative grounding contract)

Each task cites its CU's row here. Before implementing or testing a CU, the implementer MUST read the listed Part/§ via the OPC-UA reference MCP (`search_terms`/`text`/`nodes`/`cu`) — or the local PDF at `~/opcua-specs/` — to confirm the exact required behavior. "Symbol to grep" is the current-code anchor (line numbers are NOT given because the audit's are stale).

| CU | Name | Kind | OPC UA spec to read | Symbol to grep | Behavior to assert/implement |
|---|---|---|---|---|---|
| 2275 | A&C Discrete Trip | A | Part 9 §5.8.x TripAlarmType; DiscreteAlarmType | `DiscreteAlarmKind`, `Trip` | Trip alarm activates/returns-to-normal; TypeDefinition = TripAlarmType |
| 2811 | State machine GeneratesEvent | B | Part 10 (Programs), Part 16 (State Machines), Part 3 §7 GeneratesEvent | `ProgramStateMachine`, `ShelvingStateMachine`, `GeneratesEvent` | Add GeneratesEvent ref instance→event type; browsable |
| 2814 | AvailableStates/AvailableTransitions | B | Part 16 FiniteStateMachineType, Part 5 §B.4.5 | `AvailableStates`, `AvailableTransitions` | Populate Property Values with real state/transition NodeId[] |
| 2918 | HasEventSource | B | Part 3 §7 HasEventSource / HasNotifier | `has_event_source`, `write_has_condition_reference` | Add HasEventSource ref notifier→source; browsable |
| 4466 | DialogCondition Respond2 | A | Part 9 §DialogConditionType Respond2 | `Respond2`, `respond2` | Call Respond2; dialog transitions/validates like Respond |
| 2289 | History UpdateEvent | A | Part 11 §6.8 UpdateEvent, PerformUpdateType | `PerformUpdateType::Update`, `update_event` | Update-mode upsert; both in-memory + sqlite |
| 2950 | Distinct server/source timestamp | B | Part 4 §7.7 DataValue; Part 11 read | `history_read_raw_modified`, `timestamps_to_return` | In-memory honors timestamps_to_return; distinct ts round-trip; both backends |
| 2422 | Audit over SecureChannel | A | Part 4 §Auditing; Part 2 §Security | `dispatch_audit_event`, `SignAndEncrypt` | Audit event delivered over encrypted channel |
| 3224 | NodeManagement audit | A | Part 4 §5.7 NodeManagement; AuditAddNodes/DeleteNodes/AddReferences/DeleteReferences | `dispatch_*`, `AuditAddNodes` | Emit audit for DeleteNodes/AddReferences/DeleteReferences |
| 3542 | RoleMappingRuleChanged audit | B | Part 18 §4 role management; Part 3 audit event | `add_identity`, `remove_identity`, `RoleMappingRuleChanged` | Emit RoleMappingRuleChangedAuditEventType on mapping change |
| 3968 | HistoryUpdate audit | B | Part 11 §Audit; AuditHistory*UpdateEventType | `dispatch_write_audit`, `HistoryUpdate` | New dispatch arm; emit AuditHistory*UpdateEventType |
| 3539 | RBAC ConfigureAdmin perms | A | Part 3 §well-known roles; Part 18 | `ConfigureAdmin`, `preset` | Assert ConfigureAdmin permission bitset |
| 3540 | RBAC AuthenticatedUser perms | A | Part 3 §well-known roles; Part 18 | `AuthenticatedUser`, `preset` | Assert AuthenticatedUser permission bitset |
| 3541 | RBAC Observer/Engineer/Supervisor perms | A | Part 3 §well-known roles; Part 18 | `Observer`, `Engineer`, `Supervisor`, `preset` | Assert each role's permission bitset |
| 2318 | MonitoredItem queueSize clamp | A | Part 4 §5.12.2/§7.16 MonitoringParameters | `sanitize_queue_size` | Over-max queueSize clamped to server max |
| 2818 | Monitor structured value | A | Part 4 §5.12 MonitoredItems | subscriptions sampling pipeline (`sample`) | Monitor a Structure value; notification carries it |
| 3142 | Monitor XML/JSON dataEncoding | A | Part 4 §5.12; Part 6 encoding | `data_encoding` in sampling | MonitoredItem with XML/JSON dataEncoding encodes as requested |
| 5208 | MonitoredItem IndexRange | A | Part 4 §7.22 NumericRange; §5.12 | `range_of`, `index_range` | MonitoredItem IndexRange delivers only ranged sub-value |
| 3544 | ResendData | A | Part 4 §5.13.6 ResendData | `Server_ResendData`, `resend_data` | ResendData resends current values next publish |
| 2203 | Write structured value | A | Part 4 §5.10 Write | `write_node_value` | Write ExtensionObject value; round-trips on read |
| 2454 | Call structure argument | A | Part 4 §5.11 Call | method call arg handling (`Call`) | Call with ExtensionObject arg round-trips into method |
| 3605 | MaxNodesPerMethodCall | A | Part 4 §5.11; Part 5 OperationLimits | `MaxNodesPerMethodCall`, `max_nodes_per_method_call` | Call over limit is rejected per spec |
| 2476 | Server_LocalTime | A | Part 5 §6.3.3 ServerStatusDataType; Part 3 TimeZoneDataType | `Server_LocalTime`, `TimeZoneDataType` | Read Server_LocalTime; plausible TimeZoneDataType |
| 3546 | Event localTime | B | Part 5 §6.4.2 BaseEventType | `local_time`, `Server_LocalTime` | Populate event localTime from Server_LocalTime source |
| 3194 | Max Select/Where ClauseParameters | B | Part 4 §event filter; Part 5 ServerCapabilities | `MaxSelectClauseParameters`, `MaxWhereClauseParameters` | Populate node Values from config limits |
| 3201 | Custom EventTypes + encoding objects | A | Part 3 §5.8 type system; Part 6 encoding | `samples/custom-codegen`, encoding_ids | Every custom EventType exposes its Encoding object(s) |
| 2823 | Security Invalid user token | A | Part 2 §6.6, CR 1.11; Part 4 ActivateSession | `tarpit_authentication_failure`, `IDENTITY_TOKEN_VALIDATION_TARPIT` | Invalid token rejected + tarpit delay (NO lockout — R1) |
