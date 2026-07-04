# Contracts: Complexity Cuts

**Feature**: 056-complexity-cuts
**Date**: 2026-07-03

## Nature of these changes

All five cuts are **internal refactors** with zero public API changes. No new traits, no new public functions, no changes to existing function signatures (except Cut 8 which changes `chunk_info` from `&self` to `&mut self` — internal to the crate).

The existing public API contracts are preserved:

| Crate | Public API | Affected? |
|-------|-----------|-----------|
| `async-opcua-nodes` | `TypeTree` trait, `DefaultTypeTree::new()`, `is_subtype_of()` | No — same trait impl, same results |
| `async-opcua-core` | `Chunker::encode()`, `Chunker::decode()`, `MessageChunk` | No — same input/output behavior |
| `async-opcua-server` | `SessionManager`, `NodeManager`, subscription management | No — all cuts are internal to private methods |

## Behavioral contracts

Each cut preserves observable behavior:

1. **Cut 2a**: `is_subtype_of(child, ancestor)` returns identical boolean for all inputs.
2. **Cut 2b**: `impl_translate_browse_paths_using_browse` returns identical `BrowsePathResult` for all inputs.
3. **Cut 6**: `commit_create_session_draft` returns identical `Result<CreateSessionResponse, StatusCode>` for all inputs. The limit enforcement is identical.
4. **Cut 7**: `tick_subscriptions_with_publish_requests` processes subscriptions in the same order and produces identical `PublishResponse` sequences.
5. **Cut 8**: `Chunker::validate_chunk_sequence` and `Chunker::decode` return identical results. All error paths preserved.

## Verification contract

```bash
# Every cut: existing tests must pass before and after
cargo test -p async-opcua-nodes --lib      # Cut 2a
cargo test -p async-opcua-server --lib     # Cuts 2b, 6, 7
cargo test -p async-opcua-core --lib       # Cut 8
```
