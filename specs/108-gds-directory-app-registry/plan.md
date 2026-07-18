# Implementation Plan: GDS Directory Application-Registry Services

**Branch**: `108-gds-directory-app-registry` | **Date**: 2026-07-18 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/108-gds-directory-app-registry/spec.md`

## Summary

Implement DirectoryType's application-registry methods (Part 12 v1.05.07 §6.5.4/6.5.6-6.5.11) --
`RegisterApplication`, `UpdateApplication`, `UnregisterApplication`, `GetApplication`,
`FindApplications`, `QueryApplications`, and the deprecated `QueryServers` -- closing CUs 2232 and
3581. Extends the existing `GdsApplicationRecord`/`GdsPullMethodRegistry` (not a second registry)
with the real `ApplicationRecordDataType` fields, a monotonically increasing per-record identifier
for pagination, and a new small LIKE-operator matcher (Part 4 Table 120) for the string filters.
`ApplicationRecordDataType` itself is hand-authored (mirroring `samples/custom-codegen`'s
established pattern for a non-core-namespace custom type + its own `TypeLoader`), since the
vendored companion NodeSet doesn't ship a generated binding for it and, per research.md R8, doesn't
even define the classic encoding-object metadata a fully spec-conformant binding would derive from
-- a real, documented limitation of the local NodeSet export, not an implementation shortcut.
Explicitly out of scope (see spec.md Assumptions): Authorization Service, KeyCredential Service,
JWT/OAuth2 discovery, LDS-ME connectivity, and real certificate revocation on unregister (needs
CU 3582's not-yet-built ledger/CRL infrastructure).

## Technical Context

**Language/Version**: Rust (workspace MSRV, matches rest of `async-opcua-server`)
**Primary Dependencies**: `async-opcua-types` (`ApplicationDescription`, `ServerOnNetwork`,
`ApplicationType` -- all already generated; `TypeLoaderInstance`/`TypeLoader`/`ExpandedMessageInfo`
for the new hand-authored `ApplicationRecordDataType`), `async-opcua-macros`
(`#[derive(BinaryEncodable, BinaryDecodable, ...)]`), existing `gds::pull_methods`/
`gds::directory_instance` modules, existing `rbac::WellKnownRole`
**Storage**: In-memory only, extending the existing `moka`-backed `GdsPullMethodRegistry` --
no persistence across restarts (spec.md Assumption)
**Testing**: `cargo test` (registry unit tests + LIKE-matcher unit tests + a real client/server
end-to-end test extending the `gds_pull_companion_integration.rs` harness), `cargo clippy
--all-targets --all-features`, `cargo fmt --all -- --check`
**Target Platform**: Same as rest of workspace
**Project Type**: Library (Rust workspace crate feature addition)
**Performance Goals**: No new performance requirement; registry operations are simple
capacity-bounded cache lookups, matching the existing Pull-model registry's own performance profile
**Constraints**: Must not build Authorization Service/KeyCredential Service/JWT-discovery
infrastructure (out of scope, per spec.md); must not silently skip the certificate-revocation
requirement in UnregisterApplication -- document it, matching CU 3582's existing deferral
**Scale/Scope**: 7 new method handlers, 1 hand-authored companion type + its `TypeLoader` (both
client- and server-side registration), 1 new LIKE-matcher module, extension of 1 existing registry
struct, extension of `DirectoryInstanceNodeIds` with 8 fields; no new crate

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Every method's semantics grounded against the real Part 12
  PDF text (research.md R2), not assumption or the backlog summary; the R8 finding (vendored
  NodeSet lacks encoding-object metadata) was surfaced and resolved with a documented, honest
  limitation rather than silently faked or ignored. PASS.
- **II. Do It Right Once**: Extends the existing `GdsApplicationRecord`/registry rather than
  building a parallel store; reuses the established `samples/custom-codegen` pattern for the
  hand-authored type rather than inventing a new mechanism or reaching for the much heavier,
  zero-precedent `DynamicStructure` machinery when a simpler, already-proven path exists. PASS.
- **III. Individual Task Discipline**: Tasks (next phase) one-per-line, matching established
  pattern from every prior GDS feature (101-105). PASS (verified at /speckit-tasks).
- **IV. Security Is Paramount**: Write methods (`Register`/`Update`/`Unregister`) gated by
  `WellKnownRole::SecurityAdmin`, matching this module's own established RBAC simplification
  (research.md R4); registry remains capacity-bounded (existing `GDS_REGISTRY_CAPACITY` eviction),
  so a flood of registration attempts still can't grow memory unbounded; the LIKE-matcher (R3) is a
  bounded string-scan with no backtracking blowup risk (no nested quantifiers in this grammar)
  reviewed for pathological-input cost during implementation. PASS.
- **V. Leave It Better Than You Found It**: Documents (does not silently skip) two real,
  spec-mandated behaviors this feature cannot fully close yet -- certificate revocation on
  unregister (R6) and audit-event emission (R7) -- both tied to infrastructure gaps already
  tracked elsewhere (CU 3582) or newly noted in TODO.md, rather than a half-built approximation of
  either. PASS.

No violations requiring justification in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/108-gds-directory-app-registry/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

No `contracts/` directory: the OPC UA wire contract for these DirectoryType methods already exists
(defined by the companion spec); only the server-side implementation and a small client-side
hand-authored type binding are new.

### Source Code (repository root)

```text
async-opcua-server/src/gds/
├── directory_instance.rs        # + 8 resolved NodeIds (RegisterApplication/QueryServers/
│                                #   QueryApplications/FindApplications/UpdateApplication/
│                                #   UnregisterApplication/GetApplication/Applications folder)
├── application_record.rs        # NEW: hand-authored ApplicationRecordDataType + its TypeLoader
├── like_match.rs                # NEW: Part 4 Table 120 LIKE-operator matcher
└── pull_methods/
    └── mod.rs                   # extend GdsApplicationRecord; + 7 new method handlers +
                                  #   registration wiring

async-opcua-server/tests/
└── gds_pull_companion_integration.rs   # extended with the new methods' end-to-end test

async-opcua-client/ (no source change expected beyond what the end-to-end test needs --
                      Session::add_type_loader already exists, used directly in the test)
```

**Structure Decision**: Extends the existing `gds` module family in place, following the exact
file-organization precedent features 101-105 already established (one focused new file per
genuinely new concern -- the hand-authored type and the LIKE-matcher each get their own file;
everything else extends existing files).

## Complexity Tracking

> No Constitution Check violations -- section intentionally left without entries.
