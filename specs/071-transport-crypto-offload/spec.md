# Feature Specification: Transport Asymmetric Crypto Offload

**Feature Branch**: `071-transport-crypto-offload`
**Created**: 2026-07-10
**Status**: Draft
**Input**: User description: "Offload OpenSecureChannel and CreateSession asymmetric crypto onto the tokio blocking pool via spawn_blocking, completing the deferred P0 findings (C-001/002/004, tasks T086-T090) from feature 070's async-lock audit."

## Overview

Asymmetric cryptographic work (RSA/ECC signing, decryption, verification, and ECC
ephemeral-key generation) is performed during secure-channel establishment
(OpenSecureChannel Open/Renew) and session creation (CreateSession). Today this work
runs on the shared threads that also process application requests. A burst of channel
handshakes therefore competes with — and can stall — request processing for sessions
that are *already established*. This feature moves that asymmetric work off the shared
request-processing threads so handshake load can no longer starve live sessions.

This is a **denial-of-service / fairness hardening** change. It is explicitly **not** a
throughput change and **must not** alter any observable protocol behavior: the bytes on
the wire, the status codes returned, and the message ordering all stay identical. It
completes the highest-severity items deferred from the feature-070 async-lock audit
(findings C-001, C-002, C-004; tasks T086–T090).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Established sessions survive a handshake storm (Priority: P1)

An operator runs a server serving live client sessions. An unauthenticated party (or a
reconnect storm after a network blip) opens many secure channels at once, each forcing
the server to perform an RSA/ECC private-key operation. The already-connected clients
must keep getting timely responses; the handshake burst must not freeze their traffic.

**Why this priority**: This is the security lever — the inbound OpenSecureChannel decrypt
is reachable *before authentication*, so an attacker can drive expensive private-key
operations without credentials. Protecting live sessions from that is the core value of
the feature; everything else is correctness scaffolding around it.

**Independent Test**: With a server under a burst of N concurrent channel-opening clients,
a separate already-established session issuing periodic reads sees its request latency stay
within a bounded threshold, rather than degrading in proportion to the handshake load.

**Acceptance Scenarios**:

1. **Given** a server with one established, active session, **When** many clients open
   secure channels simultaneously, **Then** the established session's request round-trip
   latency stays bounded (does not scale with the number of concurrent handshakes).
2. **Given** a server whose request-processing capacity is fully occupied, **When** a new
   client attempts a secure-channel handshake, **Then** the handshake still completes
   successfully (the crypto no longer needs that capacity to run).

---

### User Story 2 - No observable protocol change (Priority: P1)

An integrator running a mixed fleet (this server plus third-party OPC UA clients/servers,
and a conformance harness) must see zero behavioral difference after the change: the same
handshake succeeds, the same faults are reported for the same bad inputs, and every
security policy still interoperates.

**Why this priority**: A DoS fix that changes the wire or the fault contract would be a
regression, not an improvement. Preserving exact observable behavior is a release gate,
not a nice-to-have — a conformant peer must not be able to tell the change happened.

**Independent Test**: The full security-policy × security-mode × identity-token conformance
matrix passes unchanged, and the encoded output of handshake/session messages is
byte-for-byte identical to the pre-change output.

**Acceptance Scenarios**:

1. **Given** any supported security policy (RSA Basic256Sha256, Aes128Sha256RsaOaep,
   Aes256Sha256RsaPss; ECC NistP256/NistP384) in Sign or SignAndEncrypt mode, **When** a
   client establishes a channel and session, **Then** it succeeds exactly as before.
2. **Given** a malformed or untrusted handshake input, **When** the server rejects it,
   **Then** the exact same fault status code is returned as before the change.
3. **Given** the conformance smoke matrix, **When** it is run against the changed server,
   **Then** every combination passes with no differences from the baseline.

---

### User Story 3 - Crypto failures stay contained (Priority: P2)

A malformed or hostile handshake reaches a cryptographic primitive and triggers an
unexpected failure while the crypto is running off the main threads. The single offending
connection must be dropped with a correct fault; the server and all other sessions must be
unaffected.

**Why this priority**: Moving work onto a separate execution context introduces a new
failure mode (the offloaded unit dying unexpectedly). That mode must be contained and must
not degrade the fault contract or the server's availability. It is P2 because it guards an
edge that should be unreachable in practice, but must still be handled explicitly.

**Independent Test**: An offloaded crypto unit that fails unexpectedly results in only its
own connection closing; a normal client connecting immediately afterward still succeeds,
and the reported fault is the specific cryptographic fault, not a generic internal error.

**Acceptance Scenarios**:

