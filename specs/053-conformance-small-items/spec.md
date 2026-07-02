# Feature Specification: Conformance Small-Items Sprint

**Feature Branch**: `053-conformance-small-items-sprint`
**Created**: 2026-07-02
**Status**: Draft
**Input**: User description: "Conformance small-items sprint: close all remaining small open findings from the conformance audit register (specs/conformance-audit/FINDINGS.md) in one feature. Scope: P5-04 ServerDiagnosticsType mandatory children; P3-09 AccessLevelEx; P8-02 EURange dynamic refresh + SemanticsChanged; P4-ATTR-02 maxAge; P4-ATTR-03 LocalizedText write locale; P4-ATTR-04 write range/enum validation; P5-03 verify-and-close. One user story per finding; independent spec-grounded tests; update FINDINGS.md as items close."

## Overview

The 2026-07-01 reconciliation of the conformance audit register left a short tail of small,
independent findings open. This sprint closes every one of them, leaving the register with **zero
open rows** (only explicit not-a-bug rulings and out-of-scope infrastructure items). Each finding is
one prioritized, independently testable user story. Every story cites the OPC UA Part and section
that defines the required behavior so implementation and tests can be grounded against the
specification text (via the opc-ua-reference MCP), and every closed story updates its row in
`specs/conformance-audit/FINDINGS.md` with evidence.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Server diagnostics object is spec-complete (P5-04) (Priority: P1)

An operator or monitoring client browses the server's standard diagnostics object
(`Server.ServerDiagnostics`) and finds the children that OPC UA Part 5 §6.3.3 declares **mandatory**
for `ServerDiagnosticsType`: `EnabledFlag`, `SubscriptionDiagnosticsArray`, and
`SessionsDiagnosticsSummary` (with its `SessionDiagnosticsArray` and
`SessionSecurityDiagnosticsArray`). Today these nodes are absent, so a conformant client (or a
conformance test tool) that relies on the standard information model fails to locate them.
Reading them returns live diagnostics data; writing `EnabledFlag = false` disables diagnostics
collection and the arrays read as empty/unavailable per Part 5 semantics.

**Why this priority**: This is the largest remaining conformance gap — mandatory standard-model
nodes are missing entirely, which is immediately visible to any client that walks the Server
object and to any profile-based conformance test.

**Independent Test**: Connect a client to the demo/test server, browse
`Server → ServerDiagnostics`, and verify the mandatory children exist with the correct NodeIds,
NodeClasses, and data types; read them and verify plausible live values; toggle `EnabledFlag`
(where permitted) and verify the documented effect.

**Acceptance Scenarios**:

1. **Given** a running server with default configuration, **When** a client browses
   `Server.ServerDiagnostics`, **Then** `EnabledFlag`, `SubscriptionDiagnosticsArray`, and
   `SessionsDiagnosticsSummary` (with both child arrays) are present with the standard NodeIds and
   type definitions from Part 5 §6.3.3.
2. **Given** diagnostics are enabled and at least one session with one subscription exists,
   **When** a client reads `SubscriptionDiagnosticsArray` and
   `SessionsDiagnosticsSummary.SessionDiagnosticsArray`, **Then** each returns one entry per live
   subscription/session with counters consistent with actual server state.
3. **Given** `EnabledFlag` is written to `false` (by a client with sufficient rights or by server
   configuration), **When** a client reads the diagnostics arrays, **Then** each array reads as
   empty (diagnostics not collected, per Part 5) and reading `EnabledFlag` returns `false`.
4. **Given** an anonymous/low-privilege session, **When** it attempts to write `EnabledFlag`,
   **Then** the write is rejected with an access-denied status.

---

### User Story 2 - Writes validate range and enumeration values (P4-ATTR-04) (Priority: P2)

A client writing a Variable value that violates the Variable's modeled constraints — a value
outside the `EURange` of an AnalogItem, or an enumeration integer with no corresponding
enumeration value — receives `Bad_OutOfRange` per OPC UA Part 4 §5.11.4 and Part 8, instead of the
server silently storing an invalid value.

**Why this priority**: Silently accepting out-of-range/invalid-enum writes corrupts data that
other clients then trust; this is the highest-impact behavioral item in the tail.

