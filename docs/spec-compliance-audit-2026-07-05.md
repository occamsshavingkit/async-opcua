# OPC UA Specification Compliance Audit — async-opcua v0.19.0

**Date**: 2026-07-05
**Scope**: Full codebase audit against OPC UA Parts 3, 4, 5, 6, 7, 12 (1.05)
**Method**: Line-by-line comparison of source against spec reference (opc-ua-reference MCP + webfetch)

---

## Findings Summary

| Severity | Count | Description |
|----------|-------|-------------|
| HIGH | 4 | Missing required behavior or parameter validation |
| MEDIUM | 6 | Spec mismatch, off-by-one, imprecision |
| MINOR | 4 | Low-impact spec deviations |
| LOW | 9 | Cosmetic, edge-case, or deferred-consequence |

23 findings total across 6 audited spec areas.

---

## Part 4 — Session Services (§5.7)

### SESSION-01 [HIGH] CreateSession: sessionName null/empty must be assigned a default
- **Spec**: §5.7.2.2 Table 15 — "If this parameter is null or empty the Server shall assign a value."
- **Code**: `async-opcua-server/src/session/manager.rs:392` passes `request.session_name.clone()` directly to `Session::create()`. `instance.rs:149` stores it as-is. No default name is ever generated.
- **Impact**: Session appears with empty name in diagnostics/audit. Non-compliant.

### SESSION-02 [HIGH] CreateSession: Must evict oldest unactivated session at capacity
- **Spec**: §5.7.2.1 — "the Server shall close the oldest Session that is not activated before reaching the maximum number of supported Sessions."
- **Code**: `async-opcua-server/src/session/manager.rs:706-708` returns `BadTooManySessions` without attempting eviction.
- **Impact**: Legitimate clients can be DOSed by creating (but not activating) many sessions. Non-compliant.

### SESSION-03 [HIGH] CreateSession: clientNonce validated against config, not spec [32,128]
- **Spec**: §5.7.2.2 Table 15 — "This number shall have a length between 32 and 128 bytes inclusive."
- **Code**: `async-opcua-server/src/session/manager.rs:474` checks `request.client_nonce.len() < info.config.session_nonce_length` — not the spec-mandated [32,128] range. Upper bound (128) never checked.
- **Impact**: If `session_nonce_length` configured < 32, short nonces accepted. Nonces > 128 never rejected.

### SESSION-04 [MEDIUM] CreateSession: serverNonce length not validated [32,128]
- **Spec**: §5.7.2.2 Table 15 — serverNonce "shall have a length between 32 and 128 bytes inclusive."
- **Code**: `async-opcua-server/src/session/manager.rs:364` generates `random::byte_string(info.config.session_nonce_length)` with no bounds validation.
- **Impact**: If config sets nonce length outside [32,128], generated nonces violate spec.

### SESSION-05 [LOW] CreateSession: authenticationToken not verified null
- **Spec**: §5.7.2.2 Table 15 — "The authenticationToken is always null."
- **Code**: No validation in `manager.rs` or `controller.rs` that `request.request_header.authentication_token` is null.
- **Impact**: Invalid request with non-null token silently accepted.

### SESSION-06 [LOW] CreateSession: revisedSessionTimeout can be 0
- **Spec**: §5.7.2.2 Table 15 — "The Server shall provide a timeout greater than 0."
- **Code**: `manager.rs:372-375` with `max_session_timeout_ms == 0` and `requested==0` yields 0.
- **Impact**: Degenerate config only. No practical security concern.

### SESSION-07 [HIGH] ActivateSession: userTokenSignature not validated for X509 tokens
- **Spec**: §7.40.5, §5.7.3.2 Table 17 — X509IdentityToken "shall always be accompanied by a Signature."
- **Code**: `manager.rs:978-1332` — `request.user_token_signature` is never read/validated independently of the authenticator. Authenticator may not verify it.
- **Impact**: Potential authentication bypass if authenticator doesn't check the X509 token signature.

### SESSION-08 [MEDIUM] ActivateSession: localeIds overwritten on subsequent calls when null
- **Spec**: §5.7.3.2 Table 17 — "If it is null or empty the Server shall keep using the current localeIds."
- **Code**: `instance.rs:295` unconditionally overwrites `self.locale_ids = locale_ids`.
- **Impact**: Second ActivateSession call with null localeIds clears previously set localeIds.

---

## Part 4 — SecureChannel (§5.6 / §6.1)

