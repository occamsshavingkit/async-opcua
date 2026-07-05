# Research: Event Loop Throughput

## US1 — Decouple Response Encoding From the Event Loop

### Current Architecture

The event loop in `SessionController::run()` (controller.rs:225-292) uses a `tokio::select!` with 4 branches. The `resp_fut` arm (line 247-268) receives a completed `Response { message, request_id }` from `FuturesUnordered` and calls `self.transport.enqueue_message_for_send()` which synchronously:

1. Calls `self.send_buffer.write(request_id, message, channel)` (buffer.rs:221)
2. Inside `write()`, calls `Chunker::encode_into()` (buffer.rs:257) which does binary encoding, chunking, and encryption of the `ResponseMessage`
3. Appends encoded chunks as `PendingPayload::Chunk` to the send buffer's `VecDeque<chunks>`
4. Increments `self.sequence_numbers.increment(chunk_count)`

This synchronous encoding blocks the event loop from reading new TCP data or collecting other completed responses during the encoding+chunking CPU work.

### Decision: Pre-encode responses in the async task, pass pre-encoded chunks to the event loop

**Rationale**: The async task already has the `ResponseMessage`, `request_id`, and (via captured context) access to the `SecureChannel`, sequence numbers, and encoding parameters. Encoding inside the task means the event loop only enqueues pre-encoded chunks into `SendBuffer`, which is O(1) push operations vs. O(n) encoding work.

**Design**:
1. The async task acquires cloned handles: `SequenceNumberHandle` (cloneable), `SecureChannel` reference or relevant encoding context, `max_message_size`, `send_buffer_size`
2. The task calls `Chunker::encode_into()` producing `Vec<MessageChunk>` with pre-assigned sequence numbers
3. The encoded chunks replace `ResponseMessage` in the completion payload: `Response(ResponseMessage, u32)` → `EncodedResponse(Vec<MessageChunk>, u32, u32)` where the extra u32 is `chunk_count` for sequence number tracking
4. The event loop receives `EncodedResponse`, pushes chunks into `SendBuffer` (requires new method `push_encoded_chunks()`), and increments sequence numbers by chunk_count
5. All chunk data lives in BytesMut storage on the transport — the async task must have its own `BytesMut` allocation for zero-copy semantics, or `MessageChunk` must own its data

**Key challenge**: `MessageChunk` currently holds zero-copy `&[u8]` slices into `SendBuffer`'s `chunk_storage: BytesMut`. Moving encoding out of `SendBuffer` means those slices must point to storage owned by the async task or the transport. Solutions:
- **Option A**: Each async task allocates its own `BytesMut` for chunk storage. Simpler but allocates per-response.
- **Option B**: The transport exposes a pool of `BytesMut` buffers that async tasks can borrow. More complex but zero-allocation at steady state.
- **Option C**: `MessageChunk` changes from borrowed `&[u8]` to owned `Vec<u8>`. Higher allocation cost but simplest API.

**Recommendation**: Option A — allocate per-response. The localhost benchmark already sees 90k req/sec, and per-response allocation of a few KB for chunk data is acceptable. If profiling shows allocation overhead, pool-based reuse (Option B) can be added later.

**Alternatives considered**:
- Keep encoding in event loop but spawn it as a separate task: Adds an extra tokio spawn and channel per response, increasing scheduler overhead. Worse than moving it into the existing response-producing task.
- Use a dedicated encoding thread: Overly complex for a single-threaded tokio runtime. Would require cross-thread channel.
- Pre-encode into the send buffer synchronously but use `tokio::task::block_in_place()`: `block_in_place` allows blocking work on the current thread without starving other tasks — but the controller owns `&mut self` and `&mut self.transport`, so other event-loop work is already blocked by Rust's borrow rules.

### Decision: Add `push_encoded_chunks()` method to `SendBuffer`

**Rationale**: `SendBuffer::write()` currently both encodes and queues. For pre-encoded chunks, we need a method that only queues. The method signature:

```rust
/// Push pre-encoded chunks into the send buffer without re-encoding.
/// The caller is responsible for ensuring sequence numbers are already
/// assigned and incremented appropriately.
pub fn push_encoded_chunks(&mut self, chunks: Vec<MessageChunk>, chunk_count: u32) {
    self.sequence_numbers.increment(chunk_count);
    self.chunks.extend(chunks.into_iter().map(PendingPayload::Chunk));
}
```

**Alternatives considered**: Expose `self.chunks` as `pub`. Violates encapsulation and would allow bypassing state checks.

---

## US2 — Batch Incoming Message Reads

### Current Architecture

`Transport::poll_inner()` (tcp.rs:416-458) processes ONE message per call:
1. If send buffer has data, writes it to TCP AND reads ONE incoming message (via `tokio::select!`)
2. If send buffer is empty, reads ONE incoming message
3. Each message goes through `handle_incoming_message()` → `process_message()` → chunk reassembly → `TransportPollResult::IncomingMessage`
4. The event loop then calls `process_request()` and may spawn an async task

