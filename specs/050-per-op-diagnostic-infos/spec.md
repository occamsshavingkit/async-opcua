# Feature Specification: Per-Operation diagnosticInfos Completion (P4-GEN-01)

**Feature Branch**: `050-per-op-diagnostic-infos`
**Created**: 2026-07-01
**Status**: Draft
**Input**: Complete P4-GEN-01 — honor `returnDiagnostics` per-operation `diagnosticInfos` for the server
services that still omit them, so `returnDiagnostics` is honored uniformly across all services
(Part 4 §5.2 / §5.3).

## Context

OPC UA Part 4 §5.2 (Common request parameters — `RequestHeader.returnDiagnostics`) and §5.3 (Common
response parameters) let a client ask the server to return diagnostic detail. When the client sets bits in
`returnDiagnostics`, the server SHOULD return, for each requested facet, a per-operation `DiagnosticInfo` at
the same array position as the corresponding result, plus consistent `ResponseHeader.stringTable` entries for
any symbolic identifiers referenced.

The server already honors this for the `ResponseHeader` (serviceDiagnostics + stringTable) and for
per-operation `diagnosticInfos` on **Read**, **Call**, **Write**, and **NodeManagement**, via an established
internal mechanism: each per-operation work item is constructed with the request's diagnostic-bits, carries a
slot for its `DiagnosticInfo`, and a shared "consume results" step emits the `(results, diagnosticInfos)` pair
gated on the requested bits.

**What the precedent actually does (verified 2026-07-01):** the shared step returns a per-operation
`diagnosticInfos` array that is *positionally aligned* with the results (same length and order) **when the
client requests per-operation diagnostics**, and returns *no* array (`None`) when it does not. The
`DiagnosticInfo` *content* is a node-manager extension point — a hook exists for a node manager to attach
detail per operation, but the built-in node managers leave it at its default, so the aligned array is present
(gated on the requested bits) though its entries may be empty. This structural, bits-gated presence — not rich
content — is the conformance behavior (Part 4 §5.2/§5.3: the per-op list matches the size/order of the results
and is empty when diagnostics were not requested; content is returned "if available").

The remaining services still hardcode an absent per-operation `diagnosticInfos` (always `None`) regardless of
what the client requested: **Browse / BrowseNext**, **HistoryRead / HistoryUpdate**, the **MonitoredItems**
service group (Create / Modify / Delete MonitoredItems, and Set Monitoring Mode / Set Triggering), and
**Query (QueryFirst / QueryNext)**. This feature closes that gap by applying the *same* existing mechanism to
those services, so `returnDiagnostics` is honored uniformly: the aligned per-op array is present when requested
and absent when not. It matches the Read/Call/Write/NodeManagement precedent exactly — it does **not** add
richer `DiagnosticInfo` content (that remains the node-manager extension point, out of scope). It is purely
additive: results, status codes, ordering, and array sizes are unchanged.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Browse and BrowseNext honor returnDiagnostics (Priority: P1)

As an OPC UA client operator diagnosing why specific Browse operations fail, when I set `returnDiagnostics`
to request per-operation diagnostics, the server returns a `DiagnosticInfo` for each failed Browse/BrowseNext
operation at its matching result index, so I can see the symbolic reason without guessing.

**Why this priority**: Browse is one of the most-used services and the most common place a client needs
per-operation failure detail (bad node id, bad reference type, bad continuation point). Highest diagnostic
value of the four groups.

**Independent Test**: Issue a Browse with multiple operations and `returnDiagnostics` requesting per-operation
diagnostics; confirm the response carries a per-operation `diagnosticInfos` array aligned with results, that
the same request with `returnDiagnostics = 0` yields no such array, and that BrowseNext behaves the same.

**Acceptance Scenarios**:

1. **Given** a Browse request with multiple operations and `returnDiagnostics` requesting per-operation
   diagnostics, **When** the server responds, **Then** the response carries a per-operation `diagnosticInfos`
   array the same length and order as `results`.
2. **Given** the same request with `returnDiagnostics = 0`, **When** the server responds, **Then** no
   per-operation `diagnosticInfos` array is returned (behavior identical to today).
3. **Given** a BrowseNext request with diagnostics requested, **When** the server responds, **Then** the
   response carries a per-operation `diagnosticInfos` array aligned with its results.

---

### User Story 2 - MonitoredItems services honor returnDiagnostics (Priority: P2)

As a client managing subscriptions, when I request per-operation diagnostics on a MonitoredItems service call
(Create / Modify / Delete MonitoredItems, Set Monitoring Mode, Set Triggering), the server returns a
`DiagnosticInfo` for each failed per-item operation at its matching index.