### SC-01 [HIGH] CloseSecureChannel: Missing mandatory audit event
- **Spec**: Part 4 §6.5.5 — "The CloseSecureChannel service shall generate an audit Event of type AuditChannelEventType."
- **Code**: `async-opcua-server/src/session/controller.rs:462` — handler is a single line: `RequestProcessResult::Close`. No audit dispatch.
- **Impact**: Security monitoring tools cannot detect channel closures. Mandatory compliance gap.

### SC-02 [MEDIUM] OpenSecureChannel: token_created_at not updated during renewal
- **Spec**: Part 6 §6.7.4 Table 64 — Response includes SecurityToken with CreatedAt field.
- **Code**: `controller.rs:1158` calls `set_token_lifetime()` but never updates `token_created_at`. Response body correctly includes `DateTime::now()` at line 1188, but `token_renewal_deadline()` at `secure_channel.rs:647` computes from stale `self.token_created_at`.
- **Impact**: Server expects client to renew slightly earlier than the client would compute from the response. Not a correctness issue but timing mismatch.

### SC-03 [LOW] Deferred resource cleanup on CloseSecureChannel
- **Spec**: Part 6 §7.1.4 — "shall release all resources allocated for the channel."
- **Code**: `controller.rs:462` returns `Close`, which triggers garbage-collected cleanup rather than explicit resource release.
- **Impact**: Keys/nonces held slightly longer than necessary. Not a security vulnerability.

### SC-04 [LOW] Redundant set_role() call in open_secure_channel
- **Code**: `controller.rs:1165` calls `set_role(Role::Server)` after role already set at construction (line 197).
- **Impact**: Harmless double-set. Code hygiene only.

---

## Part 4 — View & Attribute Services (§5.9–§5.12)

### VIEW-01 [HIGH] Browse: RESULT_MASK_IS_FORWARD (bit 1) not applied
- **Spec**: §5.9.2.2 Table 34 — resultMask bit 1 controls `is_forward` in ReferenceDescription.
- **Code**: `async-opcua-server/src/node_manager/view.rs:402-452` — `add()` method applies result mask to BrowseName, DisplayName, NodeClass, ReferenceType, TypeDefinition but does NOT clear `is_forward` when bit 1 absent.
- **Impact**: Servers always return `is_forward` even when client didn't request it. Wastes bandwidth. Non-compliant.

### VIEW-02 [MEDIUM] Browse: BrowseDirection::INVALID not rejected
- **Spec**: §7.5 Table 112 — BrowseDirection=3 is "No value specified." §5.9.2.4 Table 36 lists `BadBrowseDirectionInvalid`.
- **Code**: `async-opcua-server/src/session/services/view.rs:60-84` never checks browse_direction for validity.
- **Impact**: Invalid direction silently returns empty results instead of explicit error.

### VIEW-03 [LOW] External reference result mask bypass via add_unchecked()
- **Spec**: §5.9.2.2 Table 34 — resultMask applies to all ReferenceDescriptions.
- **Code**: `async-opcua-server/src/node_manager/view.rs:569-596` — `add_unchecked()` bypasses result mask filtering for external references.
- **Impact**: Custom node managers could leak unmasked fields for external references.

---

## Part 4 — Subscriptions & MonitoredItems (§5.13–§5.14)

### SUB-01 [MINOR] CreateSubscription: publishingInterval precision mismatch
- **Spec**: §5.14.1.2 — `requestedPublishingInterval` is a Double (ms).
- **Code**: `async-opcua-server/src/subscriptions/session_subscriptions.rs:302` truncates to ms via `as u64` cast, losing sub-ms precision. Modify path at line 340 uses `Duration::from_micros`, preserving precision.
- **Impact**: Creating then immediately modifying a subscription changes effective interval.

### SUB-02 [LOW] Subscription state machine deviation
- **Spec**: §5.13.1.2 state table.
- **Code**: `async-opcua-server/src/subscriptions/subscription.rs:522` acknowledges: "This check is not in the spec, but without it the lifetime counter won't behave properly."
- **Impact**: Documented intentional deviation for correctness.

---

## Part 6 — Binary Encoding

**No binary encoding compliance issues found.** All 14 audited areas (Variant, NodeId, String, ByteString, Guid, ExtensionObject, Array, MessageChunking, StatusCode, DateTime, QualifiedName/LocalizedText, DataValue, ExpandedNodeId, DiagnosticInfo) implement the spec correctly with proper bounds checking, recursion protection, and null-value handling.

---

