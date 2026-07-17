# Implementation Plan: GDS Pull Model Client-Side Fix (Run 2)

**Branch**: `105-gds-pull-client-fix` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/105-gds-pull-client-fix/spec.md`

## Summary

`async-opcua-client/src/gds/` hardcodes fabricated namespace-0 NodeIds for a GDS
Directory object, `RegisterApplication`, `StartSigningRequest`, and
`FinishSigningRequest` (misnamed -- the real method is `FinishRequest`,
shared by both signing-request and new-key-pair flows). This client talks to
an *external* GDS product, so every real deployment assigns the GDS
companion namespace its own index -- hardcoded constants can never work.
Replace them with real, dynamic discovery: read the target server's
namespace array to find the GDS companion namespace's index, then resolve
the Directory object and its methods via `TranslateBrowsePathsToNodeIds`
(OPC UA Part 4 §5.8.4) -- the standard mechanism for exactly this "find a
well-known node whose namespace index is unknown in advance" scenario.
`Session::get_namespace_index`/`translate_browse_paths_to_node_ids` already
exist on this project's client `Session` type; no new session-level
capability needs to be built.

## Technical Context

**Language/Version**: Rust 2021, workspace crate `async-opcua-client`
**Primary Dependencies**: `opcua-types` (`NodeId`, `BrowsePath`, `RelativePath`), existing `Session::get_namespace_index`/`translate_browse_paths_to_node_ids`
**Storage**: N/A (discovered NodeIds held in-memory on the `GdsClient` instance)
**Testing**: `cargo test` — new integration test spins up a real `async-opcua-server`
(dev-dependency already present in `async-opcua-client/Cargo.toml`) with the GDS
companion NodeSet imported at a deliberately non-default namespace index, connects
a real client, and exercises discovery + all three Call paths end-to-end
**Target Platform**: Linux (matches existing project CI matrix)
**Project Type**: Library (OPC UA client SDK)
**Performance Goals**: N/A — discovery is a one-time, per-client-instance cost, not a hot path
**Constraints**: Zero change to `register_application`/`request_signing_csr`/`poll_signing_request`'s
public signatures; zero server-side change; fail closed (no panics, no silent fallback to a guessed NodeId)
**Scale/Scope**: Rewrite of `async-opcua-client/src/gds/registration.rs` and
`csr.rs`'s NodeId-holding fields + constructors; a new discovery function on
`GdsClient`; no new files beyond the integration test.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: This feature exists because the current code is
  provably non-functional against any real GDS (hardcoded NodeIds that were never
  valid). Fixing it with real discovery — not another guess — is the point. PASS.
- **II. Do It Right Once**: Also corrects a second, related defect while in this
  code: `GdsCsrClient::certificate_manager_id` encodes the wrong architectural
  assumption (a separate "CertificateManager" object) that this SDK's own
  corrected server-side model (feature 104) already disproved — `StartSigningRequest`/
  `FinishRequest` live on the same Directory object `RegisterApplication` does.
  Renamed to `directory_object_id` rather than left wrong under a technically-unused
  discovery layer. PASS.
- **III. Individual Task Discipline**: Tasks below are scoped one file/concern at a
  time (registration.rs, csr.rs, gds_client.rs's new discovery function, tests). PASS.
- **IV. Security Is Paramount**: Discovery reads only public address-space metadata
  (namespace array, browse paths) over the already-authenticated/encrypted session
  channel — no new attacker-facing surface. Fails closed on any missing node
  (`Result`, never panics, never guesses). PASS.
- **V. Leave It Better Than You Found It**: Removes the fabricated-constant
  `Default`/`new()` footguns entirely rather than leaving them as a trap alongside
  the new discovery path. PASS.

No violations. No Complexity Tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/105-gds-pull-client-fix/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-client/
├── src/gds/
│   ├── mod.rs           # Unchanged exports
│   ├── gds_client.rs    # New: discover(session) -> Result<Self, Error>
│   ├── registration.rs  # Rewritten: no hardcoded Default/new(); explicit-NodeId constructor
│   ├── csr.rs            # Rewritten: certificate_manager_id -> directory_object_id, same treatment
│   └── gds_state.rs      # Unchanged
├── tests/
│   └── gds_pull_client_discovery.rs   # New: real client vs. real server, non-default namespace index
```

**Structure Decision**: No new crates, no new modules beyond the one new
integration test file. Targeted rewrite within the existing `gds/` module
layout, mirroring the server-side precedent (`gds/directory_instance.rs`)
of resolving real nodes rather than constructing/assuming them.

## Complexity Tracking

*No violations — table omitted.*