**Why this priority**: Subscription setup is failure-prone (bad node, bad filter, bad queue params) and
per-item diagnostics materially help clients. Second-highest value.

**Independent Test**: Create/modify monitored items with diagnostics requested; confirm the response carries a
per-item `diagnosticInfos` array aligned with results, and that with `returnDiagnostics = 0` nothing changes
from today.

**Acceptance Scenarios**:

1. **Given** a CreateMonitoredItems request with multiple items and per-operation diagnostics requested,
   **When** the server responds, **Then** the response carries a per-item `diagnosticInfos` array the same
   length and order as `results`.
2. **Given** the same request with `returnDiagnostics = 0`, **When** the server responds, **Then** no
   per-operation `diagnosticInfos` array is returned.
3. **Given** ModifyMonitoredItems, DeleteMonitoredItems, SetMonitoringMode, and SetTriggering requests with
   diagnostics requested, **When** the server responds, **Then** each returns a per-operation `diagnosticInfos`
   array aligned with its results.

---

### User Story 3 - HistoryRead and HistoryUpdate honor returnDiagnostics (Priority: P3)

As a client performing historical data access, when I request per-operation diagnostics, the server returns a
`DiagnosticInfo` for each failed HistoryRead / HistoryUpdate operation at its matching index.

**Why this priority**: Historizing is a narrower audience than Browse/subscriptions but still a mandated
common-parameter behavior; grouped as P3 for value ordering.

**Independent Test**: Issue a HistoryRead (and HistoryUpdate) with multiple per-node operations and
diagnostics requested; confirm a per-operation `diagnosticInfos` array aligned with results, and no such array
when `returnDiagnostics = 0`.

**Acceptance Scenarios**:

1. **Given** a HistoryRead with multiple node operations and per-operation diagnostics requested, **When**
   the server responds, **Then** the response carries a per-operation `diagnosticInfos` array the same length
   and order as `results`.
2. **Given** a HistoryUpdate with multiple operations and diagnostics requested, **When** the server responds,
   **Then** the response carries a per-operation `diagnosticInfos` array aligned with its results.
3. **Given** either request with `returnDiagnostics = 0`, **When** the server responds, **Then** no
   per-operation `diagnosticInfos` array is returned.

---

### User Story 4 - Query honors returnDiagnostics (Priority: P3)

As a client using the Query service, when I request per-operation diagnostics, the server returns diagnostic
detail for failing Query parsing/operation results at their matching positions, consistent with the existing
QueryFirst validation.

**Why this priority**: Query is the least-used of the four and its diagnostic surface is smaller; lowest value
but needed for uniform coverage.

**Independent Test**: Issue a QueryFirst with diagnostics requested; confirm the per-operation
`diagnosticInfos` array is present and aligned with the per-operation results, and nothing changes with
`returnDiagnostics = 0`.

**Acceptance Scenarios**:

1. **Given** a QueryFirst with per-operation results and per-operation diagnostics requested, **When** the
   server responds, **Then** the response carries a per-operation `diagnosticInfos` array aligned (length and
   order) with those results.
2. **Given** the same request with `returnDiagnostics = 0`, **When** the server responds, **Then** no
   per-operation `diagnosticInfos` array is returned.
3. **Given** QueryNext with diagnostics requested, **When** the server responds, **Then** the response carries
   a per-operation `diagnosticInfos` array aligned with its results.

### Edge Cases

- With `returnDiagnostics = 0` (the default), every affected service MUST behave byte-for-byte as today (no
  per-operation `diagnosticInfos` array).
- The per-operation `diagnosticInfos` array is returned based solely on the requested bits (via the shared
  `consume_results` step); a service never fabricates the array when diagnostics were not requested.
- The per-operation `diagnosticInfos` array, when present, MUST match the length and order of the results
  array for that service.
- `DiagnosticInfo` per-entry *content* stays a node-manager extension point (may be default/empty), exactly as
  for the existing Read/Call/Write/NodeManagement services; this feature does not populate content.
- Services must not change their result payloads, status codes, ordering, or array sizes.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Browse and BrowseNext MUST return a per-operation `diagnosticInfos` array positionally aligned
  with results (same length and order) when the client requests per-operation diagnostics via
  `returnDiagnostics`, and MUST return no such array when it does not — matching the Read/Call/Write precedent.
- **FR-002**: CreateMonitoredItems, ModifyMonitoredItems, DeleteMonitoredItems, SetMonitoringMode, and
  SetTriggering MUST return an aligned per-operation `diagnosticInfos` array when requested, and none when not.
- **FR-003**: HistoryRead and HistoryUpdate MUST return an aligned per-operation `diagnosticInfos` array when
  requested, and none when not.
