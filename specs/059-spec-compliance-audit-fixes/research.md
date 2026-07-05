# Research: Spec Compliance Audit Fixes

**Feature**: 059-spec-compliance-audit-fixes
**Date**: 2026-07-05

## Status Summary

The 2026-07-05 compliance audit identified 23 findings. Codebase verification confirms **14 findings are already resolved** by prior work. **7 findings remain open** plus 1 partial (DISC-04). This research covers the approach for each remaining finding.

---

## SESSION-04 [MEDIUM]: serverNonce runtime validation

**Decision**: Upgrade `debug_assert!` (line 365-369) to a runtime check.

**Rationale**: The spec mandates serverNonce length be in [32,128] (OPC-10000-4 §5.7.2.2). The current `debug_assert!` only fires in debug builds. In release builds, a misconfigured server would generate noncompliant nonces silently. A runtime check with an error return (`BadConfigurationError`) ensures spec compliance in all build modes.

**Alternatives considered**:
- Keep `debug_assert!` and document config validation as the user's responsibility — rejected because the server should enforce spec compliance regardless of config
- Clamp serverNonce length to [32,128] silently — rejected because silent clamping hides the configuration error

**Implementation**: Add a runtime `if` check in `CreateSessionAllocation::new()` (manager.rs:365-369) that returns an error if `nonce_len` is outside [32,128].

---

## SESSION-06 [LOW]: revisedSessionTimeout minimum

**Decision**: Add a `min_session_timeout_ms` configuration field defaulting to 1ms, with a floor applied in the timeout calculation.

**Rationale**: OPC-10000-4 §5.7.2.2 Table 15 requires "The Server shall provide a timeout greater than 0." Currently, `max_session_timeout_ms.min(requested.floor() as u64)` can yield 0. Configuring a minimum of 1ms satisfies the spec.

**Alternatives considered**:
- Hardcode 1ms minimum — rejected because server operators may want higher minimums
- Clamp to max(1, result) inline — simpler but less configurable

**Implementation**: Add `min_session_timeout_ms: u64` to `ServerConfig` defaulting to 1. Apply `max(min_config, computed_timeout)` in `manager.rs` around line 378-381.

---

## VIEW-02 [MEDIUM]: BrowseDirection::INVALID rejection

**Decision**: Add BrowseDirection validation in the `browse()` service handler (`view.rs`) before building BrowseNodes.

**Rationale**: OPC-10000-4 §5.9.2.4 Table 36 and §7.5 Table 112 require BrowseDirection value 3 (Invalid) to be rejected with `BadBrowseDirectionInvalid`. Currently, the server silently returns empty results for invalid direction. Explicit validation provides proper error feedback to clients.

**Alternatives considered**:
- Validate in `BrowseNode::new()` — rejected because the error should be per-node, not at construction time
- Validate via the existing per-node loop — chosen because each BrowseDescription has its own direction

**Implementation**: Before building `BrowseNode`, check each item's `browse_direction`. If it's value 3, push an error entry into the results vector. The `BrowseDirection` enum has explicit discriminant values: `Forward=0, Inverse=1, Both=2, Invalid=3`.

---

## VIEW-03 [LOW]: External references result mask bypass

**Decision**: Apply result mask field-stripping logic within `add_unchecked()`.

**Rationale**: OPC-10000-4 §5.9.2.2 Table 34 states resultMask applies to **all** ReferenceDescriptions. The current `add_unchecked()` method bypasses result mask filtering. Two other callers (`diagnostics/node_manager.rs:665`, `memory/mod.rs:963`) also use `add_unchecked()` for pre-filtered references. Adding field-stripping to `add_unchecked()` fixes all call sites at once.

**Alternatives considered**:
- Change `resolve_external_references` to call `add()` instead — rejected because it would double-call `matches_filter()` and force disambiguation via parameter
- Create a new method `add_no_filter()` — rejected because it adds API surface without benefit
- Strip fields in the external reference loop inline — rejected as code duplication

**Implementation**: Extract the field-stripping logic from `add()` (view.rs:412-452) into a private helper method `strip_by_result_mask(&mut self, reference: &mut ReferenceDescription)`. Call it from both `add()` and `add_unchecked()`.

---

## SC-03 [LOW]: Deferred resource cleanup on CloseSecureChannel

**Decision**: Keep the existing pattern. Document as accepted behavior.

