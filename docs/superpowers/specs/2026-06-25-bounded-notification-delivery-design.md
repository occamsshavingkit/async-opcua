# Bounded-Ring Notification Delivery (the async-delivery invariant) — Design

Date: 2026-06-25
Status: approved (brainstorm); full bounded-ring + majordomo approach confirmed

## Purpose

Hold the async invariant on the notification hot path: **never hold a session/subscription lock
across an unbounded amount of work.** Today producers flush notifications into the monitored-item
queues while holding the per-session `Mutex<SessionSubscriptions>`, and ConditionRefresh replays N
retained conditions under that same lock. Measured (this branch,
`bench_condition_refresh_lock_hold_scaling`): **O(N) at ~43 µs/condition — 87 ms at 2 000 conditions,
extrapolating to ~430 ms at 10 000** — and that window blocks every other operation on the session
(other subscriptions' Publish, GetMonitoredItems, …).

Fix: producers stop flushing under the lock and instead **push work items into a bounded lock-free
ring**; a **majordomo** drains the ring into the existing monitored-item queues **in bounded chunks**,
holding the session lock only over each chunk. Cribbed from QuackPLC's `rtrb` discipline (bounded
lock-free ring + backpressure + fan-in majordomo), minus the cross-process memfd/seal layer it does
not need in-process.

## Scope

**In scope**
- A per-session bounded lock-free ring (`crossbeam-queue::ArrayQueue`; crossbeam is already a
  dependency — no new crate) carrying `NotificationWorkItem`s.
- A `NotificationWorkItem` that is either a single (monitored-item, notification) or a contiguous
  **refresh batch** (subscription, optional target item, ordered events) so ConditionRefresh stays
  atomic in one ring slot.
- A **majordomo drain** integrated into the existing `periodic_tick` (and the Publish-driven tick)
  that pops work items and enqueues them into the monitored-item queues **in bounded chunks**,
  re-acquiring the session lock per chunk so it is never held over unbounded work.
- Rewiring the producers — `notify_events`, ConditionRefresh delivery, then `notify_data_change` — to
  push to the ring instead of flushing under the session `Mutex`.
- Backpressure/overflow accounting when the ring is full (bounded memory; surfaced via the existing
  `EventQueueOverflow` / status semantics — producers are not blocked).
- Before/after benchmark + correctness tests.

**Out of scope**
- Replacing the monitored-item queues themselves (their EventFilter / deadband / `EventQueueOverflow`
  / discard-oldest / retransmission semantics are unchanged — the ring sits *in front* of them).
- The full actor-per-session rewrite (`specs/005` US3). This reuses the existing central tick driver;
  dedicated per-session tasks remain a later option if latency demands.
- `thingbuf` slot-recycling (no-alloc) — a documented upgrade if allocation churn is later measured.

## Architecture

```
producer (notify_events / ConditionRefresh / notify_data_change)
   │  resolve target items under the SubscriptionCache READ lock (unchanged, concurrent-safe)
   │  push NotificationWorkItem  ──► [ per-session bounded ArrayQueue ]   (lock-free, O(1); Err on full)
   │  return immediately (no session Mutex held)
                                          │
   periodic_tick / publish-tick ──────────┘  MAJORDOMO DRAIN:
      take session Mutex → enqueue up to CHUNK items into monitored-item queues → release
      → repeat until ring drained or a drain budget is reached → then run the normal tick/Publish
```

The session `Mutex` is held only over a bounded `CHUNK` of enqueues, never over all N. Producers never
take the session `Mutex` at all. All existing queue semantics apply at enqueue time, unchanged.

## Components (one responsibility each)

1. **`NotificationWorkItem`** (enum) — `Data { handle, value }` | `Event { handle, event }` |
   `RefreshBatch { subscription_id, monitored_item: Option<MonitoredItemHandle>, events: Vec<…> }`.
   The refresh batch is ONE ring slot so RefreshStart→events→RefreshEnd stay contiguous and ordered
   even under concurrent producers (FIFO across producers preserves relative order; a single-slot
   batch keeps the refresh internally contiguous).
2. **Per-session ring** — `ArrayQueue<NotificationWorkItem>` of fixed capacity, created with the
   `SessionSubscriptions` (`mod.rs:362/791`) and dropped with it (`:252`). Lock-free MPMC used as
   many-producers→one-drainer. `push` returns `Err(item)` when full → overflow path.
3. **Producer push** — the existing `Notifier` (`notify.rs`) resolves target items under the
   `SubscriptionCacheInner` **read** lock (unchanged), but on flush PUSHES work items to the target
   session's ring instead of taking the session `Mutex`. ConditionRefresh pushes one `RefreshBatch`.
