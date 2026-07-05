# Data Model: Spec Compliance Audit Fixes

**Feature**: 059-spec-compliance-audit-fixes
**Date**: 2026-07-05

## Overview

This feature makes no new data model changes. All fixes are validation, filtering, or code hygiene improvements within existing entity types. The entities involved are documented below for completeness.

## Affected Existing Entities

### Session
- **Field affected**: `revisedSessionTimeout` — new minimum-floor guard (SESSION-06)
- **Field affected**: `serverNonce` — runtime validation of generated length (SESSION-04)
- No schema change. No new fields.

### SecureChannel
- **Method removed**: `set_role(Role::Server)` redundant call in OpenSecureChannel handler (SC-04)
- No schema change. No new fields.

### BrowseNode
- **New validation**: `BrowseDirection::INVALID` rejected with `BadBrowseDirectionInvalid` (VIEW-02)
- **Method enhanced**: `add_unchecked()` now applies result mask field stripping (VIEW-03)
- No schema change. No new fields.

### ServerInfo
- **New config field**: `min_session_timeout_ms: u64` with default `1` (SESSION-06)
- **Method enhanced**: `registered_application_descriptions()` applies endpoint_url filtering (DISC-03)
- **Method enhanced**: `find_servers_application_description()` applies locale filtering (DISC-04)

## Configuration Change

### ServerConfig (async-opcua-server/src/config/server.rs)

```rust
/// Minimum session timeout in milliseconds (must be > 0 per OPC-10000-4 §5.7.2.2)
/// Default: 1
pub min_session_timeout_ms: u64,
```

This field prevents `revisedSessionTimeout` from reaching 0, ensuring spec compliance.