1. **Given** a handshake whose offloaded crypto unit terminates unexpectedly, **When** the
   server handles it, **Then** only that connection is dropped and the server keeps serving
   other sessions.
2. **Given** a cryptographic validation failure, **When** it surfaces through the offloaded
   path, **Then** the specific fault (e.g. security-check failed, certificate invalid,
   policy rejected) is reported — the same as when the work ran inline.

---

### Edge Cases

- **Connection dropped mid-handshake**: the client disconnects while its handshake crypto
  is still running off-thread — the in-flight work completes and its result is discarded;
  no hang, no leaked state, no key material retained.
- **Offloaded unit panics**: a crypto primitive panics on crafted input — the connection is
  dropped with the appropriate fault; the server survives (never a process-wide crash).
- **Extreme storm exceeding offload capacity**: when concurrent handshakes exceed the
  available off-thread capacity, handshakes queue *there* rather than on the request path,
  so established-session latency stays protected (queued handshakes simply take longer).
- **Client sharing its runtime**: a client application whose async runtime also does other
  work is not blocked by its own channel-handshake crypto.
- **Sequence ordering under offload**: an offloaded handshake message must still be emitted
  in correct sequence order relative to the channel — never reordered.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: During secure-channel Open/Renew, the server MUST perform the asymmetric
  decrypt/verify (inbound) and sign/encrypt (outbound) operations without blocking the
  processing of requests belonging to already-established sessions.
- **FR-002**: During CreateSession, the server MUST perform server-signature generation and
  ECC ephemeral-key generation without blocking request processing for other sessions.
- **FR-003**: The change MUST NOT alter any bytes on the wire. The encoded output of every
  secure-channel and session message (signatures, ciphertext, encoding, and sequence
  numbers) MUST be identical to the pre-change output for identical inputs.
- **FR-004**: Every existing cryptographic fault status code (including
  `BadSecurityChecksFailed`, `BadCertificateInvalid`, `BadSecurityPolicyRejected`) MUST
  continue to be returned unchanged for the corresponding failure.
- **FR-005**: An offloaded crypto operation that terminates unexpectedly MUST result in the
  offending connection being dropped, MUST NOT crash the server or affect other sessions,
  and MUST NOT mask a more specific cryptographic fault with a generic error.
- **FR-006**: The secure-channel chunk sequence-number guarantee (monotonic, in-order
  emission per OPC-10000-6 §6.7.2) MUST be preserved.
- **FR-007**: Only the asymmetric operations tied to OpenSecureChannel and CreateSession are
  affected. The per-request symmetric message path MUST remain unchanged and MUST NOT incur
  new off-thread dispatch.
- **FR-008**: The OpenSecureChannel crypto offload MUST apply on both the server side and the
  client side.
- **FR-009**: All supported security policies (RSA Basic256Sha256, Aes128Sha256RsaOaep,
  Aes256Sha256RsaPss; ECC NistP256, NistP384) in both Sign and SignAndEncrypt modes MUST
  continue to establish channels and sessions correctly.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Under a burst of ≥50 concurrent channel-opening clients, an already-established
  session's request latency stays within a bounded threshold and does not grow in proportion
  to the number of concurrent handshakes.
- **SC-002**: The full security-policy × security-mode × identity-token conformance smoke
  matrix passes with no differences from the pre-change baseline.
- **SC-003**: A secure-channel handshake completes successfully even when all shared
  request-processing capacity is occupied, demonstrating the crypto no longer consumes it.
- **SC-004**: For identical inputs, the encoded wire output of handshake and session messages
  is byte-for-byte identical to the pre-change output across every supported policy.
- **SC-005**: The full existing test suite passes; handshake success across the entire policy
  matrix (RSA + ECC, Sign + SignAndEncrypt) is unchanged.

## Assumptions

- Off-thread execution capacity is already configurable (the `max_blocking_threads` server
  setting delivered as feature-070 task T091); tuning it is **out of scope** here.
- Secure-channel handshakes and CreateSession are infrequent relative to per-request traffic,
  so any small per-handshake allocation introduced by owning the crypto inputs off-thread is
  acceptable and does not affect steady-state performance.
- The per-request symmetric message path is already fast (symmetric encrypt + HMAC) and is
  not a blocking concern, so it is deliberately excluded.
- This feature does not add or change any security policy, cipher, key size, or wire format;
  it only changes *where* existing operations execute.

## Out of Scope

- Any change to the symmetric per-request message path.
- Tuning or redesign of the off-thread execution pool sizing (T091, already delivered).
- Any new security policy, cipher suite, or wire-format change.
- Throughput optimization of the handshake path itself (the goal is fairness under load, not
  faster individual handshakes).