At high throughput, this means one `select!` evaluation and one tokio scheduler wake-up per message.

### Decision: Drain all available complete messages in `poll_inner()` before returning

**Rationale**: After receiving one complete message from the TCP stream, immediately try to read the next one. The `TcpCodec` (via `FramedRead`) will return `None` or `Pending` when no more complete messages are available. This batch-drains available messages with minimal additional CPU cost (just looping on `self.read.next()`).

**Design**:
1. After `process_message()` returns `Ok(Some(message))` (a complete message), check if more data is immediately available
2. Loop calling `self.read.try_next()` (non-blocking) or `self.read.next().now_or_never()` to drain additional complete messages without awaiting
3. Stop the loop when:
   - No more complete messages available (returns `Pending`)
   - `max_inflight_requests_per_connection` limit is reached
   - The stream returns `None` (connection closed) or an error

**Key challenge**: `FramedRead` from tokio-util doesn't expose `try_next()` directly. Solutions:
- **Option A (Recommended)**: Use `poll_next` via `Pin::new(&mut self.read).poll_next(cx)` inside a manual loop. Requires a `Context` / `Waker`.
- **Option B**: Wrap the loop in a `futures::future::poll_fn` that polls the codec repeatedly.
- **Option C**: Change `TransportPollResult::IncomingMessage` to `TransportPollResult::IncomingMessages(Vec<Request>)` — the controller then spawns tasks for all messages in one batch.

**Recommendation**: Option C — change the return type to return a batch of messages. The transport poll drains available messages into a `Vec<Request>`, and the controller iterates over them calling `process_request()` for each. This is the cleanest API change and lets the controller decide how many tasks to spawn (respecting `max_inflight`).

**Alternatives considered**:
- Keep returning one message but loop in the controller: More controller code changes but simpler transport API. However, it wastes tokio scheduler wake-ups between messages in the same TCP read.
- Use `tokio::io::Interest` and `try_read`: Not directly applicable since we're using `FramedRead` (a codec-based stream).

### Decision: Update `TransportPollResult` to support batched messages

**Rationale**: Changing the return type to `IncomingMessages(Vec<Request>)` is a natural API evolution. The transport drains everything available, and the controller processes the batch.

**Design for `poll_inner()`**:
```rust
async fn poll_inner(&mut self, channel: &mut SecureChannel) -> TransportPollResult {
    // ... existing send buffer logic ...
    
    // Collect incoming messages
    let mut messages = Vec::new();
    loop {
        match self.read.next().now_or_never() {
            Some(Some(Ok(message))) => {
                match self.process_message(message, channel) {
                    Ok(Some(msg)) => messages.push(msg),
                    Ok(None) => continue,  // intermediate chunk
                    Err(e) => { /* handle error */ break; }
                }
            }
            _ => break,  // No more data or stream closed
        }
    }
    
    if messages.is_empty() { /* wait for one */ }
    else { TransportPollResult::IncomingMessages(messages) }
}
```

**Alternatives considered**:
- `try_next()` on the framed reader: Not available in tokio-util's `StreamExt` for `FramedRead`. Would need to use `poll_next` directly with a noop waker, which is fragile.
- Async loop with `select!` for each message: Adds latency — each message still requires a scheduler round-trip.

---

## Shared Concerns

### Decision: Sequence Number Ordering Must Be Preserved

**Rationale**: OPC UA requires monotonic, ordered sequence numbers per chunk (OPC-10000-6 §6.7.2). Pre-encoding in async tasks means multiple tasks could encode concurrently. Sequence numbers must be assigned atomically.

**Design**: Each async task acquires a `SequenceNumberHandle` clone, calls `Chunker::encode_into()` which uses the handle for sequence numbers, and records the `chunk_count`. The event loop increments the sequence number by `chunk_count` when pushing chunks. This preserves ordering because:
- Sequence numbers are assigned during encoding within the async task
- The async task has exclusive access to its clone of the handle
- The handle's `increment()` is called by the event loop after pushing, not by the task

**Correctness verification**: The integration test suite's response comparison tests catch sequence number mismatches. The `secure_channel.rs` and `chunk.rs` tests verify encode/decode round-trip with correct sequence numbers.

### Decision: max_inflight Enforcement Must Span Both US1 and US2

**Rationale**: `max_inflight_requests_per_connection` limits how many requests are in-flight simultaneously. Both US1 (more async tasks encoding) and US2 (more messages spawned per poll) could increase the number of in-flight requests.

**Design**: The existing check `self.max_inflight == 0 || self.pending_messages.len() < self.max_inflight` (controller.rs:228) already gates `transport.poll()`. For US2 batching, each message spawned increments `pending_messages.len()`. The drain loop in the controller must check this limit after each spawned task and stop if the limit is reached.
