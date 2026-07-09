# Async Lock Audit Requirements Quality Checklist

**Purpose**: Unit tests for requirements — validates quality, clarity, and completeness of the async lock audit remediation spec.
**Created**: 2026-07-08
**Focus**: Concurrency correctness, coverage completeness, OPC-UA spec alignment
**Depth**: Deep (~65 items)
**Audience**: Peer reviewer / author

---

## Requirement Completeness

- [ ] CHK001 — Are all 27 Functional Requirements (FR-001 through FR-027) mapped to at least one task with a concrete deliverable? [Completeness, Gap]
- [ ] CHK002 — Are all 12 Success Criteria (SC-001 through SC-012) covered by a dedicated test or verification task? [Completeness, Coverage]
- [ ] CHK003 — FR-001 (all RSA/ECC in spawn_blocking) is partially deferred to T086-T091 — is the remaining gap quantified with specific file paths and line numbers? [Completeness, Spec FR-001]
- [ ] CHK004 — FR-025 (client SubscriptionState actor migration, US9) spans 4 tasks — are the actor message contract and state transitions specified? [Completeness, Spec FR-025]
- [ ] CHK005 — Does the spec define requirements for spawn_blocking pool sizing, exhaustion behavior, and backpressure? FR-027 references this but T091 is the only coverage. [Completeness, Spec FR-027]
- [ ] CHK006 — Are requirements specified for each of the 13 mechanical P2 fixes in US8 individually or is blanket "replace lock type" sufficient? [Completeness, Spec US8]
- [ ] CHK007 — Does the spec define what constitutes "minimized" write lock scope for create_monitored_items reverse index (FR-023)? [Completeness, Spec FR-023]

## Requirement Clarity

- [ ] CHK008 — FR-009 states eviction MUST be "O(1) or pre-computed" — is "pre-computed" defined with specific lock scope boundaries? [Clarity, Spec FR-009]
- [ ] CHK009 — FR-018 states build_browse_name_index MUST NOT hold write lock "for the full O(nodes) iteration duration" — is a specific maximum lock hold time specified? [Clarity, Spec FR-018]
- [ ] CHK010 — FR-019 states body limit lookup MUST be O(1) — is the lookup key (secure channel ID? session ID?) explicitly identified? [Clarity, Spec FR-019]
- [ ] CHK011 — SC-001 requires P99 latency "within 2x of median" — is the measurement methodology (sampling interval, warmup, client count ramp) specified? [Clarity, Spec SC-001]
- [ ] CHK012 — SC-002 requires notification latency "under 10ms for a 100ms publishing interval" — is the measurement scope (end-to-end vs server-side only) defined? [Clarity, Spec SC-002]
- [ ] CHK013 — SC-005 requires "P99 latency does not grow with session count at 10,000 active sessions" — is "does not grow" quantified (e.g., within 2x of single-session baseline)? [Clarity, Spec SC-005]
- [ ] CHK014 — Are "lock tracing verification" (T082) success criteria defined — what specific hold-time reductions or patterns must be observed? [Clarity, Spec SC-011]

## Requirement Consistency

- [ ] CHK015 — Do FR-009 (O(1) eviction) and FR-010 (O(log n) expiry) use consistent complexity metric granularity? One says O(1), the other O(log n). [Consistency, Spec FR-009/FR-010]
- [ ] CHK016 — FR-012 (atomic RMW for set_client_offset) and FR-013 (reload request_send) are both marked pre-implemented — do the existing implementations satisfy the atomicity and reload semantics the spec requires? [Consistency, Spec FR-012/FR-013]
- [ ] CHK017 — Are the lock type migration requirements (FR-016: std to parking_lot) consistent with the rest of the codebase which already uses parking_lot per plan.md? [Consistency, Spec FR-016]
- [ ] CHK018 — Do the deferred US1 tasks (T007/T008/T010 mapped to T086-T089) maintain consistent scope with the original Phase 3 tasks, or has scope changed? [Consistency, Spec US1]
- [ ] CHK019 — T053 (pre-construct SessionEntry outside write lock) and T054 (merge sequential write locks) both target subscriptions/mod.rs — are the lock scopes between them consistent and non-overlapping? [Consistency, Spec FR-015/FR-020]

## Acceptance Criteria Quality

