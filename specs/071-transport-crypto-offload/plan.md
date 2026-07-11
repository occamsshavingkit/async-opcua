# Implementation Plan: Transport Asymmetric Crypto Offload

**Branch**: `071-transport-crypto-offload` | **Date**: 2026-07-10 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/071-transport-crypto-offload/spec.md`

## Summary

Move the asymmetric cryptographic work performed during OpenSecureChannel (Open/Renew) and
CreateSession off the async request-processing threads onto Tokio's blocking pool via
`spawn_blocking`, so a burst of channel handshakes can no longer stall requests for
already-established sessions. This completes the highest-severity items deferred from the
feature-070 async-lock audit (findings C-001/C-002/C-004; tasks T086–T090).

**Approach — "thin offload seam" (Approach A, approved 2026-07-10):** The two `&self` crypto
methods on `SecureChannel` — `asymmetric_sign_and_encrypt` and `asymmetric_decrypt_and_verify`
(`async-opcua-core/src/comms/secure_channel.rs`) — borrow `self.private_key`/`self.remote_cert`
and use a thread-local scratch buffer, so neither is `Send`/`'static` and neither can be moved
into a `spawn_blocking` closure. We extract their bodies into **free functions that take owned
key material** (`PrivateKey` already `#[derive(Clone)]`) **and owned buffers**, using a local
scratch `Vec` instead of the `PADDING_AND_SIGNATURE_SCRATCH` thread-local on the OSC path. The
existing `&self` methods remain as thin wrappers (clone material → call core) so any sync caller
or test is unaffected. In the async transport/session layers, only the **OpenSecureChannel /
asymmetric** branch extracts owned material and runs the core under `spawn_blocking`; the
symmetric per-request path is untouched.

Byte-identical wire output is guaranteed because the same crypto primitives run on the same
inputs — only the execution context changes. Two invariants are enforced explicitly: (1) chunk
sequence ordering (the sequence number is stamped into the chunk plaintext at
`SendBuffer::encode_next_chunk` *before* crypto, and OSC is a single handshake message, so no
reordering is possible); (2) status-code fidelity (the inner crypto `Result` is returned
verbatim; the new `JoinError` from a panicking task maps to dropping the connection without
masking a more specific crypto fault).

## Technical Context

**Language/Version**: Rust (workspace edition 2021), stable `rustc` 1.96.0
**Primary Dependencies**: `tokio` (`spawn_blocking`, already a dependency); existing crypto
backends unchanged — `aws-lc-rs`/`rsa` for RSA, `p256`/`p384` for ECC. **No new dependencies.**
**Storage**: N/A
**Testing**: `cargo test` (workspace); new behavioral/structural tests in
`async-opcua-core`/`-server`/`-client`; the existing conformance smoke matrix
(`async-opcua/tests/integration/conformance.rs`) as the no-wire-change guard; an `#[ignore]`'d
50-client handshake-storm test; full `tools/ci-playbook.sh --ci` gate.
**Target Platform**: any; reference measurements on x86-64 Linux with `taskset -c <core>`.
**Project Type**: Rust library workspace (transport → secure-channel → session layers across
`async-opcua-core`, `async-opcua-server`, `async-opcua-client`).
**Performance Goals**: **fairness, not speed** — no throughput regression in the uncontended
path; under a ≥50-client handshake storm, an established session's request latency stays
bounded rather than scaling with concurrent handshakes (SC-001).
**Constraints**: byte-identical wire output (conformance smoke stays green, SC-002/SC-004); no
new locks/mutexes/blocking primitives (AGENTS.md) — `spawn_blocking` is Tokio's sanctioned
offload, not a lock; network-reachable paths fail closed and never panic (constitution IV);
crypto fault codes preserved verbatim (FR-004); only OSC/CreateSession asymmetric ops
offloaded — symmetric hot path unchanged (FR-007).
**Scale/Scope**: 4 offload sites across 3 crates; one crypto-core refactor in
`secure_channel.rs`; no new public API on the umbrella crate; small-to-medium change.

## Constitution Check

*GATE: evaluated against constitution v1.0.0 — PASS (re-check after Phase 1 design).*

