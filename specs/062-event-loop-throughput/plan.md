# Implementation Plan: Event Loop Throughput

**Branch**: `062-event-loop-throughput` | **Date**: 2026-07-05 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/062-event-loop-throughput/spec.md`

## Summary

Improve per-connection throughput by removing synchronous CPU work from the tokio event loop and reducing scheduler wake-ups. Two changes: (1) move response encoding (`Chunker::encode_into()` + `SendBuffer::write()`) from the event loop into the async task that produces the response, and (2) drain all available incoming messages per `transport.poll()` wake-up instead of processing one at a time.

## Technical Context

**Language/Version**: Rust (edition 2021, workspace resolver = "2")
**Primary Dependencies**: tokio (async runtime, select!, FuturesUnordered), opcua-core (Chunker, SendBuffer, MessageChunk, SecureChannel), tracing (instrumentation)
**Storage**: N/A (in-memory message processing, no persistence)
**Testing**: cargo test --locked --all-features, cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings
**Target Platform**: Linux server (localhost benchmark via tokio/TCP)
**Project Type**: library (workspace of 15+ crates) consumed as a network server
**Performance Goals**: Stable or improved localhost benchmark throughput (no regression); reduced CPU per-request from eliminated event-loop synchronous encoding
**Constraints**: Response ordering and sequence number monotonicity must be preserved; `max_inflight_requests_per_connection` limit still enforced; identical response content to pre-change
**Scale/Scope**: Two surgical changes to the transport/event-loop layer: ~3 files modified (controller.rs, transport/tcp.rs, buffer.rs)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Status |
|-----------|-----------|--------|
| I. Correctness Over Completion | Sequence number ordering is correctness-critical. The encoding refactor must preserve monotonic, ordered assignment. Full test suite must pass after each change. | PASS |
| II. Do It Right Once | Both changes are architecturally minimal: US1 moves encoding to where the response is produced, US2 drains the buffer in-place. No new abstractions or indirections. | PASS |
| III. Individual Task Discipline | US1 and US2 are independent — they touch different code paths and can be implemented and verified separately. | PASS |
| IV. Security Is Paramount | Encoding and buffering are internal; no new network-facing paths, no changes to crypto, decode, or auth. Response content is unchanged. | PASS |
| V. Leave It Better Than You Found It | Eliminates unnecessary synchronous CPU work from the event loop. Makes the architecture cleaner (encoding happens where responses are produced). | PASS |

**Gate Result**: All principles pass. No violations to justify.

## Project Structure

### Documentation (this feature)

```text
specs/062-event-loop-throughput/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output — internal refactoring, no external contracts
└── tasks.md             # Phase 2 output (/speckit.tasks command — NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
async-opcua-core/src/comms/
├── buffer.rs             # US1: SendBuffer::write() / Chunker::encode_into() called here
├── chunker.rs            # US1: Chunker::encode_into() — the encoding target
└── message_chunk.rs      # MessageChunk type passed to SendBuffer

async-opcua-server/src/
├── session/
│   └── controller.rs     # US1: Move encode_into() from resp_fut arm into async task
│                         # US2: Batch-drain transport.poll() results
└── transport/
    └── tcp.rs            # US1: ConnectionTransport trait methods (enqueue_message_for_send, poll)
                          # US2: poll_inner() drain loop
```

**Structure Decision**: No new files or crates needed. Changes are localized to the existing transport/controller boundary. `async-opcua-core` may need minor API additions on `SendBuffer` if pre-encoded chunk insertion is required.

## Complexity Tracking

No violations to justify. All constitution principles pass without exceptions.
