# Research: Conformance Small-Items Sprint (053)

**Date**: 2026-07-02 · **Inputs**: `specs/conformance-audit/FINDINGS.md` (2026-07-01 reconciliation
banner), opc-ua-reference MCP grounding, 3 parallel code-mapping passes over the current tree.

All spec citations below were re-grounded against the reference MCP on 2026-07-02 (not taken from
the register). One register citation was wrong and is corrected here: **ServerDiagnosticsType is
Part 5 §6.3.3 (Table 11)**, not §6.3.2.

---

## R1 — P5-04: ServerDiagnostics mandatory children (US1)

**Spec ground** (Part 5 §6.3.3, Table 11): ServerDiagnosticsType members `ServerDiagnosticsSummary`
(Mandatory), `SubscriptionDiagnosticsArray` (`SubscriptionDiagnosticsDataType[]`,
`SubscriptionDiagnosticsArrayType`, Mandatory), `SessionsDiagnosticsSummary`
(`SessionsDiagnosticsSummaryType` Object, Mandatory — contains `SessionDiagnosticsArray` +
`SessionSecurityDiagnosticsArray`), `EnabledFlag` (Boolean Property, Mandatory);
`SamplingIntervalDiagnosticsArray` is Optional (out of scope).

**Codebase state**:
- `ServerDiagnosticsSummary` is ALREADY served: `async-opcua-server/src/diagnostics/server.rs`
  (`ServerDiagnostics` struct :8, `ServerDiagnosticsSummary` :108, `is_mapped` :137, `get` :156,
  `sample()` :176) read via the core manager catch-all `node_manager/memory/core.rs:586` gated on
  the `read_diagnostics` permission (:587–588). Counters fed from `session/manager.rs:739,784,904`
  and `subscriptions/mod.rs:599,696,698,1239,1282`.
- MISSING: `EnabledFlag` (2294), `SubscriptionDiagnosticsArray` (2290),
  `SessionsDiagnosticsSummary` (3706) + `SessionDiagnosticsArray` (3707) +
  `SessionSecurityDiagnosticsArray` (3708). Constants and all DataType structs already exist in
  `async-opcua-types` (`generated/types/{subscription,session,session_security}_diagnostics_data_type.rs`,
  `generated/node_ids.rs:11425–11426, :7405, :11669–11670`). Today the catch-all returns `None`
  (`core.rs:595`) → static/empty address-space values.
- Data sources: `SessionManager.sessions` is a private `HashMap<NodeId, Arc<RwLock<Session>>>`
  (`session/manager.rs:506`) with **no public all-sessions iterator — must be added**. `Session`
  getters map ~1:1 onto `SessionDiagnosticsDataType` (`session/instance.rs:293,468,298,427,458,463,
  448,473,228,106,453`); locale ids via `ServerInfo.session_locale_ids` (feature 049,
  `manager.rs:50`). Subscriptions enumerable via `SubscriptionCache::get_session_subscriptions`
  (`subscriptions/mod.rs:388`) / `with_session_subscriptions` (:401); `Subscription` fields
  (`subscription.rs:194–204`) are private — **getters must be added**.
- Config: `config/server.rs:556 diagnostics: bool` (default false :777), builder
  `builder.rs:672 diagnostics_enabled`; runtime flag `ServerDiagnostics.enabled`
  (`server.rs:420`). `EnabledFlag` value = this flag; a privileged write toggles it.

**Decision**: extend the existing `diagnostics/server.rs` + `core.rs` mapped-VariableId pattern to
the five missing NodeIds; add a sessions iterator on `SessionManager` and read-only diagnostics
getters on `Subscription`; per Part 5 SessionSecurityDiagnostics is security-sensitive → gate array
reads on the same `read_diagnostics` permission (already the pattern) with SessionSecurity
additionally admin-gated. When `EnabledFlag` is false, arrays serve empty per Part 5.
**Alternatives considered**: a separate diagnostics node manager (rejected — the mapped-id
read-path already exists and stays lock-light); sampling arrays continuously (rejected — compute
on read, matching `summary()` precedent).

## R2 — P4-ATTR-04: write range/enum validation (US2)