- **I. Correctness Over Completion (NON-NEGOTIABLE)**: correctness verified three ways —
  behavioral (a `current_thread`-runtime handshake that can only pass if crypto runs off the
  single worker), correctness (all-policies handshake success, RSA + ECC, Sign + SignAndEncrypt),
  and equivalence (the conformance smoke matrix stays byte-identical). Both invariants (ordering,
  status-code fidelity) are enforced in code and covered by tests. No story ships with a known
  gap; if a site turns out infeasible to offload cleanly it is documented, not half-offloaded.
- **II. Do It Right Once**: the entanglement (borrowed `&self` + thread-local) is resolved by a
  proper refactor into owned-input free functions with thin wrappers — not `#[allow(dead_code)]`
  or duplication. The `JoinError` path is handled explicitly (drop connection), never `unwrap`ed.
  No `// TODO` left on a reachable path.
- **III. Individual Task Discipline**: `tasks.md` will keep one task per unit — the core
  refactor, then each of the four offload sites, then each test — with a Part/§ citation where a
  task touches wire behavior. Codex dispatches get one task each.
- **IV. Security Is Paramount**: this *is* a security hardening — it removes a pre-auth DoS lever
  (the inbound OSC decrypt no longer blocks the event loop). Fail-closed is preserved: the inner
  crypto `Result` is returned unchanged, and a panicking offloaded task drops the one connection
  with a fault rather than crashing the server or masking the fault. Cloned `PrivateKey` material
  lives transiently on the blocking thread and drops at closure end; the zeroize posture is
  unchanged from the existing `Clone` usage (a `PrivateKey`-drop zeroize gap, if any, is
  pre-existing and out of scope — recorded in research.md). No secret is logged. No new attack
  surface: decode of the request types is unchanged; only *handling* moves threads.
- **V. Leave It Better**: retires the T086–T090 deferral from feature 070; the extracted
  crypto cores are directly unit-testable for the first time; the conformance smoke gate doubles
  as a standing equivalence guard.

**No violations. Complexity Tracking table omitted (nothing to justify).**

## Project Structure

### Documentation (this feature)

```text
specs/071-transport-crypto-offload/
├── plan.md              # This file
├── research.md          # Phase 0 — decisions: Approach A rationale, site map, JoinError, tests
├── data-model.md        # Phase 1 — owned-input crypto-core contracts (the "entities" here)
├── quickstart.md        # Phase 1 — build/verify commands, storm-test + no-wire-change checks
├── contracts/
│   └── crypto-offload-contracts.md   # Phase 1 — core fn signatures + behavioral invariants
├── checklists/
│   └── requirements.md  # from /speckit-specify
└── tasks.md             # Phase 2 — /speckit-tasks (NOT created here)
```

### Source Code (repository root)

```text
async-opcua-core/src/comms/
├── secure_channel.rs        # extract owned-input crypto cores; keep &self wrappers
│                            #   (asymmetric_sign_and_encrypt / asymmetric_decrypt_and_verify)
└── buffer.rs                # SendBuffer::encode_next_chunk — async OSC-encode seam (site 2/3)

async-opcua-server/src/
├── transport/tcp.rs         # site 1: inbound OSC decrypt+verify (pre-auth) → spawn_blocking
│                            # site 2: outbound OSC sign+encrypt → spawn_blocking
└── session/
    ├── controller.rs        # site 4a: CreateSession server-signature RSA signing (preflight)
    └── manager.rs           # site 4b: ECC ephemeral keygen (issue_server_ephemeral_key ~357/1539)

async-opcua-client/src/transport/
└── stream.rs                # site 3: client OSC sign+encrypt + response decrypt → spawn_blocking

# Tests
async-opcua-core/src/comms/secure_channel.rs   # unit tests for the extracted crypto cores
async-opcua-server/tests/                        # current_thread handshake proof; all-policies;
                                                 #   #[ignore]'d 50-client storm test
async-opcua/tests/integration/conformance.rs     # unchanged — equivalence guard
```

**Structure Decision**: This is a cross-layer change in an existing Rust library workspace, not
a new project skeleton. Work is localized to the secure-channel crypto core
(`async-opcua-core`), the two transports (`async-opcua-server/src/transport/tcp.rs`,
`async-opcua-client/src/transport/stream.rs`), and the CreateSession path
(`async-opcua-server/src/session/{controller,manager}.rs`). No new crate, module tree, or public
umbrella API is introduced.
