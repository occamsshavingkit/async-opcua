# Implementation Plan: Complexity Cuts (2a, 2b, 6, 7, 8)

**Branch**: `056-complexity-cuts` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `specs/056-complexity-cuts/spec.md`

## Summary

Five independent, small-scope complexity reductions in the OPC-UA server hot paths:
- **2a**: `is_subtype_of` memoization via `moka` cache (type tree immutable per OPC 10000-3 §4.10.1 single-inheritance guarantee)
- **2b**: `(parent, BrowseName)` index for TranslateBrowsePathsToNodeIds (OPC 10000-4 BrowseNext service)
- **6**: Per-channel unactivated-session counter for CreateSession (OPC 10000-4 §5.7.2 session-channel binding)
- **7**: Subscription priority cache aligned with OPC 10000-4 §5.14.2.2 (highest-priority-first, round-robin for equals)
- **8**: Single-parse ChunkInfo per OPC 10000-6 §6.7.2.2 (MessageHeader + SecurityHeader + SequenceHeader)

Each cut preserves existing behavior exactly. All five are independent and mergeable separately.

## Technical Context

**Language/Version**: Rust 1.75+ (workspace edition 2021)
**Primary Dependencies**: `moka` 0.12 (already in `async-opcua-server`), `hashbrown`, `dashmap`, `tokio`
**Storage**: N/A (in-memory caches/indices only, no persistence)
**Testing**: `cargo test -p async-opcua-server --lib` (unit + integration), `cargo clippy --workspace`
**Target Platform**: Linux server (cross-platform Rust, server profile)
**Project Type**: Protocol library (OPC UA server stack consumed by downstream systems)
**Performance Goals**: Bounded O(1) or O(log n) per hot-path operation; server test suite must pass before/after
**Constraints**: Zero behavioral change; must not regress existing test suite; must not introduce new allocations on steady-state hot path
**Scale/Scope**: 5 files changed across 3 crates (`async-opcua-nodes`, `async-opcua-core`, `async-opcua-server`)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| **I. Correctness Over Completion** | PASS | Each cut is verified by existing test suite; behavioral identity is the acceptance criterion. No cut changes observable output. |
| **II. Do It Right Once** | PASS | No shortcuts: each cut uses a well-understood data structure (moka cache, HashMap index, AtomicUsize counter, BTreeSet cache, in-place ChunkInfo reuse). No copy-paste. |
| **III. Individual Task Discipline** | PASS | Five independent cuts, each a single commit. Each is independently testable and reversible. |
| **IV. Security Is Paramount** | PASS | No cut introduces new panics, allocations on attacker-controlled input, or weakens existing bounds. Cut 8 (chunker) is on the decode path; the reuse preserves existing validation. Cut 6 counter uses AtomicUsize (no overflow at realistic scale). |
| **V. Leave It Better Than You Found It** | PASS | Each cut removes unnecessary work with smaller, clearer data flow. No dead code left behind. |

**Gate verdict**: PASS. All principles satisfied. Proceed to Phase 0.

## Project Structure

### Documentation (this feature)

```text
specs/056-complexity-cuts/
├── plan.md              # This file
├── research.md          # Phase 0: OPC-UA standard grounding + design rationale
├── data-model.md        # Phase 1: Entity models for caches/indices
├── quickstart.md        # Phase 1: How to implement each cut
├── contracts/           # Phase 1: Interface contracts (none — all internal refactors)
└── tasks.md             # Phase 2: /speckit.tasks output
```

### Source Code (repository root)

Each cut touches specific files within the workspace crates:

```text
async-opcua-nodes/
└── src/
    └── type_tree.rs              # Cut 2a: add moka cache to DefaultTypeTree

async-opcua-core/
└── src/
    └── comms/
        ├── chunker.rs            # Cut 8: single-parse ChunkInfo across validate+decode
        └── message_chunk.rs      # Cut 8: ChunkInfo storage on MessageChunk

async-opcua-server/
└── src/
    ├── node_manager/
    │   ├── view.rs               # Cut 2b: BrowsePath index for impl_translate_browse_paths_using_browse
    │   └── memory/
    │       └── mod.rs            # Cut 2b: address-space mutation hooks for index invalidation
    ├── session/
    │   └── manager.rs            # Cut 6: per-channel unactivated-session counter
    └── subscriptions/
        └── session_subscriptions.rs  # Cut 7: priority cache (BTreeSet + dirty flag)
```

**Structure Decision**: This is a library-internal refactor. No new crates, modules, or public API changes. Each cut modifies existing structs and functions within their owning crate.

## Complexity Tracking

> No constitution violations to justify. All cuts simplify existing code by replacing O(n²)/O(n log n)/2× work with O(1) or amortized O(1) equivalents.
