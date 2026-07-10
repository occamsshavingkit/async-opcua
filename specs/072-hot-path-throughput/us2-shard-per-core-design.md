# Feature 072 · US2 design — shard-per-core dispatch + RCU address space

Design captured 2026-07-11 from a collaborative design pass. **Not yet built** — this is the spec + the
measurement gate. It targets the multi-core / under-load regime (US2), where the per-request cuts already
pay off (S1a+S2 = 508K→569K plateau, research.md R14). Everything here is **measure-first**: ship only if it
beats the **569K** plateau on the concurrency sweep.

## Why (what the measurements established)

- Single-client per-request cuts are ~2% (R13); the ~1.5× clean-core gap to open62541 is **structural** —
  async-runtime `spawn` + syscall boundary + serialization, not the actor hop.
- Under load, the cuts are +12% aggregate (R14) because they reduce per-request **allocator + scheduler
  contention that scales**. The next lever in that direction is to stop paying the spawn/steal tax entirely
  under load — i.e. a shard-per-core pipeline that keeps work core-local.

## Architecture

**Thread-per-core (shard-per-core), the seastar/glommio model:**

- **Network agent** — epoll + `recv` + chunk **reassembly** only. Pushes the *reassembled, still-encrypted*
  message chunk to the owning connection's per-core ring. **No crypto, no decode here** (see "the detail
  that makes or breaks it" below).
- **Per-core worker** — one pinned single-thread (`current_thread`) runtime per logical CPU, each owning a
  set of connections end-to-end: **decrypt → decode → process → encode → encrypt → send**. Because a shard
  is single-threaded, everything inside it (any local task pool, its LIFO reuse, its 15s idle reap) needs
  **no lock** — the only cross-core synchronization is the SPSC rings (lock-free by construction) and the
  global address space (see RCU below).
- **Per-core SPSC ring** — single-producer (the assigner / network agent) → single-consumer (the pinned
  worker). Head/tail atomics, no permit semaphore, no per-request `oneshot` allocation. This is the cheap
  FIFO; it is *not* tokio's `mpsc`/`oneshot`.
- **Accept-time load balancer** — a new connection is assigned to the core with the **fewest active workers**
  (real, instantaneous load), ties broken by **fewest active connections** (potential load). Reads a per-core
  atomic busy-count at accept; staleness is harmless for a once-per-connection decision. This self-corrects
  for *new* load: a hot core stops receiving connections until its busy-count drops. Residual imbalance from
  *existing* connections turning bursty is handled by **backpressure**, not migration — see below.
- **Per-core backpressure (the primary imbalance response)** — a saturated core simply lets its SPSC ring
  fill; the network agent then slows `recv` on that core's connections and TCP flow control throttles the
  clients. No state transfer, no ordering hazard, no migration machinery. This is correctness-safe by
  construction and is the default answer to a hot core.

**Optional simplification**: the network agent can push directly to the connection's per-core ring (it knows
the mapping), collapsing the separate central "assigner"/FIFO stage. Keep the assigner separate only if the
concern-isolation is worth the extra hop.

## Read/write asymmetry on the (global, shared) address space

Reads dominate; only writes need serialization. **RCU / `ArcSwap`** (already the codebase's `TypeTreeSnapshot`
pattern) — chosen over a seqlock deliberately:

- **A seqlock is unsound here.** A seqlock reader reads optimistically and retries on a concurrent write —
  sound only for small *trivially-copyable* (POD) payloads. The address space is a `HashMap`/`Vec` of nodes
  and references; reading it while a writer mutates it is a **data race = UB in Rust** (freed-pointer deref,
  `HashMap` mid-resize). A correct Rust seqlock needs atomic/`volatile` payloads (POD), which a node graph
  is not.
- **RCU dominates the seqlock for this payload:** readers `load()` the current immutable snapshot (one atomic
  pointer load, lock-free) and hold it — **no retry, no torn reads**; the old snapshot lives until the last
  reader's `Arc` drops (automatic reclamation — no ABA, no hazard pointers).

Mapping the "seqlock" intuition onto RCU:

| Intuition | RCU realization |
|---|---|
| read-only request → seqlock, no queuing | reader `snapshot = cold.load()`, processes **inline** — no queue, no lock, no retry |
| write → CAS, grab only if seqlock "even" | writer CASes a **single-writer gate** (the "even" check), reads the snapshot, builds a **new** snapshot with the mutation (COW), `store()`s it, releases |