**Rationale**: OPC-10000-6 §7.1.4 requires "release all resources allocated for the channel." The current implementation returns `RequestProcessResult::Close` which triggers `set_closing()`. Session cleanup occurs through normal timeout expiration, which is the established pattern in this codebase. Forcing synchronous cleanup would require significant refactoring of the session/channel lifecycle without practical benefit — keys/nonces are zeroized on drop, and sessions time out independently.

**Alternatives considered**:
- Iterate and close all associated sessions synchronously — rejected as a major refactoring that could introduce bugs without security benefit

**Implementation**: Document in code comments that resource release follows the async drop pattern. No code change needed.

---

## DISC-03 [LOW]: FindServers endpoint_url filtering for registered servers

**Decision**: Add endpoint URL filtering for registered servers, matching the own-server behavior.

**Rationale**: OPC-10000-12 §5.1 indicates LDS should filter by endpoint URL. Currently `registered_application_descriptions` ignores the `_endpoint_url` parameter entirely. The own-server path already filters via `matches_discovery_endpoint_url`. Extending this filtering to registered servers provides consistent behavior.

**Alternatives considered**:
- Skip filtering for registered servers (current behavior) — rejected per spec requirement

**Implementation**: In `registered_application_descriptions()` (info.rs:651-669), add a filter that checks each registered server's `discovery_urls` against `endpoint_url`. If `endpoint_url` is non-empty, only include servers where at least one discovery URL matches the requested endpoint_url.

---

## DISC-04 [LOW]: Own server application_name locale filtering

**Decision**: Apply the same locale-aware name selection used for registered servers to the own server.

**Rationale**: OPC-10000-4 §7.2.4 requires `application_name` to support locale selection. Registered servers already use `registered_server_application_name()` (info.rs:1395-1418) for locale-aware name selection. The own server path in `find_servers_application_description()` (info.rs:672-681) simply returns configured `application_description()` without locale filtering.

**Alternatives considered**:
- Change the own-server ApplicationName in config to support multiple locales — rejected as too invasive for a LOW finding

**Implementation**: If the own server's `application_description().application_name` has multiple locale-tagged texts, filter to the requested locale(s) using the existing `locale_id_matches` helper from `registered_server_application_name`. Apply this in `find_servers_application_description` by accepting `locale_ids` parameter.

---

## SC-04 [LOW]: Redundant `set_role()` call

**Decision**: Remove the redundant `set_role(Role::Server)` call at controller.rs:1181.

**Rationale**: The channel is already initialized with `Role::Server` at construction (controller.rs:200). No code path changes the role between construction and the OpenSecureChannel handler. The call is harmless code duplication.

**Implementation**: Delete line 1181 (`self.channel.set_role(Role::Server);`) from controller.rs.

---

## Resolved Findings (Verification Confirmed)

| Finding | Status | Resolution |
|---------|--------|------------|
| SESSION-01 | Fixed | Empty sessionName → "UnnamedSession" (manager.rs:386-393) |
| SESSION-02 | Fixed | Oldest unactivated session evicted (manager.rs:736-769) |
| SESSION-03 | Fixed | clientNonce validated against [max(config_len,32), 128] (manager.rs:492-506) |
| SESSION-05 | Fixed | Non-null authenticationToken rejected (manager.rs:486-489) |
| SESSION-07 | Fixed | X509 userTokenSignature validated (manager.rs:1109-1114) |
| SESSION-08 | Fixed | Null localeIds preserves existing (manager.rs:1297-1306) |
| VIEW-01 | Fixed | IS_FORWARD applied in result mask (view.rs:412-417) |
| SC-01 | Fixed | dispatch_close_secure_channel audit (controller.rs:468-474) |
| SC-02 | Fixed | token_created_at updated (controller.rs:1175) |
| DISC-01 | Fixed | `>` instead of `>=` (info.rs:527) |
| DISC-02 | Fixed | ECC policies get nonzero security_level (endpoint.rs:104-105) |
| SUB-01 | Fixed | Identical Duration::from_micros in both paths |
| AS-01 | Fixed | set_browse_name is pub(crate) (base.rs:267) |

## Out of Scope

| Finding | Reason |
|---------|--------|
| CANCEL-01 | Cancel stub is spec-compliant no-op; intentional |
| SUB-02 | Documented intentional deviation for correctness |
| DISC-05 | ECC asymmetric encryption: known limitation, requires new crate work |