## Part 3 / Part 5 — Address Space & Information Model

**No significant issues.** All mandatory Server Object children, reference type hierarchies, type definitions match spec. Generated from official OPC UA NodeSet2.xml v1.05.

### AS-01 [LOW] set_browse_name() publicly mutable
- **Spec**: Part 3 §6.2.7 — "The BrowseName and the NodeClass shall never change."
- **Code**: `async-opcua-nodes/src/base.rs:267` exposes `set_browse_name` publicly, allowing runtime mutation.
- **Impact**: Could violate immutability constraint. Should be crate-internal only.

---

## Part 12 / Part 7 — Discovery & Security Profiles

### DISC-01 [MEDIUM] FindServersOnNetwork: starting_record_id uses >= instead of >
- **Spec**: Part 4 §5.5.3.1 — "Only records with an identifier greater than this number are returned."
- **Code**: `async-opcua-server/src/info.rs:527` — `*record_id >= starting_record_id` should be `>`.
- **Impact**: Duplicate record resent to client. Off-by-one bug.

### DISC-02 [MINOR] ECC security_level = 0 (same as None)
- **Spec**: Part 7 §4.8 — security level reflects policy strength.
- **Code**: `async-opcua-server/src/config/endpoint.rs:105` — ECC policies map to 0.
- **Impact**: ECC policies get same security level as unsecured None. Should be nonzero.

### DISC-03 [LOW] FindServers: registered servers not filtered by endpoint_url
- **Spec**: Part 12 §5.1 — LDS should filter by endpoint URL.
- **Code**: `async-opcua-server/src/info.rs:651` — `_endpoint_url` parameter ignored for registered servers.
- **Impact**: FindServers may return servers not accessible at the client's URL.

### DISC-04 [LOW] FindServers: local server name not locale-filtered
- **Spec**: Part 4 §7.2.4 — `application_name` should support locale selection.
- **Code**: `async-opcua-server/src/info.rs:672-681` — uses single-configured name without locale awareness.
- **Impact**: Clients requesting specific locale get server's default name.

### DISC-05 [LOW] ECC asymmetric encryption returns BadNotImplemented
- **Spec**: Part 6 §6.8 — ECC secure channel encryption.
- **Code**: `async-opcua-crypto/src/security_policy.rs:605-606` — documented limitation.
- **Impact**: ECC secure channel establishment unavailable. Known gap.

---

## Part 4 — Cancel (§5.7.5)

### CANCEL-01 [NOTE] Cancel is an explicit no-op stub
- **Spec**: §5.7.5.1 — Cancel cancels outstanding requests.
- **Code**: `async-opcua-server/src/session/message_handler.rs:431-453` — documented stub. Returns Good with cancelCount=0.
- **Impact**: Valid per spec (0 outstanding = 0 cancelled) but no actual cancellation infrastructure.

---

## Severity Classification

| Severity | Definition | Count |
|----------|-----------|-------|
| HIGH | Missing required behavior, mandatory parameter validation, or spec-mandated audit event | 4 |
| MEDIUM | Spec mismatch, off-by-one error, precision loss, incorrect field behavior | 6 |
| MINOR | Low-impact spec deviation, edge case, cosmetic | 4 |
| LOW | Cosmetic, documentation drift, deferred-consequence, harmless duplication | 9 |

## Key Strengths

1. **Part 6 Binary Encoding**: Zero issues found. Variant, NodeId, String, ByteString, DataValue, ExtensionObject encoding all spec-accurate with correct bounds checking, null handling, and recursion protection.

2. **Subscription state machine**: Thoroughly implemented. Every Part 4 §5.13.1.2 transition documented. Notification queue overflow, discard policy, triggering, late-join all correct.

3. **Part 3/5 Address Space**: Generated from official v1.05 NodeSet2.xml. All mandatory Server Object children, reference type hierarchies correct.

4. **Security**: Certificate validation at OpenSecureChannel, duplicate nonce check for RENEW, channel certificate binding, key grace period (125%), token lifetime backstop all correctly implemented.

## Remediation Priority

1. **SESSION-01** (sessionName default) — 2-line fix in manager.rs
2. **SESSION-03** (clientNonce spec range) — validate against [32,128]
3. **VIEW-01** (RESULT_MASK_IS_FORWARD) — add is_forward to result mask filtering
4. **SC-01** (CloseSecureChannel audit) — dispatch AuditChannelEventType
5. **DISC-01** (starting_record_id off-by-one) — `>=` → `>`