- The hot **value-read path is already lock-free** (`node_map` is a `DashMap`, per-entry concurrent). It is
  the **cold** side (references, browse-name index, namespaces) that becomes `ArcSwap<Arc<AddressSpaceCold>>`.
- Cost: RCU writes are **copy-on-write** (a structural mutation copies the cold structure). Fine precisely
  because writes (`AddNodes`/`DeleteNodes`) are rare and reads are constant — the read-mostly trade.
- This is the same **US2 "correctness-preserving outer-lock reduction"** flagged in research.md R0/R7:
  route reads through the lock-free map + snapshot, keep a single-writer barrier for structural mutation.
  It preserves `node_map`↔`cold` atomicity for `browse` (a reader holds one consistent snapshot).

## The detail that makes or breaks it

Per-connection sharding only earns its keep if the **per-request crypto (AES decrypt/encrypt) and session
context run on the connection's assigned core** — that is the state the sharding keeps hot. So the network
agent must do sockets + framing only; the **worker** does decrypt→decode→…→encrypt. Decoding/decrypting
centrally would move the single most expensive per-request work onto one serial thread and throw away the
locality the sharding was for.

## Open risks (to resolve by measurement)

1. **Central network agent + assigner as serial fronts** — one thread doing epoll/framing (and one routing)
   for the whole box. Cheap per request, but the eventual scaling ceiling; seastar shards `accept()` to
   avoid it. Fine for a first prototype.
2. **Mid-life rebalancing (deferred, measured-later)** — connections are pinned at accept, so a connection
   that later turns bursty can hot-spot its core. The **decision is to NOT migrate connections in the first
   cut**; rely on accept-time balancing + per-core backpressure, and only add migration if a measured
   per-core distribution under the sweep shows sustained imbalance the balancer + backpressure cannot absorb.
   The rationale, worked through explicitly:

   - **The tempting trigger "core at max workers + empty idle stack" is the wrong signal on its own.** An
     empty worker stack means "all workers busy" — but that is two different worlds. Workers busy
     *computing* → the core is CPU-saturated (shedding load is right). Workers busy *`.await`-ing* (slow node
     manager, socket backpressure) → the core's CPU is actually **idle**, and the correct response is *more
     concurrency* (grow the pool / raise the cap), **not** migration. The signal that actually means "this
     core can't keep up" is the **ring sustained-backing-up** (arrivals outpacing drain) or CPU-util — not
     the worker count.
   - **The response — live connection migration — is the real hazard, not the trigger.** A "rebalance
     request" to the balancer is trivial; *moving the load* is a minefield, which is why seastar/glommio
     largely **don't** migrate connections:
     - It **temporarily inverts the whole point** — the connection's session + secure-channel keys go
       **cache-cold on the new core**, so migration *costs* before it pays; only worth it under *sustained*
       imbalance.
     - **Secure-channel sequence ordering** — OPC UA chunks on a channel are strictly ordered by sequence
       number. Migrating mid-stream risks reordering/torn requests. It requires **draining the old core's
       in-flight work for that connection, then atomically flipping the connection→core mapping**, with the
       network agent buffering anything that arrives during the handoff — a small protocol in its own right.
   - **If migration is ever built**, its trigger must be **sustained ring-backlog** (not empty stack), gated
     with **hysteresis + a cooldown** so a connection doesn't ping-pong, and it must carry the
     drain-and-atomic-flip ordering protocol above.
3. **Run-to-completion vs await** — a worker request that `.await`s (slow node manager, socket backpressure)
   must not stall its shard's whole ring; the shard's `current_thread` runtime provides cooperative
   concurrency (multiple in-flight interleaving at await points) so this is handled *if* requests are
   spawned onto the shard runtime rather than run strictly to completion.

## Measurement gate (non-negotiable)

- **Prototype in the bench server first** (`async-opcua-bench-server`, which we control and measure), prove
  it beats **569K** on `run_concurrency_sweep.py` (server 5-11) before grafting into async-opcua proper.
- Capture `perf c2c`/HITM + off-CPU/wakeup on the prototype to confirm the win is the removed scheduler
  steal + allocation, not noise.
- Ship into async-opcua only if the loaded aggregate improves **and** correctness (conformance byte-identical,
  `browse`/structural-write atomicity under RCU) holds.
- Concurrency review: run the `audit-locks` skill on the SPSC rings + the single-writer gate + RCU
  reclamation; `loom` the single-writer CAS. (The shard-local single-threadedness removes the earlier
  `Mutex<Vec>` pool-stack hazard entirely.)
