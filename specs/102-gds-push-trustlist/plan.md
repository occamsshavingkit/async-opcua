# Implementation Plan: GDS Push Model TrustList Completion (Run 2)

**Branch**: `102-gds-push-trustlist` | **Date**: 2026-07-17 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/102-gds-push-trustlist/spec.md`

## Summary

Implement the TrustListType (OPC-10000-12 §7.8.2) file-based read/write
protocol -- `Open`, `OpenWithMasks`, `Read`, `Write`, `CloseAndUpdate` --
plus the immediate `AddCertificate`/`RemoveCertificate` (§7.8.2.6/§7.8.2.7)
methods, wired against the empirically-verified, real
`ServerConfiguration.CertificateGroups.DefaultApplicationGroup.TrustList`
AddressSpace nodes. Extends Run 1's `PushTransaction`/`GdsPushRegistry`
(`async-opcua-server/src/gds/push_methods.rs`) to optionally carry a
pending TrustList change alongside the existing pending certificate/key
change, so the existing `ApplyChanges`/`CancelChanges` methods commit or
discard either kind of pending change.

## Technical Context

**Language/Version**: Rust (workspace edition per `Cargo.toml`, stable toolchain)
**Primary Dependencies**: `async-opcua-crypto` (`CertificateStore` extended with trusted/issuer cert+CRL write helpers, `X509`), `async-opcua-types` (generated `TrustListDataType`/`TrustListMasks` binary codec), `async-opcua-server` (`gds` module, `RequestContext`, method-callback registration, session-scoped handle cache modeled on `history::continuation::HistoryContinuationPointCache`)
**Storage**: Filesystem PKI directories via the existing `CertificateStore` (trusted/issuer cert and CRL dirs already read; write-side helpers added this feature)
**Testing**: `cargo test -p async-opcua-server --lib -- gds::` (unit tests for the new TrustList method handlers and extended transaction) + `cargo test -p async-opcua-server --test gds_push_integration` (extended end-to-end wire-dispatch proof)
**Target Platform**: Cross-platform Rust library/server (Linux CI primary)
**Project Type**: Library (OPC UA server SDK) -- single Cargo workspace
**Performance Goals**: N/A (administrative/rarely-called methods, not a hot path)
**Constraints**: Must not affect any request type outside the GDS module; must not regress Run 1's certificate-rotation transaction; security-sensitive -- certificate/CRL handling must fail closed on any validation error; file-handle state must not leak across sessions or accumulate unbounded on abandoned opens
**Scale/Scope**: Continuation of 1 conformance unit (2231); extends `push_methods.rs`'s transaction and adds a new TrustList sub-module, additions to `CertificateStore` (trusted/issuer cert+CRL write helpers), extended integration test coverage

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Correctness Over Completion**: Every target NodeId was verified
  empirically (live Read against a running server) before this plan was
  written, using the exact methodology that caught Run 1's fabricated-
  NodeId bug. `CloseAndUpdate`'s certificate validation reuses the
  existing, already-audited `CertificateStore` validation path rather than
  a new, unaudited one. PASS.
- **II. Do It Right Once**: Extends Run 1's existing `PushTransaction`/
  `GdsPushRegistry` rather than building a second, parallel transaction
  mechanism; reuses the generated `TrustListDataType` binary codec rather
  than hand-rolling TrustList file serialization; models the new
  session-scoped file-handle cache on the existing
  `HistoryContinuationPointCache` age/capacity-bounded pattern rather than
  inventing an unbounded one. PASS.
- **III. Individual Task Discipline**: Two user stories (full TrustList
  read/write/apply cycle; single-certificate Add/Remove), each
  independently testable. PASS.
- **IV. Security Is Paramount**: TrustList content directly controls which
  peer certificates the server accepts. Every method enforces its
  spec-mandated authenticated-channel + SecurityAdmin requirement; file
  handles are session-bound and idle-timeout-bounded to prevent
  cross-session use or unbounded resource growth; `CloseAndUpdate`
  validates every certificate before accepting any change and discards
  the whole update on any failure (fail closed). PASS.
- **V. Leave It Better Than You Found It**: Completes CU 2231 rather than
  leaving it permanently partial; updates AUDIT_TABLE with verified
  evidence; explicitly records `DefaultHttpsGroup`/`DefaultUserTokenGroup`
  and the Pull-model CU 2230 sibling bug as documented follow-ups rather
  than silently expanding scope or silently ignoring them. PASS.

No violations; Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/102-gds-push-trustlist/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
async-opcua-server/src/gds/push_methods.rs        # extended: PushTransaction gains an optional pending TrustList change; ApplyChanges/CancelChanges commit/discard it
async-opcua-server/src/gds/trust_list.rs          # NEW: TrustList method handlers (Open/OpenWithMasks/Read/Write/CloseAndUpdate/AddCertificate/RemoveCertificate), session-scoped file-handle cache
async-opcua-crypto/src/certificate_store.rs        # + store_trusted_cert/remove_trusted_cert/store_issuer_cert/remove_issuer_cert (+ CRL equivalents as needed)
async-opcua-server/tests/gds_push_integration.rs   # extended: end-to-end wire-dispatch proof for TrustList Open/Read
tools/cu-coverage-report/src/lib.rs                # AUDIT_TABLE evidence for CU 2231, full closure
specs/conformance-tester/CU-COVERAGE.md            # regenerated
TODO.md                                            # CU 2231 fully closed; DefaultHttps/DefaultUserToken groups + CU 2230 remain as follow-ups
```

**Structure Decision**: New `async-opcua-server/src/gds/trust_list.rs`
module (the TrustList method surface is large enough -- file-handle
state, mask filtering, validation -- to warrant its own file rather than
growing `push_methods.rs` further), registered alongside the existing
push methods from `gds/mod.rs`. `PushTransaction` in `push_methods.rs` is
extended in place rather than duplicated, since Run 1's `ApplyChanges`/
`CancelChanges` must already be the single place both kinds of pending
change are resolved.

## Complexity Tracking

*No violations -- section not needed.*