- [ ] CHK020 — US6/AC1: "reloads the request_send channel sender and retries on the new transport" — is "retries" defined (single retry? backoff? timeout?)? [Acceptance Criteria, Spec US6/AC1]
- [ ] CHK021 — US6/AC2: "Each update is applied correctly (no lost updates)" — can this be deterministically tested or is it probabilistic? [Acceptance Criteria, Spec US6/AC2]
- [ ] CHK022 — US7/AC2: "only the HashMap insertion happens inside the lock, and the lock is dropped immediately after" — is "immediately" defined against an observable metric? [Acceptance Criteria, Spec US7/AC2]
- [ ] CHK023 — US8/AC2: "two threads no longer both rebuild the index unnecessarily" — can "unnecessarily" be observed/tested (e.g., via counter)? [Acceptance Criteria, Spec US8/AC2]
- [ ] CHK024 — US9/AC2: "notification is delivered through the actor's message handler without any lock acquisition" — is "any lock acquisition" scoped to just the subscription path or all locks? [Acceptance Criteria, Spec US9/AC2]
- [ ] CHK025 — SC-012: "zero Mutex acquisitions in the hot path" — is "hot path" explicitly defined (which operations, which call chains)? [Acceptance Criteria, Spec SC-012]

## Scenario Coverage

- [ ] CHK026 — Are requirements defined for the scenario where spawn_blocking pool is exhausted during a connection storm (FR-027 edge case)? [Coverage, Spec Edge Cases]
- [ ] CHK027 — Are requirements defined for concurrent close_session + commit_create_session_draft competing for the same session slot? [Coverage, Spec Edge Cases]
- [ ] CHK028 — Are requirements defined for the scenario where a subscription actor's ring buffer is full and push_notification drops work items? [Coverage, Spec US2]
- [ ] CHK029 — Are requirements defined for renew_secure_channel when the transport is disconnected mid-renewal? [Coverage, Spec US3]
- [ ] CHK030 — Are requirements defined for create_subscription when the session entry already exists (race between two concurrent CreateSubscription calls)? [Coverage, Spec US7]
- [ ] CHK031 — Are requirements defined for the scenario where OPC-UA client sends PublishRequests faster than the server processes notifications? [Coverage, Spec US2]
- [ ] CHK032 — Are requirements defined for check_session_expiry when both the heap and sessions HashMap are concurrently modified by commit_create_session_draft? [Coverage, Spec US5]
- [ ] CHK033 — Are requirements defined for the program engine (US8/AC6) when a program's state transition collides with an external write to the same address space node? [Coverage, Spec US8]
- [ ] CHK034 — Are requirements defined for data_route_snapshot when a monitored item is deleted between snapshot creation and iteration? [Coverage, Spec US8]

## Edge Case Coverage

- [ ] CHK035 — Does the spec address the edge case where spawn_blocking closure must own Arc<> clones of session/certificate data — are lifetime and Send+'static constraints documented? [Edge Case, Spec Edge Cases]
- [ ] CHK036 — Does the spec address the edge case where SQLite pool max_size must be configurable for different deployment profiles? [Edge Case, Spec Edge Cases]
- [ ] CHK037 — Does the spec address the edge case where session expiry and session creation race on the same BinaryHeap entry? [Edge Case, Spec US5]
- [ ] CHK038 — Does the spec address the edge case where teardown_session's two write locks create a TOCTOU window for session recreation? [Edge Case, Spec FR-020]
- [ ] CHK039 — Does the spec address the edge case where ensure_browse_name_index DCL check races with build_browse_name_index on a cold start? [Edge Case, Spec FR-017]
- [ ] CHK040 — Does the spec address backward compatibility — must all existing public API signatures remain unchanged? [Edge Case, Spec Edge Cases]
- [ ] CHK041 — Does the spec address the edge case where lock tracing (OPCUA_TRACE_LOCKS=1) is enabled and lock acquisition macros behave differently? [Edge Case, Spec SC-011]

## Non-Functional Requirements