**Spec ground**: Part 4 §5.11.4 permits `Bad_OutOfRange` as a Write result; Part 8 §5.3.2.2
(AnalogItem EURange) and §5.3.3.3/§5.3.3.4 (MultiState*: "robust Servers should be prepared to
handle writes of illegal values, by providing error code Bad_OutOfRange"). This is
spec-permitted/recommended validation, not a hard SHALL — scope stays additive and conservative.

**Codebase state**: no EURange/enum check exists on the Value write path. Central validator:
`address_space/utils.rs:362 validate_node_write` (RBAC :368 → attribute-type :378 → locale :379 →
Value-specific :381–398 → per-locale capture :400); value application
`utils.rs:662 write_node_value` → `variable.rs set_value_range`. EURange resolution precedent:
`alarms/limit.rs:199 read_eurange` (find_node_by_browse_name HasProperty "EURange" → Variant
ExtensionObject → `Range{low,high}` :219–224). Enum values are NOT in the type tree
(`type_tree.rs` stores NodeClass/subtype/abstract only); they live on the DataType node's
`DataTypeDefinition` attribute (`data_type.rs:47`, `DataTypeDefinition::Enum(EnumDefinition)` in
`types/src/data_type_definition.rs:9–13`, `EnumField.value: i64`).

**Decision**: add the check inside `validate_node_write`'s Value arm (address space + type tree in
hand): resolve the target Variable's `EURange` property (reuse the `limit.rs` pattern) and, for
enum DataTypes, the DataType node's `EnumDefinition` fields; reject with `BadOutOfRange` before
`set_value_range`. Index-ranged writes validate the written elements. No constraint modeled → no
check (FR-004 no-regression clause). **Alternatives considered**: validating in the service layer
(rejected — the service has no address-space access; per-node-manager validation is the
established seam, same place as the feature-031 RBAC gate).

## R3 — P4-ATTR-03: LocalizedText write locale rules (US3)

**Spec ground** (Part 4 §5.11.4.1): for Attributes with DataType LocalizedText — add/overwrite by
locale, null String for text deletes the locale, invalid/unsupported LocaleId →
`Bad_LocaleNotSupported`; behavior for **Value** attributes is "Server specific but it is
recommended to follow" the same rules.

**Codebase state — largely implemented already (feature 049)**: per-locale side table
`LocalizedTextAttributeValues` for DisplayName/Description/InverseName (`utils.rs:18–23`,
qualifier :446), write-side capture `remember_localized_text_attribute_value` (:453, called at
:400 and :686), locale validation `validate_localized_text_attribute_write_locale` (:487) already
returns `BadLocaleNotSupported` (:514), read-side `apply_session_locale_to_localized_text` (:552)
applied at :627. Integration tests exist (`read.rs:135 localized_text_read_locale`,
`read.rs:203 unsupported_special_locale_write_is_rejected`).

**Decision**: this story is **gap-closure + lock-in, not greenfield**: (a) verify/implement
null-text-deletes-locale in the side table, (b) verify/implement null-locale → default-text rule,
(c) decide + document Value-attribute behavior (adopt the recommended rules only where they don't
conflict with plain value-write semantics; a Variant::LocalizedText value is a single-locale
scalar today — keep single-value semantics for Value, per the spec's "server specific" latitude,
and document it), (d) spec-grounded tests for the full matrix. **Alternatives considered**:
multi-locale storage for Value attributes (rejected — Part 4 leaves Value server-specific; a
multi-locale Value store would change subscription/data-change semantics for a non-mandatory
behavior).

## R4 — P4-ATTR-02: Read maxAge (US4)

**Spec ground** (Part 4 §5.11.2.2, Table 47): maxAge 0 → Server shall attempt to read a new value
from the data source; ≥ max Int32 → shall attempt cached value; between → value no older than
maxAge; negative invalid (`Bad_MaxAgeInvalid` — already enforced).

**Codebase state**: `max_age` is validated (`session/services/attribute.rs:30–31`) and plumbed
end-to-end (`node_manager/mod.rs:351` → `memory_mgr_impl.rs:1275/1286` → `utils.rs:573
read_node_value` → `node.rs:157–173 get_attribute_max_age`) but **discarded at both sinks**:
`variable.rs:613–618 Variable::value(_max_age)` and `base.rs:99–105`. The one place with a real
"source": `SimpleNodeManager` read callbacks already receive `max_age`
(`simple.rs:240–245`), and the internal sampler mechanism (`simple.rs:330/338`, `SyncSampler`)
holds sampled values with timestamps.

**Decision**: implement where a refreshable source exists — (a) sampler-backed/callback values:
if the cached `DataValue.source_timestamp` is older than `maxAge` ms (or maxAge==0), trigger a
fresh sample/callback before answering; (b) plain in-memory variables are definitionally current →
return as-is (documented; matches the trait doc "hint" contract and the spec's data-source
wording); (c) never silently drop the parameter for callback sources — pass it so user callbacks
can decide. Tests drive a callback/sampled node with a controllable source. **Alternatives
considered**: forcing timestamp rewrites on in-memory reads (rejected — fabricating freshness is
worse than truthful current values).

## R5 — P8-02: EURange dynamic refresh + SemanticsChanged (US5)

**Spec ground** (Part 8 §5.3.2.2, §5.2): "The StatusCode SemanticsChanged bit shall be set if any
of the EURange (could change the behaviour of a Subscription if a PercentDeadband filter is used)
… Properties are changed."

**Codebase state**: EURange read once at create/modify — `session/services/monitored_items.rs:35–98
get_eu_range` (TranslateBrowsePaths "EURange" :59 + Read :98), plumbed via `CreateMonitoredItem.eu_range`
(`monitored_item.rs:177`) into `MonitoredItem.eu_range` (:401, ponytail deferral comment
:398–400; modify() re-resolution seam :476–497). Percent math in `types/src/data_change.rs`
(`Deadband::Percent` :67–73, `ParsedDataChangeFilter::parse` :130–145, `is_changed` :89–109)
evaluated in `notify_data_value` (`monitored_item.rs:756–757`). `StatusCode::set_semantics_changed`
exists (`status_code.rs:162–166`) and is never called server-side. Status-bit injection precedent:
overflow bit set on queued notifications in `enqueue_notification` (`monitored_item.rs:861–879`).

