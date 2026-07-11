# Contracts: Hot-Path Per-Request Throughput

Internal contracts. The "interface" is (1) the measurement gate every change must pass, and (2) the
behavioral invariants the read fast-path and the per-request cuts must uphold. No external/wire contract
changes (that is the point — SC-004).

## C1. Measurement gate (applies to EVERY change)

- **G-baseline**: the HEAD baseline (data-model "HEAD performance baseline") is captured and recorded in
  `research.md` before the first code change. No structural change is committed until it exists.
- **G-us1**: a US1 change is committed iff, on a pinned single core (median of ≥3 runs), single-client
  read (and, where relevant, write) throughput **improves or is neutral** vs the baseline. A change that
  regresses is reverted. A pure safety cleanup that is throughput-neutral may still land (with that noted).
- **G-us1-stage2**: the read fast-path (S2) is *kept* only if it clears the ≥1.5× US1 bar (SC-001) or
  otherwise materially improves single-client + aggregate throughput. If it does not, it is removed, not
  left dormant.
- **G-us2**: a US2 change is committed iff a multi-thread sweep shows per-core efficiency / plateau
  improves **and** the targeted `perf c2c` HITM line measurably drops. Never on projection.
- **G-evidence**: each committed change records its before/after numbers (SC-006).

## C2. Equivalence invariant (all changes)

- Byte-identical wire output for identical inputs across every security-policy × mode × identity-token
  combination. **Test**: `async-opcua/tests/integration/conformance.rs` stays green, unchanged.
- No protocol-visible diagnostic value changes (internal-only counters may change; a client-readable
  diagnostic node/value must not).

## C3. Read fast-path invariants (S2)

1. **Status-code fidelity**: for identical inputs the fast path returns the exact same `StatusCode`s per
   node as the actor path. **Test**: all-attributes × all-policies read equivalence.
2. **Panic isolation (fail-closed)**: a node manager that panics on a fast-path read yields
   `BadInternalError` for that request and the connection + other sessions survive — the fast path wraps the
   read in `AssertUnwindSafe(...).catch_unwind()` exactly as `actor.rs:323`. **Test**: a node manager rigged
   to panic on read → assert `BadInternalError` + a subsequent request on the same connection succeeds.
3. **Cancellation/deadline**: the fast path returns `AsyncMessage(JoinHandle)`; the controller's deadline +
   abort wrapper (`controller.rs:973`) still aborts an in-flight fast-path read. **Test**: a slow read past
   its deadline is aborted.
4. **Session-activity touch**: unchanged — done in the controller before dispatch (`instance.rs:227` via
   `controller.rs:1124`); the fast path must not skip request validation.
5. **Ordering (documented)**: a fast-path read MAY run concurrently with a queued write on the same session;
   memory-safe via the `AddressSpace` `RwLock`; OPC UA does not require cross-service-call read-after-write
   ordering. This is an accepted, documented behavior — not a regression.
6. **Scope**: only pure Value-attribute reads take the fast path; any non-Value attribute or any read
   needing browse/cold data goes through the existing path.

## C4. Per-request cut invariants (S1a–S1d)

- **S1a**: `validate_timed_out` still enforces the session timeout; the monotonic `AtomicU64` is
  behaviorally equivalent to the prior `ArcSwap<Instant>` for the comparison it feeds.
- **S1b**: `ChunkInfo` results are byte-identical; it is still computed only on decrypted chunks and remains
  per-chunk. **Test**: existing chunk/secure-channel tests green.
- **S1c**: gating actor timing behind `diagnostics` changes no protocol-visible value; with `diagnostics`
  on, the metric still populates.
- **S1d**: reducing timer re-arming does not change request deadline/timeout semantics — deadlines still fire
  at the same wall-clock time. **Test**: existing timeout/cancellation tests green.

## C5. No-new-locks invariant (AGENTS.md)

No change introduces a new lock/mutex/blocking primitive; S1a *removes* an `Arc` alloc (ArcSwap→AtomicU64),
S1b *replaces* a `Mutex` with `OnceLock`, US2 *replaces* `RwLock<HashMap>` with `DashMap`. If any new
synchronization is unavoidable, run the `audit-locks` skill on it first.
