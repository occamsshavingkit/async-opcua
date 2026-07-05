# Data Model: Event Loop Throughput

## Overview

This feature is a refactoring of the internal transport/controller boundary. No new domain entities are introduced. The data model documents the existing types being modified and the new intermediate types needed.

## Existing Types (Unchanged)

### `SessionController<T: ConnectionTransport>` (controller.rs:113-127)

```rust
pub(crate) struct SessionController<T: ConnectionTransport> {
    channel: SecureChannel,
    transport: T,
    // ...
    pending_messages: FuturesUnordered<Pin<Box<PendingMessageResponse>>>,
    max_inflight: usize,
    // ...
}
```

**Modification**: The type of `pending_messages` may change from `PendingMessageResponse` to include pre-encoded chunks, or an additional field for encoded chunk queue.

### `ConnectionTransport` trait (tcp.rs:37-59)

```rust
pub(crate) trait ConnectionTransport: Send + 'static {
    fn enqueue_message_for_send(&mut self, channel: &mut SecureChannel,
        message: ResponseMessage, request_id: u32) -> Result<(), StatusCode>;
    fn poll(&mut self, channel: &mut SecureChannel)
        -> impl Future<Output = TransportPollResult> + Send;
    // ...
}
```

**Modification**: US2 changes `poll()` return type from single-message `TransportPollResult` to batch-capable variant.

### `TransportPollResult` (tcp.rs)

```rust
// CURRENT
pub enum TransportPollResult {
    IncomingChunk,
    IncomingMessage(Request),
    RecoverableError(StatusCode, u32, u32),
    Error(StatusCode),
    OutgoingMessageSent,
    Closed,
}

// AFTER (US2 batch):
pub enum TransportPollResult {
    IncomingChunk,
    IncomingMessages(Vec<Request>),  // Changed from single to batch
    RecoverableError(StatusCode, u32, u32),
    Error(StatusCode),
    OutgoingMessageSent,
    Closed,
}
```

### `SendBuffer` (buffer.rs:111-134)

```rust
pub struct SendBuffer {
    buffer: Cursor<Vec<u8>>,
    chunk_storage: bytes::BytesMut,
    chunk_scratch: Vec<MessageChunk>,
    chunks: VecDeque<PendingPayload>,
    last_request_id: u32,
    sequence_numbers: SequenceNumberHandle,
    pub max_message_size: usize,
    pub max_chunk_count: usize,
    pub send_buffer_size: usize,
    state: SendBufferState,
}
```

**Modification**: New method `push_encoded_chunks()` to accept pre-encoded chunks.

### `PendingPayload` (buffer.rs:35-39)

```rust
enum PendingPayload {
    Chunk(MessageChunk),
    Ack(AcknowledgeMessage),
    Error(ErrorMessage),
}
```

**No change**: Pre-encoded chunks are still inserted as `PendingPayload::Chunk`.

## New / Modified Types

### `EncodedResponse` (new, controller.rs)

```rust
/// A response whose message has already been encoded into chunks.
/// Carries pre-assigned sequence numbers.
struct EncodedResponse {
    chunks: Vec<MessageChunk>,
    chunk_count: u32,
    request_id: u32,
}
```

**Purpose**: Replaces `Response { message: ResponseMessage, request_id }` in the output of async tasks. The event loop pushes these chunks directly into `SendBuffer`.

### `PendingMessageResponse` (modified, controller.rs)

```rust
// CURRENT
type PendingMessageResponse = dyn Future<Output = Result<Response, String>> + Send + Sync + 'static;

// AFTER (US1):
type PendingMessageResponse = dyn Future<Output = Result<EncodedResponse, String>> + Send + Sync + 'static;
```

**Purpose**: Async tasks now produce pre-encoded chunks instead of an unencoded `ResponseMessage`.

### `SendBuffer::push_encoded_chunks()` (new method, buffer.rs)

```rust
impl SendBuffer {
    /// Push pre-encoded chunks into the pending queue.
    /// The caller must have already assigned sequence numbers and incremented
    /// the handle by `chunk_count`. This method does NOT call Chunker::encode_into().
    pub fn push_encoded_chunks(&mut self, chunks: Vec<MessageChunk>, chunk_count: u32) {
        self.sequence_numbers.increment(chunk_count);
        self.chunks.extend(chunks.into_iter().map(PendingPayload::Chunk));
    }
}
```

## State Transitions

The encoding state machine for a single response:

```
CURRENT:
  Request arrives → process_request() → HandleMessageResult::AsyncMessage
  → spawn async task into pending_messages
  → async task produces Response { message: ResponseMessage, request_id }
  → event loop: transport.enqueue_message_for_send(channel, msg, id)
    → SendBuffer::write() calls Chunker::encode_into()
    → chunks pushed as PendingPayload::Chunk

AFTER (US1):
  Request arrives → process_request() → HandleMessageResult::AsyncMessage
  → spawn async task into pending_messages (with encoding context)
  → async task calls Chunker::encode_into() internally
  → async task produces EncodedResponse { chunks, chunk_count, request_id }
  → event loop: transport.push_encoded_chunks(chunks, chunk_count)
    → chunks pushed as PendingPayload::Chunk (no re-encoding)
```

The chunk encryption (`secure_channel.apply_security()`) still happens inside `SendBuffer::encode_chunks_to_buffer()` when data is actually written to the wire — this is unchanged.
