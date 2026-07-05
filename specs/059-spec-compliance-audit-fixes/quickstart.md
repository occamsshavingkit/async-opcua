# Quickstart: Spec Compliance Audit Fixes

**Feature**: 059-spec-compliance-audit-fixes
**Date**: 2026-07-05

## Build and Test

```bash
# Build the entire workspace
cargo build

# Run the server crate tests
cargo test -p async-opcua-server

# Run full workspace tests
cargo test --workspace
```

## Files Changed

| File | Fixes |
|------|-------|
| `async-opcua-server/src/session/manager.rs` | SESSION-04 (runtime nonce validation), SESSION-06 (min timeout floor) |
| `async-opcua-server/src/session/controller.rs` | SC-04 (remove redundant set_role) |
| `async-opcua-server/src/session/services/view.rs` | VIEW-02 (BrowseDirection validation) |
| `async-opcua-server/src/node_manager/view.rs` | VIEW-03 (add_unchecked result mask) |
| `async-opcua-server/src/info.rs` | DISC-03 (endpoint URL filter), DISC-04 (locale filter) |
| `async-opcua-server/src/config/server.rs` | SESSION-06 (min_session_timeout_ms config) |

## Verification

To verify each fix independently after implementation:

1. **SESSION-04**: Set `session_nonce_length` to 16 in server config → server should reject at startup/runtime
2. **SESSION-06**: Set `max_session_timeout_ms: 0` and request `0` → server returns `revisedSessionTimeout: 1`
3. **VIEW-02**: Send Browse with `browseDirection: 3` → server returns `BadBrowseDirectionInvalid`
4. **VIEW-03**: Browse with cleared IS_FORWARD bit in resultMask → external references also have `is_forward: false`
5. **DISC-03**: FindServers with non-empty `endpoint_url` → only matching registered servers returned
6. **DISC-04**: FindServers with `locale_ids: ["fr"]` → server returns French name if configured
7. **SC-04**: Code review → `set_role(Role::Server)` at controller.rs:1181 no longer exists