**Decision**: detect EURange property writes server-side (the write path knows the node; a
property write to an `EURange` child of a monitored Variable) and notify the subscription cache →
affected monitored items (matched by monitored node) re-resolve the range (reuse `get_eu_range`
machinery / the modify() seam) and arm a one-shot `semantics_changed` flag; the next queued
notification ORs the bit (same pattern as overflow), then clears. Items without percent deadband
on ArrayItem-class nodes still signal per Part 8 where the property is one of the listed set —
scope here is EURange (the register's scope); the mechanism is built so other Part-8 properties
can join later. EURange deleted → filter fails safe (keep last-known range; item keeps
functioning). **Alternatives considered**: re-reading EURange on every sample (rejected — the
ponytail note's original reason stands: a node read per sample on the hot path; event-driven
refresh is O(changes) not O(samples)).

## R6 — P3-09: AccessLevelEx (US6)

**Spec ground** (Part 3 §5.6.2): optional Variable attribute, `AccessLevelExType` (UInt32); low
byte mirrors AccessLevel; extended bits include NonatomicRead/NonatomicWrite/WriteFullArrayOnly/
NoSubDataTypes etc. Also Part 3 §8.60 WriteMask bit 25 (already mapped: `utils.rs:107`).

**Codebase state**: `AttributeId::AccessLevelEx = 27` exists (`types/src/attribute.rs:84`),
`AccessLevelExType` bitflags exist (`generated/types/enums.rs:15`). `Variable`
(`async-opcua-nodes/src/variable.rs:146`) has NO `access_level_ex` field, no read arm
(`get_attribute_max_age` :177–212 falls through → `BadAttributeIdInvalid` via `utils.rs:598–600`),
no set arm. Special-case handlers exist in `diagnostics/node_manager.rs:548` and the tags sample.
Write-side type validation already accepts UInt32 (`utils.rs:313`).

**Decision**: add `access_level_ex` to the Variable node (builder + getter/setter + read/set
attribute arms), **derived default** = `access_level` low byte (keeps every existing Variable
consistent without nodeset regeneration); include in attribute read dispatch so Read returns
UInt32. Non-Variable node classes keep `BadAttributeIdInvalid`. **Alternatives considered**:
storing a full u32 with independent low byte (rejected — low byte MUST mirror AccessLevel per
Part 3; derive it so they cannot diverge; store only the extended bits).

## R7 — P5-03: NamespaceMetadata NodeClass verify-and-close (US7)

**Spec ground** (Part 5 §6.3.13 Table 22, §6.3.14): NamespaceMetadataType instances are Objects;
their members (NamespaceUri, NamespaceVersion, NamespacePublicationDate, IsNamespaceSubset,
StaticNodeIdTypes, StaticNumericNodeIdRange, StaticStringNodeIdPattern, Default*Permissions…) are
Properties → NodeClass Variable.

**Codebase state — verification result: code is CORRECT, finding is inverted as suspected**:
`diagnostics/node_manager.rs:178` sets `node_class: NodeClass::Object` on the metadata node and
:418 serves NodeClass=Object on read; property children enumerated at :263–289 are served as
Variables (:197, :463) with PropertyType type definition; values at :441. Matches Part 5 exactly.

**Decision**: close as **not-a-bug** with a lock-in test (browse `Server → Namespaces → <ns>`
asserting Object NodeClass + Variable property children; natural home `integration/browse.rs`,
near `browse_multiple` :115) and update the FINDINGS.md row. No code change expected.

---

## Cross-cutting decisions

- **Register updates**: each story's commit updates its FINDINGS.md row (status, spec §, named
  tests) — FR-011; the reconciliation banner table gets the same statuses.
- **Test placement** (mapped): US1 → `integration/read.rs:1285 test_diagnostics` +
  `integration/browse.rs`; US2/US3/US6 → `integration/write.rs` + `integration/read.rs` + unit
  tests in `utils.rs:708 mod tests`; US4 → `SimpleNodeManager` callback tests + integration read;
  US5 → `monitored_item.rs mod tests :1036` + `integration/subscriptions.rs`
  (`test_data_change_filters` :780, `modify_…_with_eurange_succeeds` :1051 are templates);
  US7 → `integration/browse.rs`.
- **Workflow**: codex implements one task at a time (constitution Principle III; memory:
  one-task-per-dispatch, no self-authored tests); Claude writes independent spec-grounded tests;
  every task cites its Part/§ (memory: speckit-tasks-cite-spec-sections). Commit per user story.
- **CI**: run the full server-crate test binaries (not just --lib + integration) — feature 018/
  fork-CI lessons; clippy `--all-targets --all-features` plus the json-off/no-default legs before
  push.