**Independent Test**: Against a server exposing an AnalogItem with an `EURange` and a Variable of
an enumerated DataType, write in-range/valid values (accepted) and out-of-range/undefined-enum
values (rejected with `Bad_OutOfRange`), verifying the stored value is unchanged after a rejected
write.

**Acceptance Scenarios**:

1. **Given** an AnalogItem Variable with `EURange` [0, 100], **When** a client writes 150,
   **Then** the per-operation result is `Bad_OutOfRange` and a subsequent read returns the prior value.
2. **Given** the same Variable, **When** a client writes 99.5, **Then** the write succeeds.
3. **Given** a Variable whose DataType is an enumeration with values {0,1,2}, **When** a client
   writes 7, **Then** the result is `Bad_OutOfRange`; writing 2 succeeds.
4. **Given** a Variable with no modeled range/enumeration constraints, **When** any
   type-compatible value is written, **Then** behavior is unchanged from today (no new rejections).

---

### User Story 3 - LocalizedText writes follow locale rules (P4-ATTR-03) (Priority: P2)

A client writing a `LocalizedText` **attribute** (DisplayName, Description, InverseName)
experiences the locale semantics of OPC UA Part 4 §5.11.4.1: writing text for a locale updates
that locale's text without discarding other stored locales, writing a null text for a locale
deletes that locale entry, and writing with a locale the server does not support yields
`Bad_LocaleNotSupported`. For the **Value** attribute the spec leaves behavior server-specific;
this server keeps single-locale value semantics, locked in by test.

**Why this priority**: Multi-locale servers currently lose or mishandle per-locale text on write;
the failure is observable but narrower in blast radius than US2.

**Independent Test**: On a writable LocalizedText attribute (e.g. Description of a test node),
exercise add-locale, update-locale, delete-locale, unsupported-locale, and null-locale writes and
verify stored per-locale state after each step.

**Acceptance Scenarios**:

1. **Given** a node's Description holds text for locale "en", **When** a client writes Description
   text with locale "de" (a server-supported locale), **Then** both "en" and "de" texts are
   subsequently readable (each session reads its negotiated locale).
2. **Given** stored Description texts for "en" and "de", **When** a client writes a LocalizedText
   with locale "de" and null/empty text, **Then** the "de" entry is removed and "en" remains.
3. **Given** a server whose supported-locale set excludes "xx", **When** a client writes a
   LocalizedText attribute with locale "xx", **Then** the per-operation result is
   `Bad_LocaleNotSupported` and the stored texts are unchanged.
4. **Given** a write of a LocalizedText attribute with a null locale, **Then** the server applies
   the Part 4 default-locale rule (updates the invariant/default text) rather than rejecting.
5. **Given** a Variable whose Value is a LocalizedText, **When** clients write values with
   different locales, **Then** the server's documented single-locale value semantics hold (last
   write wins as a whole value) — the server-specific choice permitted by Part 4 §5.11.4.1,
   locked in by test.

---

### User Story 4 - Read honors maxAge semantics (P4-ATTR-02) (Priority: P3)

A client issuing a Read with a `maxAge` parameter gets the semantics of OPC UA Part 4 §5.11.2
(Table 47): `maxAge = 0` forces the server to obtain a fresh value from the underlying source
where one exists; `maxAge ≥` the maximum Int32 value permits any cached value; intermediate
values allow a cached value no older than `maxAge` milliseconds. For purely in-memory values
(which are always current) the observable effect is that source timestamps reflect the rule and
the parameter is never silently ignored for node managers with genuine sources (sampled/external).

**Why this priority**: Meaningful mainly for external/sampled sources; for the default in-memory
manager the change is mostly timestamp/plumbing semantics, so it ranks below the write-validation
stories.

**Independent Test**: Against a node whose value is produced by a sampled/callback source with a
known refresh cadence, issue Reads with `maxAge = 0`, a mid-range `maxAge`, and `maxAge = max`,
and verify freshness behavior (source re-invocation or cached return) matches the rule.

**Acceptance Scenarios**:

1. **Given** a node backed by a dynamic source last sampled T ms ago, **When** a client reads with
   `maxAge = 0`, **Then** the returned value is freshly obtained (source timestamp ≥ request time
   tolerance window, or the source callback is observably re-invoked).
2. **Given** the same node, **When** a client reads with `maxAge ≥ 2147483647`, **Then** the
   server may return the cached value unchanged (no forced refresh).