4. **Majordomo drain** — `drain_ring(session, budget)` called at the start of `periodic_tick`'s
   per-session step and when a Publish request is serviced: pop up to `CHUNK` items, take the session
   `Mutex`, enqueue them (applying EventFilter/deadband/overflow exactly as today), release; loop to a
   `budget` (so one session can't starve others). A `RefreshBatch` is expanded and enqueued in
   `CHUNK`-sized sub-batches across lock acquisitions.
5. **Overflow accounting** — when `push` returns `Err` (ring full), increment a per-subscription
   dropped-notification counter and set the overflow indicator the next delivered notification carries
   (reuse the `EventQueueOverflow`/`StatusCode` overflow path rather than blocking the producer).

## Data flow & correctness

- **Ordering**: `ArrayQueue` is a FIFO queue linearized at push-completion, and the drain pops in that
  order, so a *single producer's* items keep their relative order — which is what matters, since a given
  monitored item's notifications come from one producer (the sampler for data, the event dispatch for
  events). RefreshStart→…→RefreshEnd stay ordered because the whole refresh is one batch slot (it cannot
  be torn or reordered by other producers). Cross-producer interleaving of *different* items is
  immaterial and spec-permitted.
- **Refresh atomicity / `Bad_RefreshInProgress`**: a refresh is one `RefreshBatch` slot, so it cannot
  be torn by concurrent producers. Concurrent refreshes become two ordered batches; interleaving of
  *other* events between two refreshes is spec-permitted (Part 9 §5.5.7). `Bad_RefreshInProgress`
  remains structurally unreachable (push is instant; nothing spans publishes).
- **Latency**: enqueue now happens at the next tick/Publish drain rather than synchronously. Because
  notifications are delivered to the client on Publish anyway (bounded by the publishing interval), the
  drain runs at the start of each tick/Publish, so client-visible latency is unchanged in the normal
  case; under burst it is bounded by the drain budget.
- **The benchmark** must show ConditionRefresh's *Call* time drops from O(N) to ~O(1) (it just pushes a
  batch), and that a concurrent op on the same session no longer stalls for the full N.

## Backpressure / overflow

Bounded ring (capacity sized from the subscription limits, e.g. a multiple of
`max_notifications_per_publish × max_subscriptions_per_session`). On full, the producer does NOT block
or spin (QuackPLC's refuse-and-retry is wrong for a shared async producer): it drops the work item and
records overflow, which surfaces to the client through the existing queue-overflow indication. Memory
is bounded by ring capacity + the (already bounded) item queues.

## Error handling

- Ring full → overflow accounting, never panic, never block. The connection/subscription is not torn.
- Session torn down mid-drain → the ring is dropped with its `SessionSubscriptions`; in-flight items
  are discarded (the session is gone).
- A `RefreshBatch` whose subscription/item vanished before drain → dropped (status logged), not a panic.

## Testing (Claude authors, independent)

1. **Before/after benchmark** (extend `bench_condition_refresh_lock_hold_scaling`): assert the
   ConditionRefresh *Call* time is now ~flat in N (push-only), and add a **concurrent-stall** measurement
   — a small op on the same session during a 2 000-condition refresh — showing its latency no longer
   scales with N.
2. **Correctness, unchanged behavior**: the full existing alarms + subscription integration suites stay
   green (ConditionRefresh late-subscriber sync, Refresh2 targeting, ack/confirm, limit alarms,
   data-change subscriptions, EventQueueOverflow).
3. **Ordering**: a refresh burst + interleaved normal events arrive with RefreshStart before
   RefreshEnd and per-subscription order preserved.
4. **Overflow**: drive more notifications than the ring holds; assert overflow is indicated (not a
   panic / not unbounded memory) and the subscription recovers.

## Implementation split

Per project workflow: **codex implements** the ring, work-item, producer push rewiring, and majordomo
drain (feature/hot-path code); **Claude authors/validates** the benchmark + correctness tests and runs
the before/after numbers. One concern per codex dispatch; every brief carries the scope-escape rule and
"run `cargo fmt` on files you touch". No-git guardrail; branch verified after each.

## Decomposition (→ implementation plan)

- T1 (codex): `NotificationWorkItem` + the per-session `ArrayQueue` ring on `SessionSubscriptions`
  (created/dropped with it), with overflow accounting. No behavior change yet (ring unused).
- T2 (codex): majordomo `drain_ring` (bounded-chunk enqueue into the existing queues) wired into
  `periodic_tick` + the Publish-tick path.
- T3 (codex): rewire the **event** producers (`notify_events`) + **ConditionRefresh** to push to the
  ring (`RefreshBatch`) instead of flushing under the session `Mutex`. (The measured pathology.)
- T4 (codex): rewire `notify_data_change` (and `maybe_notify`) onto the ring.
- T5 (Claude): before/after benchmark (flat-in-N + concurrent-stall) + ordering/overflow correctness
  tests; confirm the full existing suites stay green.
