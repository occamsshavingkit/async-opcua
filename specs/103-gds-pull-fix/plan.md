# Implementation Plan: GDS Pull Model Fix (Run 1)

**Branch**: `103-gds-pull-fix` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/103-gds-pull-fix/spec.md`

## Summary

Fix CU 2230 by building a real, working `CertificateDirectoryType` Pull-model
surface (OPC-10000-12 §7.9), replacing `gds/pull_methods.rs`'s current
implementation of the wrong concepts (`GetRejectedList`/`UpdateCertificate`,
Push-model methods) against fabricated NodeIds. Since `CertificateDirectoryType`
doesn't exist in this project's generated core nodeset at all -- it lives only
in the (currently entirely dormant) `companion-gds` companion NodeSet -- this
run also wires that companion import into the server (opt-in) and builds
targeted logic to instantiate a real "Directory" object from the imported
type, since no pre-built singleton instance ships with the companion XML
(unlike Run 1/2's `ServerConfigurationType`, which does).

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`, stable toolchain)
**Primary Dependencies**: `async-opcua-server` (`companion` module -- made reachable, `SimpleNodeManager`/`AddressSpace`, `ObjectBuilder`/`MethodBuilder`/`VariableBuilder`), `async-opcua-nodes` (`NodeSet2Import`, `NamespaceMap`), `async-opcua-crypto` (`X509`, reusing Run 1's signing patterns)
**Storage**: In-memory registries only (application records, pending/completed pull requests) -- no new filesystem state; certificate material reuses `CertificateStore`'s existing signing capabilities
**Testing**: `cargo test -p async-opcua-server --features companion-gds --lib -- gds::pull_methods::` (requires `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` present locally per `schemas/companion/README.md`) + a real Call-service integration test mirroring `gds_push_integration.rs`
**Target Platform**: Cross-platform Rust library/server (Linux CI primary); entirely inert unless `companion-gds` feature is enabled
**Project Type**: Library (OPC UA server SDK) -- single Cargo workspace
**Performance Goals**: N/A (administrative/rarely-called methods, not a hot path)
**Constraints**: Zero effect on servers built without `companion-gds`; must not regress GDS Push (features 101/102, untouched); certificate/key handling must fail closed
**Scale/Scope**: 1 conformance unit (2230); rewritten `gds/pull_methods.rs`; new object-instantiation helper scoped to `CertificateDirectoryType`; `companion` module exposed; `ServerBuilder` gains an opt-in wiring hook

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Every NodeId this feature relies on
  was verified against the actual GDS companion NodeSet2.xml (not assumed):
  `CertificateDirectoryType`'s identifier (63) and its six Mandatory
  methods' identifiers, confirmed to carry over unchanged under namespace
  remapping (`NodeSetNamespaceMapper` only remaps the namespace index, never
  the identifier -- verified by reading its source, `async-opcua-types/src/namespaces.rs`).
  The object-instantiation logic is built for exactly what
  `CertificateDirectoryType` needs, not spot-checked afterward. PASS.
- **II. Do It Right Once**: Reuses the existing (dormant but already-built)
  `companion-gds` import plumbing rather than writing a second XML importer;
  reuses Run 1's certificate-signing patterns (`X509::create_signing_request`,
  `CertificateStore`) rather than re-inventing CSR/key-pair generation;
  reuses the existing `ObjectBuilder`/`MethodBuilder`/`VariableBuilder`
  pattern already proven in `fota/file_node.rs`. PASS.
- **III. Individual Task Discipline**: Two user stories (certificate
  issuance workflow; discovery/status methods), each independently
  testable, plus a clearly separated foundational phase (companion wiring +
  instantiation) that both depend on. PASS.
- **IV. Security Is Paramount**: Every method enforces its spec-mandated
  channel security level and role; the object-instantiation logic only
  creates additional nodes when `companion-gds` is explicitly enabled by the
  operator (opt-in, not silently expanding the default AddressSpace);
  certificate/key generation reuses already-audited crypto-crate code paths
  rather than new unaudited ones. PASS.
- **V. Leave It Better Than You Found It**: Removes the wrong
  `GetRejectedList`/`UpdateCertificate` Pull-model callbacks entirely rather
  than leaving them alongside the real ones; the newly-discovered
  client-side sibling defect (same fabricated NodeIds in
  `async-opcua-client/src/gds/gds_client.rs` et al.) is explicitly recorded
  as a follow-up (Run 2) rather than silently ignored. PASS.

No violations; Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/103-gds-pull-fix/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/lib.rs                          # `mod companion;` -> `pub mod companion;`
async-opcua-server/src/gds/directory_instance.rs        # NEW: instantiates a CertificateDirectoryType "Directory" object from the imported companion type
async-opcua-server/src/gds/pull_methods.rs              # rewritten: real Pull-model methods, application registry, pending-request registry
async-opcua-server/src/gds/mod.rs                       # wiring: register_gds_pull_methods_from_companion(...)
async-opcua-server/tests/gds_pull_companion_integration.rs  # NEW: real Call-service dispatch proof, feature-gated on companion-gds
tools/cu-coverage-report/src/lib.rs                      # AUDIT_TABLE evidence for CU 2230
specs/conformance-tester/CU-COVERAGE.md                  # regenerated
TODO.md                                                  # CU 2230 closed; client-side sibling defect (Run 2) recorded
```

**Structure Decision**: A new `gds/directory_instance.rs` module holds the
object-instantiation logic (kept separate from `pull_methods.rs` so the
"build a live object graph from an imported type" concern doesn't get
tangled with the method-handler logic, mirroring how Run 2 kept
`trust_list.rs` separate from `push_methods.rs`). `pull_methods.rs` is
rewritten in place (its existing shape -- registry + handler struct +
registration function -- is the right shape, just pointed at the wrong
methods and missing the instantiation step it now depends on).

## Complexity Tracking

*No violations -- section not needed.*
