# Contracts: Spec Compliance Audit Fixes

**Feature**: 059-spec-compliance-audit-fixes  
**Date**: 2026-07-05

## Overview

This feature has no external facing interface contracts. All changes are internal to the async-opcua-server crate. No new public APIs, CLI commands, network protocols, or service endpoints are introduced.

## Internal Behavior Changes

These are the behavioral contract changes observable to OPC UA clients, documented for conformance testing:

| OPC UA Service | Change | Expected Client-Observable Behavior |
|---------------|--------|-------------------------------------|
| Browse | BrowseDirection::INVALID now rejected | Client receives `BadBrowseDirectionInvalid` instead of empty results |
| Browse | External references respect resultMask | Client no longer receives unmasked fields when not requested |
| FindServers | Endpoint URL filtering for registered servers | Client only receives servers accessible at requested URL |
| FindServers | Locale-filtered own server name | Client receives locale-appropriate application name |
| CreateSession | Timeout minimum enforcement | Client never receives `revisedSessionTimeout: 0` |
| CreateSession | serverNonce runtime validation | Server refuses to start with out-of-range nonce config |
| OpenSecureChannel | No redundant set_role | No behavioral change (code hygiene) |
| CloseSecureChannel | No behavioral change | Documented: resource cleanup follows existing async-drop pattern |