3. **Given** a cached value older than the requested mid-range `maxAge`, **When** read, **Then**
   the server refreshes before answering; a value younger than `maxAge` may be returned as-is.
4. **Given** an in-memory static node, **When** read with any `maxAge`, **Then** the read
   succeeds and results are self-consistent (no error, no stale-beyond-maxAge timestamp claims).

---

### User Story 5 - Monitored items track EURange changes (P8-02) (Priority: P3)

A client with a data-change monitored item using a percent deadband keeps getting correct
deadband behavior after the underlying AnalogItem's `EURange` property changes at runtime, per
OPC UA Part 8: the server re-reads the changed `EURange` (instead of using the value cached at
item creation) and flags affected notifications with the `SemanticsChanged` status bit so the
client knows the semantics of the value stream changed.

**Why this priority**: Real but narrow — only affects percent-deadband subscriptions whose
engineering ranges change at runtime; today's create-time caching is a documented deferral.

**Independent Test**: Create a percent-deadband monitored item on an AnalogItem, change the
node's EURange, then drive value changes that would pass/fail the deadband under old vs. new
range and verify filtering follows the **new** range and the first notification after the change
carries the `SemanticsChanged` bit.

**Acceptance Scenarios**:

1. **Given** a monitored item with PercentDeadband 10% on an AnalogItem with EURange [0,100],
   **When** EURange is changed to [0,1000] and the value moves by 50, **Then** the change (5% of
   the new range) is suppressed by the deadband — the filter uses the updated range.
2. **Given** the same setup, **When** EURange changes, **Then** the next data-change notification
   delivered for that item has the `SemanticsChanged` bit set in its StatusCode, and subsequent
   notifications do not.
3. **Given** a monitored item with no percent deadband, **When** EURange changes, **Then**
   monitoring behavior is unchanged apart from the `SemanticsChanged` signaling required by Part 8.

---

### User Story 6 - AccessLevelEx attribute is exposed (P3-09) (Priority: P4)

A client reading the optional `AccessLevelEx` attribute (OPC UA Part 3 §5.6.2, attribute id 27)
on a Variable receives a 32-bit value whose low byte is consistent with `AccessLevel` and whose
extended bits (e.g. nonatomic read/write, write-full-array-only) are modeled, instead of
`Bad_AttributeIdInvalid`. Servers can set extended access-level semantics on their Variables.

**Why this priority**: Optional attribute; absence is permitted for many profiles, so this is
completion/completeness value rather than a hard conformance failure.

**Independent Test**: Read `AccessLevelEx` on a standard Variable and on a Variable configured
with extended bits; verify the low byte mirrors `AccessLevel` and configured extended bits are
returned; verify the attribute participates in Read like any other attribute (indexRange/locale
rules not applicable, per-op status correct).

**Acceptance Scenarios**:

1. **Given** any Variable node, **When** a client reads attribute `AccessLevelEx`, **Then** it
   receives a UInt32 whose low 8 bits equal the node's `AccessLevel` value.
2. **Given** a server-side node configured with an extended access-level bit, **When**
   `AccessLevelEx` is read, **Then** the configured bit is set in the returned value.
3. **Given** a non-Variable node (e.g., an Object), **When** `AccessLevelEx` is read, **Then**
   the per-operation result is `Bad_AttributeIdInvalid` (unchanged behavior for node classes that
   lack the attribute).

---

### User Story 7 - P5-03 NamespaceMetadata finding is formally closed (Priority: P4)

A maintainer consulting the conformance register finds P5-03 (NamespaceMetadata NodeClass)
resolved with a definitive ruling. The finding is believed inverted (the code is correct:
`NamespaceMetadataType` instances are Objects with Property children as Variables, per Part 5
§6.3.13/§8.2). This story is **verify-before-fix**: re-verify the behavior against the
specification text; if the code is correct, close the register row as not-a-bug and add a lock-in
test that pins the correct NodeClass structure; if verification instead confirms a real defect,
fix it.

**Why this priority**: Housekeeping — no expected behavior change; value is register hygiene and
a regression guard.

**Independent Test**: A test asserts the NodeClass of the server's namespace-metadata object and
its children match the Part 5 model; the register row carries a final status with evidence.

**Acceptance Scenarios**:

1. **Given** the server address space, **When** the namespace-metadata structure is inspected,
   **Then** the node under `Server.Namespaces` is an Object of `NamespaceMetadataType` and its
   Property children (NamespaceUri, NamespaceVersion, etc.) are Variables, matching Part 5.
2. **Given** verification confirms the code is correct, **When** the sprint completes, **Then**
   `FINDINGS.md` marks P5-03 **not-a-bug** with spec citation and a named lock-in test.
3. **Given** verification instead finds a real divergence, **Then** the divergence is fixed and
   the row is marked FIXED with evidence.

---

### Edge Cases

- Diagnostics arrays while sessions/subscriptions are being created/closed concurrently — array
  reads must return a consistent snapshot, not crash or mix entries (US1).
- `EnabledFlag` toggled while subscriptions are live — collection stops/starts without corrupting
  counters (US1).
- Out-of-range write via an index range (writing one element of an analog array out of range)
  must also be rejected (US2).
- Enumeration validation must not reject writes to Variables whose DataType is an integer but not
  an enumeration (US2).
- LocalizedText write where the value is a whole-array or index-ranged write of LocalizedText
  elements — locale rules apply per element or the write is handled per existing array semantics
  without data loss (US3).
- `maxAge` negative or non-finite handling: Part 4 defines maxAge as a Duration; invalid values
  must not panic and follow a documented interpretation (US4).
- EURange deleted (property removed) after item creation — deadband filtering must fail safe
  (item keeps functioning; `Bad_FilterNotAllowed`-class behavior only where spec requires) (US5).
- `SemanticsChanged` bit must not leak onto unrelated monitored items on the same node (US5).
- `AccessLevelEx` on Variables created before the attribute existed (default value derivation
  from `AccessLevel`) (US6).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST expose `EnabledFlag`, `SubscriptionDiagnosticsArray`, and
  `SessionsDiagnosticsSummary` (including `SessionDiagnosticsArray` and
  `SessionSecurityDiagnosticsArray`) under `Server.ServerDiagnostics` with the standard NodeIds,
  NodeClasses, and DataTypes mandated by OPC UA Part 5 §6.3.3. [P5-04]
- **FR-002**: Diagnostics values read from FR-001 nodes MUST reflect live server state (one array
  entry per live subscription/session) while `EnabledFlag` is true, and follow Part 5 semantics
  when false. [P5-04]
- **FR-003**: `EnabledFlag` write access MUST be restricted (rejected for unprivileged sessions),
  and `SessionSecurityDiagnosticsArray` MUST be readable only by administrative sessions (it
  exposes per-session security parameters). [P5-04]
- **FR-004**: Write operations MUST return `Bad_OutOfRange` per OPC UA Part 4 §5.11.4 when the
  written value violates the Variable's modeled EURange or enumeration value set, leaving the
  stored value unchanged; Variables without such constraints are unaffected. [P4-ATTR-04]
- **FR-005**: Write operations on LocalizedText values MUST implement the Part 4 §5.11.4 locale
  rules: per-locale update, null-text deletion, default-locale handling, and
  `Bad_LocaleNotSupported` for locales the server does not support. [P4-ATTR-03]
- **FR-006**: Read operations MUST honor `maxAge` per OPC UA Part 4 §5.11.2: 0 forces a fresh
  source read where a refreshable source exists, values ≥ max Int32 permit cached values, and
  intermediate values bound acceptable staleness; the parameter MUST NOT be silently ignored for
  node managers with refreshable sources. [P4-ATTR-02]
- **FR-007**: Percent-deadband monitored-item filtering MUST use the current `EURange` of the
  monitored node, re-evaluated when the property changes after item creation, per OPC UA Part 8. [P8-02]
- **FR-008**: When a monitored node's `EURange` (value semantics) changes, the next notification
  for affected monitored items MUST carry the `SemanticsChanged` StatusCode bit, exactly once per
  change, per OPC UA Part 4/8. [P8-02]
- **FR-009**: Variables MUST support the optional `AccessLevelEx` attribute (Part 3 §5.6.2): the
  low byte mirrors `AccessLevel`, extended bits are configurable by server code, and Read returns
  it like any standard attribute. [P3-09]
- **FR-010**: The P5-03 NamespaceMetadata NodeClass finding MUST be re-verified against Part 5;
  the outcome (not-a-bug or fix) MUST be recorded in the register with a lock-in test either way. [P5-03]