- [ ] CHK042 — Are performance requirements (latency, throughput) quantified for the client actor migration (US9)? The spec mentions eliminating 22-27 lock sites but no throughput target. [NFR, Spec US9]
- [ ] CHK043 — Are memory requirements specified for the BinaryHeap expiry (FR-010) — each entry stores a NodeId clone; what is the worst-case heap size? [NFR, Spec FR-010]
- [ ] CHK044 — Are CPU overhead requirements specified for the NotificationAvailable wake path — could high-frequency data changes cause excessive state-machine transitions? [NFR, Spec US2]
- [ ] CHK045 — Are security requirements specified for the spawn_blocking crypto offloading — does moving crypto to a separate thread pool change the attack surface? [NFR, Spec US1]
- [ ] CHK046 — Are availability requirements specified — what happens to in-flight sessions if the spawn_blocking pool panics? [NFR, Spec FR-027]
- [ ] CHK047 — Are debuggability requirements specified — can lock hold times be inspected with lock tracing after all fixes? [NFR, Spec SC-011]

## OPC-UA Specification Alignment

- [ ] CHK048 — Do all tasks that implement OPC-UA protocol behavior carry a grounding reference to the correct specification section (Part 4, Part 6, Part 11)? [Traceability, OPC-10000]
- [ ] CHK049 — Does US2's wake behavior (immediate notification delivery for intervals greater than or equal to 1s) align with OPC-10000-4 Section 5.13.1.2 state transition table? [Spec Alignment, OPC-10000-4 Section 5.13.1.2]
- [ ] CHK050 — Does US3's RenewGuard pattern (CAS + Notify) align with OPC-10000-6 Section 6.7.4 secure channel renewal semantics? [Spec Alignment, OPC-10000-6 Section 6.7.4]
- [ ] CHK051 — Does US4's SQLite WAL mode with r2d2 pool align with OPC-10000-11 Section 6.3 history read concurrency requirements? [Spec Alignment, OPC-10000-11 Section 6.3]
- [ ] CHK052 — Does US5's session expiry (BinaryHeap) preserve the same deadline semantics as OPC-10000-4 Section 5.7.5 session timeout specification? [Spec Alignment, OPC-10000-4 Section 5.7.5]
- [ ] CHK053 — Does the activate_session reverse nesting fix (T042) preserve the OPC-10000-4 Section 5.7.4 activation state machine semantics? [Spec Alignment, OPC-10000-4 Section 5.7.4]
- [ ] CHK054 — Do US6's client transport race fixes (T047-T049) align with OPC-10000-6 Section 6.7 transport state requirements? [Spec Alignment, OPC-10000-6 Section 6.7]
- [ ] CHK055 — Do subscription tasks T051-T054 carry correct OPC-10000-4 Section 5.14.2 references, and are any additional spec sections needed for monitored item, modify, or delete operations? [Spec Alignment, OPC-10000-4 Section 5.14]

## Dependencies & Assumptions

- [ ] CHK056 — Is the assumption that "symmetric crypto for chunks less than 64KB takes less than 100 microseconds" (FR-003) validated with benchmarks, and are the benchmark results traceable? [Assumption, Spec FR-003]
- [ ] CHK057 — Is the assumption that "SQLite WAL mode enables concurrent readers" (Spec Assumptions) validated against the specific SQLite version bundled by rusqlite? [Assumption, Spec Assumptions]
- [ ] CHK058 — Is the assumption that "per-channel response body limits change infrequently" (Spec Assumptions) validated with production telemetry or is it speculative? [Assumption, Spec Assumptions]
- [ ] CHK059 — Is the assumption that "tokio's default 512 spawn_blocking threads are adequate" (Spec Assumptions) validated against expected load, and does T091 address configurable sizing? [Assumption, Spec Assumptions]
- [ ] CHK060 — Is the assumption that "program engine runs infrequently enough that batching writes does not affect correctness" (Spec Assumptions) explicitly documented in code? [Assumption, Spec Assumptions]

## Traceability & Verification Gates

- [ ] CHK061 — Does each user story have a clearly defined independent test that can be run in isolation to validate that story alone? [Traceability]
- [ ] CHK062 — Do all 9 user stories specify which audit findings they cover, and are all 35 audit findings (7 P0, 10 P1, 18 P2) accounted for? [Traceability]
- [ ] CHK063 — Is there a defined CI gate (tools/ci-playbook.sh --ci) and is its expected behavior (all steps pass) documented? [Traceability, Plan Verification Strategy]
- [ ] CHK064 — Are the phase-level verification commands (cargo test, cargo clippy) specified for every phase, and do phases 8-13 include them? [Traceability, Tasks Notes]
- [ ] CHK065 — Is there a requirement that no existing test (618+) regresses after each phase — and is this verified per-phase or only at the end? [Traceability, Spec SC-010]
