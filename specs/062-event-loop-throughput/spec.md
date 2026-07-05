# Feature Specification: Event Loop Throughput

**Feature Branch**: `062-event-loop-throughput`  
**Created**: 2026-07-05  
**Status**: Draft  
**Input**: User description: "Decouple response encoding from the event loop and batch incoming message reads to improve per-connection throughput."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Decouple Response Encoding From the Event Loop (Priority: P1)

When a response completes in `SessionController::run()`, the event loop calls `transport.enqueue_message_for_send()` which encodes and chunks the response synchronously before the next `select!` iteration. This CPU work (binary encoding, chunking, sequence number assignment) blocks the event loop from reading new incoming TCP data or collecting other completed responses. Moving encoding into the async task that produced the response allows the event loop to interleave send-prep and receive-poll without CPU stalls.

**Why this priority**: Eliminates synchronous CPU work from the single-threaded event loop. Every response currently delays the next receive poll by the encoding+chunking cost. For large responses (e.g., bulk Browse, large DataValue arrays), this is a measurable stall.

**Independent Test**: Run the localhost benchmark before and after. Throughput (req/sec) should increase or remain stable. Verify no response ordering or sequence-number correctness issues via the existing integration test suite.

**Acceptance Scenarios**:

1. **Given** a response is ready to be sent, **When** the response is produced by an async service task, **Then** encoding and chunking happen inside that task before the response is placed on the event loop's completion queue.
2. **Given** encoded chunks are delivered to the event loop, **When** the event loop processes the completion, **Then** it only enqueues pre-encoded chunks into `SendBuffer` without running `Chunker::encode_into()`.
3. **Given** the refactored send path, **When** `cargo test --locked --all-features` runs, **Then** all tests pass with identical response content.

---

### User Story 2 - Batch Incoming Message Reads (Priority: P1)

The event loop currently calls `transport.poll()` once per `select!` iteration, reading at most one message. When multiple chunks arrive back-to-back (common at high throughput), each requires a separate `select!` wake-up. Draining all available incoming messages in a single wake-up reduces `select!` overhead and lets the event loop batch-process completions.

**Why this priority**: Reduces tokio scheduler wake-ups per message. At 100k req/sec, this is 100k fewer `select!` evaluations per second, directly reducing CPU overhead.

**Independent Test**: Same as US1 — benchmark comparison before/after, full test suite for correctness.

**Acceptance Scenarios**:

1. **Given** incoming TCP data is available, **When** `transport.poll()` yields data, **Then** all available complete messages are drained and dispatched before returning control to the `select!` loop.
2. **Given** batched dispatch, **When** multiple messages are drained, **Then** each message is spawned as a separate async task (as before) and their completions are collected by `FuturesUnordered`.
3. **Given** batched reads, **When** `cargo test --locked --all-features` runs, **Then** all tests pass.

---

### Edge Cases

- **Encoding failure in async task**: If encoding or chunking fails in the async task, the error must be propagated to the event loop (e.g., close the connection) without panicking or dropping the connection silently.
- **Ordered delivery**: OPC UA sequence numbers must remain monotonic and ordered per chunk. Pre-encoding must preserve the same sequence-number assignment order that the synchronous path would produce.
- **Backpressure**: If `SendBuffer` fills up while the event loop drains incoming messages, the batched read must stop and let the send side catch up.
- **Partial chunks**: If TCP delivers only part of a message (intermediate chunks without the Final chunk), the drain loop must still accumulate chunks correctly using the existing `pending_chunks` mechanism.

## Requirements *(mandatory)*

### Functional Requirements

#### US1 — Decouple Encoding
- **FR-001**: Response encoding (`Chunker::encode_into()`, `SendBuffer::write()`) MUST occur in the async task that produces the response, not synchronously in the event loop's `resp_fut` arm.
- **FR-002**: The event loop MUST receive pre-encoded chunks from completed async tasks and enqueue them into `SendBuffer` without re-encoding.
- **FR-003**: Sequence number assignment for outgoing chunks MUST remain ordered and monotonic, matching the current synchronous behavior.

#### US2 — Batch Incoming Reads
- **FR-004**: `Transport::poll()` or the event loop calling it MUST drain all available complete messages from the TCP stream in a single wake-up.
- **FR-005**: Each drained message MUST be dispatched as an async task (existing `process_request()` path) before the drain loop returns.
- **FR-006**: The `max_inflight_requests_per_connection` limit MUST still be respected — if the limit is reached, the drain loop MUST stop and yield to let completions flush.

### Key Entities

- **Response encoding pipeline**: `Chunker::encode_into()` splits a `ResponseMessage` into `MessageChunk` segments, assigning sequence numbers from a per-connection `SequenceNumberHandle`. Currently called in `enqueue_message_for_send()`. After: called in the async service task.
- **SendBuffer**: Holds a `VecDeque<PendingPayload>` of encoded chunks waiting to be written to TCP. The event loop drains this in `transport.poll()`. After US1: receives pre-encoded chunks directly.
- **Event loop drain**: The `transport.poll()` → `process_request()` path that currently processes one message per iteration. After US2: processes all available messages per iteration.

## Success Criteria *(mandatory)*

- **SC-001**: Localhost benchmark throughput does not regress. Combined US1+US2 should show stable or improved throughput.
- **SC-002**: All existing CI gates pass: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings`, `cargo test --locked --all-features`.
- **SC-003**: Response content, ordering, and chunk sequence numbers are identical to the pre-change implementation.
- **SC-004**: The `max_inflight_requests_per_connection` limit is still enforced after batching.

## Assumptions

- `Chunker::encode_into()` requires only the response message, sequence numbers, and encoding context — all of which are clonable or shareable via `Arc`. The async task can acquire these before encoding.
- The `SendBuffer` API can accept pre-encoded `MessageChunk` structs with pre-assigned sequence numbers.
- Batching in the drain loop doesn't introduce starvation — if send-backpressure builds up, the drain stops and yields to the send path via the existing `can_poll_transport` mechanism.