- **FR-011**: Every closed item MUST update its row in `specs/conformance-audit/FINDINGS.md`
  (status, evidence: file/test names, spec citation), leaving the register with no open rows in
  this sprint's scope.
- **FR-012**: All new behavior MUST be covered by independent tests grounded in the cited OPC UA
  spec sections (spec text consulted via the opc-ua-reference MCP), authored separately from the
  implementation.

### Key Entities

- **ServerDiagnostics object**: the standard Part 5 diagnostics summary under the Server object;
  owns EnabledFlag and the diagnostics arrays; sourced from live session/subscription state.
- **SubscriptionDiagnosticsDataType / SessionDiagnosticsDataType /
  SessionSecurityDiagnosticsDataType entries**: per-subscription/per-session structures whose
  fields are defined by Part 5.
- **EURange / enumeration constraint**: modeled value-domain restrictions on a Variable that
  gate writes (US2) and parameterize percent-deadband filtering (US5).
- **LocalizedText store**: per-locale text set held by a Variable value, mutated per Part 4
  write locale rules.
- **Conformance register row**: a finding in `specs/conformance-audit/FINDINGS.md` carrying
  status + evidence; the sprint's unit of "done".

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After the sprint, `specs/conformance-audit/FINDINGS.md` contains zero rows in
  status `OPEN`/`PARTIAL`/`deferred` for P5-04, P3-09, P8-02, P4-ATTR-02, P4-ATTR-03,
  P4-ATTR-04, and P5-03 — each is FIXED or not-a-bug with named test evidence.
- **SC-002**: A client browsing the Server object finds 100% of the Part 5 §6.3.3 mandatory
  ServerDiagnosticsType children present and readable with live data.
- **SC-003**: 100% of out-of-range and undefined-enumeration writes against constrained test
  Variables are rejected with `Bad_OutOfRange` and cause no stored-value change; 0 regressions on
  unconstrained Variables (existing write test suite stays green).
- **SC-004**: Locale write matrix (add/update/delete/unsupported/null-locale) passes for all five
  cases on a multi-locale test Variable.
- **SC-005**: In the EURange-change scenario, deadband filtering follows the new range in 100% of
  driven value changes and exactly one notification per change carries `SemanticsChanged`.
- **SC-006**: The full existing test suite (unit + integration + interop smoke) passes unchanged
  apart from tests deliberately updated by this sprint.
- **SC-007**: Each user story lands as its own reviewable commit on the feature branch (7
  stories → ≥7 commits), enabling independent revert of any single finding's closure.

## Assumptions

- The sprint targets the built-in/default server node managers (in-memory core + sampled
  sources); external custom node managers get the capability hooks but their own policies are out
  of scope.
- Part 5 diagnostics scope is the **mandatory** ServerDiagnosticsType members named in the
  register row; optional diagnostics extensions (e.g., per-session per-request rate metrics
  beyond the standard structures) are out of scope.
- For US4 (maxAge), the in-memory node manager holds always-current values; the observable
  contract there is "no error, self-consistent timestamps", while genuine refresh semantics apply
  to sources that can be re-sampled. This scoping follows the register's own "to the extent
  meaningful" note.
- `SemanticsChanged` scope is EURange (and analogous property) changes affecting deadband
  semantics per Part 8; general model-change events are already covered by existing
  GeneralModelChangeEvent support and are not re-scoped here.
- Enumeration validation (US2) applies where the DataType resolves to an enumeration with a known
  value set in the type tree; custom/opaque enumerations without modeled values are not validated.
- Per Part 4 §5.11.4.1, the LocalizedText locale rules (US3) are mandatory for **non-Value**
  LocalizedText attributes (DisplayName/Description/InverseName); for the Value attribute the
  behavior is explicitly server-specific with a recommendation to follow the same rules. US3's
  mandatory surface is the non-Value attributes; Value-attribute conformance to the recommended
  rules is adopted only where it does not conflict with existing value semantics, and the chosen
  behavior is documented.
- The register update (FR-011) happens per story, in the same commit as the story's code+tests,
  consistent with the one-commit-per-story cadence.
- No API-breaking changes to public server-builder interfaces; new capabilities are additive
  (new attribute support, new validation on writes mandated by spec).
