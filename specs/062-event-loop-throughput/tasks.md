# Tasks: Event Loop Throughput

**Input**: Design documents from `/specs/062-event-loop-throughput/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md

**Tests**: No new tests required. Existing test suite (`cargo test --locked --all-features`) serves as the regression guard for all changes. The benchmark (`tools/opcua-localhost-bench`) provides throughput measurement after each phase.

**Organization**: Tasks are grouped by user story. US1 and US2 are both P1 and independent — they touch different code paths (US1: encoding pipeline, US2: transport poll). No phase ordering between them beyond recording a baseline first.

**OPC UA Spec Citations**:
- OPC-10000-6 §6.7.2: Sequence number ordering for outgoing chunks (US1, FR-003)
- OPC-10000-6 §7.1: Transport layer message framing (US2, FR-004)

**Scope note**: US1 (T002-T005) applies only to the async response path — completed futures from `FuturesUnordered` in the `resp_fut` arm of `controller.rs::run()`. Synchronous `enqueue_message_for_send()` calls inside `process_request()` itself (OpenSecureChannel, CreateSession, RegisterServer, validation faults — at lines 375, 436, 455, 694, 761, 827, 923, 987) are connection-setup and error paths, not the throughput hot-path. These are intentionally left unchanged.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Include exact file paths and OPC UA spec grounding in descriptions

---

## Phase 1: Setup — Establish Baseline

**Goal**: Record the pre-change benchmark throughput so each story's impact can be measured.

- [x] T001 Build release binary at HEAD and record baseline benchmark throughput (both read and write, 3 runs each, report median) using `cargo build --release --bin async-opcua-localhost-bench && for i in 1 2 3; do ./target/release/async-opcua-localhost-bench run --op read --warmup 3 --measure 5; done`

---

## Phase 2: User Story 1 — Decouple Response Encoding From the Event Loop (Priority: P1)

**Goal**: Move `Chunker::encode_into()` + `SendBuffer::write()` from the synchronous `resp_fut` arm in the event loop (`controller.rs:260`) into the async task that produces the response. The event loop receives pre-encoded `MessageChunk` vectors and pushes them into `SendBuffer` without re-encoding.

**Independent Test**: `cargo test --locked --all-features` passes. Benchmark throughput does not regress. Manual inspection: `enqueue_message_for_send()` is no longer called in the `resp_fut` arm; encoding happens in the async task closure.

**Spec grounding**: FR-001, FR-002, FR-003; OPC-10000-6 §6.7.2

### Implementation for User Story 1

- [x] T002 [US1] Add `push_encoded_chunks(chunks: Vec<MessageChunk>, chunk_count: u32)` method to `SendBuffer` in `async-opcua-core/src/comms/buffer.rs`. Pushes pre-encoded chunks into `self.chunks` as `PendingPayload::Chunk` and increments `self.sequence_numbers` by `chunk_count`. No `Chunker::encode_into()` call inside this method — it is a pure queue operation. Grounding: OPC-10000-6 §6.7.2 (per-chunk sequence number monotonicity).
- [x] T003 [US1] Define `EncodedResponse` struct and update `PendingMessageResponse` type alias in `async-opcua-server/src/session/controller.rs`. `EncodedResponse` carries `chunks: Vec<MessageChunk>`, `chunk_count: u32`, `request_id: u32`. Change `PendingMessageResponse` output from `Result<Response, String>` to `Result<EncodedResponse, String>`. Grounding: FR-002 (pre-encoded chunk handoff from task to event loop).
- [x] T004 [US1] Refactor the async task closure in `async-opcua-server/src/session/controller.rs` (lines 867-899, the `AsyncMessage` arm) to call `Chunker::encode_into()` internally and produce `EncodedResponse`. The closure must capture (cloned, before the `async move` block): `SequenceNumberHandle` (clone), `max_message_size` (copy), `send_buffer_size` (copy), the `ServerInfo`'s encoding context (clone `DecodingOptions` from `self.channel.context()`), and a newly allocated `bytes::BytesMut` for chunk storage per Option A in research.md. Encoding context is read-only data extracted from `SecureChannel` before the async block starts, avoiding borrow of `&mut self.channel`. The task produces `EncodedResponse` instead of `Response`. Grounding: OPC-10000-6 §6.7.2 (Chunker::encode_into() with correct sequence numbers); OPC-10000-6 §5.1.3 (encoding context from server info).
- [x] T005 [US1] Update the `resp_fut` arm in `async-opcua-server/src/session/controller.rs::run()` (lines 247-268) to receive `EncodedResponse` and call `self.transport.push_encoded_chunks()` instead of `self.transport.enqueue_message_for_send()`. Remove the synchronous `Chunker::encode_into()` path from this code path. The event loop's `SendBuffer` is no longer involved in encoding — only enqueuing pre-encoded chunks. Grounding: FR-001, FR-002.
- [x] T006 [US1] Run `cargo test --locked --all-features` to verify no regressions. Then run `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` and `cargo fmt --all -- --check`. Grounding: SC-002, SC-003.
- [x] T007 [US1] Rebuild release binary (`cargo build --release --bin async-opcua-localhost-bench`) and run benchmark (read mode, 3 runs, report median). Compare throughput against T001 baseline; confirm no regression. Grounding: SC-001.

**Checkpoint**: US1 complete — encoding decoupled from event loop. Benchmark verified.

---

## Phase 3: User Story 2 — Batch Incoming Message Reads (Priority: P1)

**Goal**: Drain all available complete messages from the TCP stream per `transport.poll()` wake-up instead of processing one message at a time. Reduces `tokio::select!` evaluations and scheduler wake-ups per message.

**Independent Test**: `cargo test --locked --all-features` passes. Benchmark throughput stable or improved. Manual inspection: `poll_inner()` contains a drain loop; `TransportPollResult::IncomingMessages` carries a `Vec<Request>`.

**Spec grounding**: FR-004, FR-005, FR-006; OPC-10000-6 §7.1

### Implementation for User Story 2

- [x] T008 [US2] Update `TransportPollResult` enum in `async-opcua-server/src/transport/tcp.rs` — rename `IncomingMessage(Request)` variant to `IncomingMessages(Vec<Request>)`. Update all match arms referencing this variant in `controller.rs` (line 271). Grounding: FR-004 (batch return from transport poll).
- [x] T009 [US2] Add a drain loop to `Transport::poll_inner()` in `async-opcua-server/src/transport/tcp.rs` (lines 416-458). After receiving one complete message via `self.read.next().await`, loop calling `self.read.next().now_or_never()` (from `futures::FutureExt`) to drain additional complete messages without awaiting. Stop the drain loop when: (a) `now_or_never()` returns `None` (no more data immediately available), (b) the send buffer has pending data that needs flushing (check `can_read()`), (c) the stream returns `None` (connection closed), or (d) an error occurs. Collect all complete messages into a `Vec<Request>` and return `TransportPollResult::IncomingMessages`. Grounding: OPC-10000-6 §7.1 (message framing — drain all complete frames from the TCP stream).
- [x] T010 [US2] Update `ConnectionTransport` trait's `poll()` signature in `async-opcua-server/src/transport/tcp.rs` (line 55) to reflect the batched return type. Ensure the trait's `poll()` return type is compatible with `TransportPollResult::IncomingMessages(Vec<Request>)`. Grounding: FR-004.
- [x] T011 [US2] Update `async-opcua-server/src/session/controller.rs::run()` (lines 269-286) to iterate over the batched `Vec<Request>` from `TransportPollResult::IncomingMessages`. For each request, call `process_request()` — which spawns an async task into `pending_messages`. After each spawn, check `self.pending_messages.len() >= self.max_inflight`; if the limit is reached, stop the iteration and yield to let completions flush. Grounding: FR-005, FR-006; OPC-10000-6 §7.1 (dispatch all messages before returning to select! loop).
- [x] T012 [US2] Run `cargo test --locked --all-features` to verify no regressions from batching. Then run `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings` and `cargo fmt --all -- --check`. Grounding: SC-002, SC-003.
- [x] T013 [US2] Rebuild release binary and run benchmark (read mode, 3 runs, report median). Compare throughput against T001 baseline; confirm no regression. Grounding: SC-001.

**Checkpoint**: US2 complete — batched incoming reads working. Benchmark verified.

---

## Phase 4: Combined Verification

**Goal**: Verify both US1 and US2 work together correctly and pass all CI gates.

- [x] T014 Rebuild release binary with both US1 and US2 applied and run benchmark (read + write, 3 runs each, report median). Compare against T001 baseline; confirm stable or improved throughput. Grounding: SC-001.
- [x] T015 Run the full CI playbook via `tools/ci-playbook.sh --ci` to confirm all gates pass with both changes applied. Grounding: SC-002.
- [x] T016 Run `perf stat -e instructions,cycles,cache-misses,branch-misses` on the final binary (both changes applied) and compare against pre-change (T001) baseline to confirm reduced instructions-per-request from eliminated event-loop encoding. Grounding: SC-001 (performance validation).

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1 (Setup): T001 (baseline)
     │
     ├── Phase 2 (US1): T002 → T003 → T004 → T005 → T006 → T007
     │
     └── Phase 3 (US2): T008 → T009 → T010 → T011 → T012 → T013
                              │
                          Phase 4: T014 → T015 → T016
```

