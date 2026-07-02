# Data Model: Conformance Small-Items Sprint (053)

## US1 — Server diagnostics (P5-04)

| Entity | Kind | Fields / Notes |
|---|---|---|
| `ServerDiagnostics` (existing, `diagnostics/server.rs:8`) | runtime state on `ServerInfo` | gains: mapped-id coverage for the 5 new VariableIds; `enabled` becomes externally readable/writable (privileged) as `EnabledFlag` |
| `SubscriptionDiagnosticsDataType` (generated, exists) | wire struct | one per live subscription; populated from `Subscription` getters (publishing interval, lifetime/keep-alive counts, priority, item counts, publish/notification counters) |
| `SessionDiagnosticsDataType` (generated, exists) | wire struct | one per live session; from `Session` getters + `session_locale_ids`; request counters where tracked |
| `SessionSecurityDiagnosticsDataType` (generated, exists) | wire struct | client cert/security mode/policy/endpoint per session; **security-sensitive** — admin-gated |
| `SessionManager::iter_sessions` (new) | accessor | read-only enumeration of `Arc<RwLock<Session>>`; consistent snapshot semantics |
| `Subscription` diagnostics getters (new) | accessors | read-only; no state change |

**Validation rules**: array reads gated on `read_diagnostics` permission (existing pattern,
`core.rs:587`); `SessionSecurityDiagnosticsArray` additionally requires an administrative
identity; `EnabledFlag=false` → arrays read empty per Part 5. `EnabledFlag` write requires
privileged session; toggling transitions `ServerDiagnostics.enabled` (state machine:
enabled↔disabled; counters keep accumulating internal correctness either way, exposure is gated).

## US2 — Write range/enum validation (P4-ATTR-04)

| Entity | Kind | Fields / Notes |
|---|---|---|
| `EURange` constraint | modeled property (`Range{low,high}` ExtensionObject) | resolved via HasProperty "EURange" (pattern: `alarms/limit.rs:199`) |
| `EnumDefinition` value set | `DataTypeDefinition::Enum` on the DataType node | valid values = `EnumField.value` set; absent definition ⇒ no validation |
| Write validation outcome | per-op StatusCode | new rejection: `BadOutOfRange`; ordering: after RBAC/type checks, before `set_value_range` |

**Rules**: numeric scalar/array elements compared against [low, high]; enum writes must match a
defined field value; index-ranged writes validate written elements only; unconstrained Variables
unaffected.

## US3 — LocalizedText locale store (P4-ATTR-03)

| Entity | Kind | Fields / Notes |
|---|---|---|
| `LocalizedTextAttributeValues` (existing, `utils.rs:18-23`) | per-server DashMap `(NodeId, AttributeId) → Vec<LocalizedText>` | gains: null-text delete transition; null-locale → default-text update rule |

**State transitions** (per key): add locale (write text+locale), update locale (overwrite same
locale), delete locale (write null text + locale), default update (null locale). Unsupported
locale ⇒ `BadLocaleNotSupported`, store unchanged. Value attribute: single-locale scalar semantics
retained (documented server-specific choice).

## US4 — maxAge (P4-ATTR-02)

| Entity | Kind | Fields / Notes |
|---|---|---|
| Freshness decision | read-path rule | `age = now - source_timestamp`; refresh iff `maxAge == 0 || age > maxAge` and the node has a refreshable source (callback/sampler); in-memory plain values are always current |

No new stored state; `max_age` already plumbed to both sinks.

## US5 — EURange refresh + SemanticsChanged (P8-02)

| Entity | Kind | Fields / Notes |
|---|---|---|
| Range-change notice | new signal write-path → subscription cache | keyed by monitored (owner) NodeId; fired when a Variable's `EURange` property Value is written |
| `MonitoredItem.eu_range` (existing, :401) | cached filter input | now refreshed on range-change notice (reuses modify() seam) |
| `MonitoredItem.semantics_changed` (new) | one-shot bool | set on range change; ORed into next queued notification's StatusCode (`set_semantics_changed`), then cleared (pattern: overflow bit, :861-879) |

## US6 — AccessLevelEx (P3-09)

| Entity | Kind | Fields / Notes |
|---|---|---|
| `Variable.access_level_ex_extended` (new) | node field (extended bits only, default 0) | full attribute value derived: `(extended_bits << 8) | access_level as u32` — low byte can never diverge from AccessLevel (Part 3 §5.6.2) |

Read arm in `Variable::get_attribute_max_age`; set arm honoring WriteMask bit 25; builder setter.

## US7 — P5-03 closure

No data model change; artifacts = lock-in test + FINDINGS.md row update (status `not-a-bug`,
evidence: `diagnostics/node_manager.rs:178/:418/:463`, Part 5 §6.3.13/§6.3.14).