- **FR-004**: Query (QueryFirst and QueryNext) MUST return per-operation diagnostic arrays positionally
  aligned with their per-operation results when requested, and none when not.
- **FR-005**: All affected services MUST reuse the existing diagnostic-bits + shared `consume_results`
  mechanism already used by Read/Call/Write/NodeManagement; no new, parallel diagnostics mechanism is
  introduced. Where a service's per-operation work item does not yet carry the diagnostic-bits + diagnostic-
  info slot and `IntoResult` impl, they are added following the existing `ReadNode`/`WriteValue` precedent.
- **FR-006**: When `returnDiagnostics` does not request per-operation diagnostics, every affected service MUST
  return no per-operation `diagnosticInfos` (byte-for-byte identical to current behavior).
- **FR-007**: The change MUST NOT alter results, status codes, ordering, or array sizes for any affected
  service; it is additive observability only.
- **FR-008**: When present, a service's per-operation `diagnosticInfos` array MUST be the same length and
  order as that service's results array. (`DiagnosticInfo` field-level filtering by the requested bits is
  handled by the existing `consume_results`/`filter_diagnostic_info` step and is reused unchanged.)
- **FR-009**: After this feature, no server service handler still hardcodes an always-absent per-operation
  `diagnosticInfos`; `returnDiagnostics` is honored uniformly across the full service surface.
- **FR-010**: Populating `DiagnosticInfo` *content* (symbolic id, localized text, etc.) remains the existing
  node-manager extension point and is NOT added here; like the current Read/Call/Write/NodeManagement
  services, entries may be default/empty. Only the aligned, bits-gated *presence* of the array is required.

### Key Entities *(include if feature involves data)*

- **Per-Operation diagnosticInfos array**: the optional array returned alongside a service's results,
  positionally aligned with them (same length and order), present only when the client requests per-operation
  diagnostics and absent otherwise. Per-entry `DiagnosticInfo` content is a node-manager extension point and
  may be default/empty.
- **Diagnostic Bits (returnDiagnostics)**: the client-supplied request flags selecting which diagnostic facets
  the server should return; already threaded to Read/Call/Write/NodeManagement work items.
- **Response String Table**: the `ResponseHeader.stringTable` holding symbolic identifiers referenced by
  diagnostics; already managed by the existing mechanism and must stay consistent.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For each affected service (Browse, BrowseNext, HistoryRead, HistoryUpdate, CreateMonitoredItems,
  ModifyMonitoredItems, DeleteMonitoredItems, SetMonitoringMode, SetTriggering, QueryFirst, QueryNext), a
  request with per-operation diagnostics requested returns a per-operation `diagnosticInfos` array whose length
  and order match that service's results — demonstrated by a test that fails before the change (array absent)
  and passes after (array present and aligned).
- **SC-002**: For each affected service, the same request with `returnDiagnostics = 0` returns no
  per-operation `diagnosticInfos` (no regression), demonstrated by test.
- **SC-003**: The array presence is driven solely by the requested bits via the shared `consume_results` step;
  no service fabricates an array when diagnostics were not requested.
- **SC-004**: No server service handler still hardcodes an always-absent per-operation `diagnosticInfos`
  (verified by grep against the previously-identified `n: None` / `diagnostic_infos: None` sites).
- **SC-005**: The full existing server test suite passes unchanged; no result/status/ordering/size change is
  observable for any service.

## Assumptions

- The existing diagnostic-bits + "consume results" mechanism used by Read/Call/Write/NodeManagement is the
  intended pattern and is reusable for the remaining services (verified 2026-07-01 against current code); this
  feature extends it rather than redesigning diagnostics.
- Per-operation `DiagnosticInfo` content is whatever the existing mechanism already produces for the done
  services — which today is default/empty unless a node manager attaches detail; this feature does not invent
  richer diagnostic content and only guarantees the aligned array's bits-gated presence.
- Single-server-per-process behavior is the norm; the change is behavior-preserving except for the additive
  per-operation diagnostics when explicitly requested.
- The `ResponseHeader.serviceDiagnostics` / `stringTable` handling is already complete and correct and is out
  of scope here; this feature only adds the per-operation arrays and relies on the existing string-table
  management.
- Part 4 §5.2/§5.3 treat the per-operation diagnostics list as matching the size/order of the results and
  empty when not requested; returning the aligned array gated on the requested bits (with content optional) is
  the accepted, precedent-matching interpretation.

## Out of Scope

- The `ResponseHeader.serviceDiagnostics` / `stringTable` mechanism (already implemented).
- The other general findings P4-GEN-02 and P4-GEN-03.
- Any new or richer `DiagnosticInfo` content beyond what the existing mechanism produces for the done services.
- Any change to results, status codes, service semantics, or wire encoding.