- **US1 and US2 are independent**: They touch different code concerns (encoding pipeline vs. transport poll) and can be implemented in parallel after T001.
- **Phase 4** requires both US1 and US2 to be complete.

### Within Each User Story

- US1: T002 (SendBuffer API) → T003 (EncodedResponse type) → T004 (async task refactor) → T005 (event loop arm). T006 (verification) → T007 (benchmark).
- US2: T008 (enum change) → T009 (drain loop in poll_inner) and T010 (trait signature) within same file → T011 (controller iteration). T012 (verification) → T013 (benchmark).

### Parallel Opportunities

```
# After T001:
Agent A: T002 → T003 → T004 → T005 → T006 → T007  (US1)
Agent B: T008 → T009 → T010 → T011 → T012 → T013  (US2)
```

T009 and T010 are in the same file (`tcp.rs`) and sequential — enum must change before the trait can reference it.

---

## Implementation Strategy

### MVP (User Story 1 Only)

1. T001: Record baseline
2. T002-T007: Implement US1 (decouple encoding)
3. **STOP and VALIDATE**: Benchmark comparison, full test suite
4. This delivers the primary benefit — no synchronous encoding in the event loop

### Incremental Delivery

1. T001: Baseline
2. T002-T007: US1 → verify → (optionally merge as independent improvement)
3. T008-T013: US2 → verify → (optionally merge as independent improvement)
4. T014-T016: Combined verification → ready to merge

### Recommended Order

Both stories are P1. Start with US1 because it has the most direct architectural benefit (eliminates synchronous CPU work from the event loop) and its API changes to `SendBuffer` are naturally isolated. US2 builds on the same transport layer but is mechanically simpler (pure drain loop + enum change).
