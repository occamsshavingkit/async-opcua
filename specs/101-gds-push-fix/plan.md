# Implementation Plan: GDS Push Model Fix + Completion (Run 1)

**Branch**: `101-gds-push-fix` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/101-gds-push-fix/spec.md`

## Summary

Fix a real bug (fabricated/wrong AddressSpace NodeIds) in the existing GDS
Push-model method wiring, remove a misplaced Pull-model method that had
been living in the Push file, and implement the real `ServerConfigurationType`
Push-model method surface (OPC-10000-12 §7.10) that has real, verified
target nodes in this project's generated standard nodeset:
`CreateSigningRequest`, `UpdateCertificate`, `ApplyChanges`,
`CancelChanges`, `GetRejectedList`, `ResetToServerDefaults`.
`CreateSelfSignedCertificate`/`DeleteCertificate`/`GetCertificates` are
deferred (their nodes don't exist in the imported nodeset). TrustList/
CertificateGroup methods are deferred to a follow-up run per user
direction.

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`, stable toolchain)
**Primary Dependencies**: `async-opcua-crypto` (`CertificateStore`, `X509`, `PrivateKey`, `x509_cert`/`der`/`spki`/`rsa` for real CSR generation), `async-opcua-server` (`gds` module, `RequestContext`, `ServerInfo`, method-callback registration)
**Storage**: Filesystem PKI directories via the existing `CertificateStore` (own cert/key, trusted/issuer/rejected cert dirs) — no new storage
**Testing**: `cargo test -p async-opcua-server --lib -- gds::` (unit tests for method handlers) + `cargo test -p async-opcua-server --test gds_push_integration` (new end-to-end wire-dispatch proof)
**Target Platform**: Cross-platform Rust library/server (Linux CI primary)
**Project Type**: Library (OPC UA server SDK) — single Cargo workspace
**Performance Goals**: N/A (administrative/rarely-called methods, not a hot path)
**Constraints**: Must not affect any request type outside the GDS module; must not regress the existing (correct) Pull-model `FinishSigningRequest`/rejected-cert tracking in `pull_methods.rs` (untouched); security-sensitive — certificate/key handling must fail closed on any validation error
**Scale/Scope**: 1 conformance unit (2231); rewritten `push_methods.rs`, small addition to `CertificateStore` (`read_rejected_certs`), new integration test suite

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Every target NodeId was verified
  empirically (live Read against a running server), not just looked up in
  generated source — this is how the original bug (fabricated NodeIds)
  was caught in the first place. The three nodes confirmed absent from
  the nodeset are documented as a real nodeset-source gap rather than
  silently skipped or faked. PASS.
- **II. Do It Right Once**: Reuses existing, already-built infrastructure
  (`opcua_crypto::gds_reload::save_new_credentials`/`reload_store_from_disk`,
  `CertificateStore`, `x509_cert::builder::RequestBuilder` pattern already
  used for self-signed cert creation) rather than inventing new
  mechanisms. PASS.
- **III. Individual Task Discipline**: Two user stories (certificate
  rotation workflow; rejected-list/reset), each independently testable.
  PASS.
- **IV. Security Is Paramount**: This is certificate/private-key handling
  code. Every method enforces its spec-mandated channel security level
  and SecurityAdmin role; transaction ownership is bound to the
  originating session; certificate validation fails closed. PASS.
- **V. Leave It Better Than You Found It**: Removes fabricated NodeId
  constants and a misplaced method rather than leaving them alongside new
  code; updates AUDIT_TABLE with verified evidence; records the sibling
  Pull-model bug as a documented follow-up rather than silently ignoring
  it. PASS.

No violations; Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/101-gds-push-fix/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/gds/push_methods.rs   # rewritten: fixed NodeIds, real method handlers, transaction state
async-opcua-crypto/src/certificate_store.rs   # + read_rejected_certs()
async-opcua-server/tests/gds_push_integration.rs # NEW: end-to-end wire-dispatch proof (real Call service)
async-opcua/tests/integration/mod.rs            # + mod gds_push;
tools/cu-coverage-report/src/lib.rs              # AUDIT_TABLE evidence for CU 2231, verified NodeIds
specs/conformance-tester/CU-COVERAGE.md          # regenerated
TODO.md                                            # CU 2231 backlog entry closed; CU 2230 sibling bug + TrustList follow-up recorded
```

**Structure Decision**: No new modules; `push_methods.rs` is rewritten in
place (its existing shape — registry + handler struct + registration
function — is the right shape, just pointed at the wrong nodes and
missing methods). `pull_methods.rs` is explicitly untouched.

## Complexity Tracking

*No violations — section not needed.*
