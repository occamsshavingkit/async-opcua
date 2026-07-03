# Tasks: Optional Dependencies and Security Hardening

**Input**: Design documents from `/specs/055-optional-deps-security/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/feature-flags.md

**Constitution grounding**: All behavior-affecting tasks below cite the relevant OPC UA Part/§ to
match the spec-level grounding. Per constitution IV, crypto/network paths fail closed and MUST NOT
panic. Per constitution III, one task per line, independently verifiable.

## Phase 1: Setup (blocking all stories)

- [X] T001 Add `pubsub` (`default = true`) and `history-sqlite` (`default = true`) feature flags to
      `async-opcua/Cargo.toml`, wiring them as `dep:async-opcua-pubsub` and
      `dep:async-opcua-history-sqlite` respectively. The `nano`, `micro`, `embedded`, and `standard`
      aliases MUST NOT transitively enable either. `server` and `base-server` keep them ON. Verify
      with `cargo check -p async-opcua --no-default-features --features nano` (must not resolve
      pubsub or history-sqlite). [Cite: research.md R2]
- [X] T002 [P] Remove any unconditional `extern crate` or `use` of pubsub/history-sqlite types from
      `async-opcua/src/lib.rs` that would break under `default-features = false` without those flags.
      [Cite: contracts/feature-flags.md §1]
- [X] T003 [P] Create `SecurityCheckEntry`, `SecurityCheckCategory`, `SecurityCheckOutcome` structs
      and enums in `async-opcua-server/src/security_checks.rs` as defined in data-model.md. Derive
      `Clone`, `Debug`. [Cite: data-model.md SecurityCheckEntry]
- [X] T004 [P] Create `SecurityCheckRegistry` struct with bounded `VecDeque<SecurityCheckEntry>`, a
      `max_entries` field (default 1000), and `record()`/`snapshot()`/`count()` methods in
      `async-opcua-server/src/security_checks.rs`. Bounded: when `entries.len() >= max_entries`,
      `pop_front()` before pushing. [Cite: data-model.md SecurityCheckRegistry; research.md R3]

**Checkpoint**: `cargo check --workspace` green; `cargo tree -p async-opcua --no-default-features --features nano` shows zero pubsub/history-sqlite deps.

## Phase 2: User Story 1 — Optional pubsub and history-sqlite (P1)

**Goal**: Profile builds exclude pubsub and history-sqlite from the dependency tree.
**Independent test**: `cargo tree` for nano/micro/embedded/standard shows neither crate; full build unchanged.

- [X] T005 [US1] Update `async-opcua/Cargo.toml` profile alias comments to note that `nano`/`micro`/`embedded`/`standard` explicitly exclude `pubsub` and `history-sqlite`.
- [X] T006 [US1] Verify `cargo build --profile embedded -p async-opcua-foundation-profile-nano-server` compiles without pubsub/history-sqlite in `cargo tree -e normal` output. Verify same for micro, embedded, and standard samples.
- [X] T007 [US1] Verify the full-featured workspace compiles unchanged: `cargo check --workspace --all-features` green, confirming `server`/`base-server` still pull in pubsub and history-sqlite.

**Checkpoint**: US1 complete; profile builds exclude the two deps; full build unchanged.

## Phase 3: User Story 2 — RSA-DH identity token encryption (P2)

**Goal**: Server supports RSA-KEM decryption of UserName identity tokens per OPC 10000-6 §6.7.3.
**Independent test**: Integration test connects, activates with RSA-DH encrypted UserName token, gets `StatusCode::Good`.

### Tests (red first)

- [~] T008 [P] [US2] Write integration test `async-opcua/tests/integration/rsa_dh_token.rs`:
      server with Basic256Sha256 endpoint advertising RSA-DH UserTokenPolicy; client creates session,
      activates with RSA-KEM-encrypted UserName token. Assert activation returns `StatusCode::Good`.
      RED today (RSA-DH not yet implemented). [Cite: OPC 10000-6 §6.7.3; OPC 10000-4 §7.41; spec FR-005]
- [~] T009 [P] [US2] Write rejection test in same file: malformed RSA-KEM ciphertext → server returns
      `BadIdentityTokenRejected`. [Cite: OPC 10000-4 §7.40; spec FR-005]

### Implementation

- [X] T010 [US2] Implement `decrypt_rsa_dh_token` in `async-opcua-crypto/src/user_token.rs` (or new
      `rsa_kem.rs`): accept `(ciphertext: &[u8], private_key: &PrivateKey)` → `Result<Vec<u8>>`.
      Algorithm: RSA-OAEP decrypt the wrapped symmetric key, then AES-256-KeyWrap unwrap the token.
      Fail closed: any step returning an error → `Err(StatusCode::BadIdentityTokenRejected)`.
      [Cite: OPC 10000-6 §6.7.3]
- [X] T011 [US2] Wire RSA-DH decryption into the server's ActivateSession handler in
      `async-opcua-server/src/session/manager.rs` (or wherever RSA-OAEP and ECC decryption are
      currently dispatched). Add an arm for `SecurityPolicy::RsaDh` → call T010. Must NOT regress
      existing RSA-OAEP or ECC paths. [Cite: spec FR-007]
- [X] T012 [US2] Update `EndpointDescription` construction to advertise an RSA-DH UserTokenPolicy
      when the server's certificate uses an RSA key, per Part 4 §7.41 Table 192. On EC-only cert
      endpoints, omit the RSA-DH policy. [Cite: OPC 10000-4 §7.41; spec FR-006]
- [X] T013 [US2] Verify T008/T009 green. Verify existing encrypted token tests still green (RSA-OAEP,
      ECC).

**Checkpoint**: RSA-DH token encryption works; no regression in existing crypto paths.

## Phase 4: User Story 3 — Server security checks framework (P3)

**Goal**: Centralized, bounded, queryable security check registry per OPC 10000-4 §6.5.
**Independent test**: Unit test records certificate rejection + user auth; verifies both are retrievable.

### Tests (red first)

- [X] T014 [P] [US3] Write unit test `async-opcua-server/src/security_checks.rs` (in-module):
      record 2 entries (one CertificateValidation/Fail, one UserAuthentication/Pass), call
      `snapshot()`, assert length = 2, assert entries match. [Cite: spec FR-009]
- [X] T015 [P] [US3] Write bounding test in same file: record 1001 entries with `max_entries=1000`,
      assert `count() == 1000`, assert oldest entry has been evicted. [Cite: spec FR-010]

### Implementation

- [X] T016 [US3] Add `SecurityCheckRegistry` field to `ServerInfo` in
      `async-opcua-server/src/info.rs`, initialized with `max_entries` from `ServerConfig`
      (new field, default 1000). Expose `security_checks()` and `security_check_count()` on
      `ServerHandle` (`async-opcua-server/src/server_handle.rs`). [Cite: contracts/feature-flags.md §3]
- [X] T017 [US3] Add `max_security_check_entries: usize` field to `ServerConfig`
      (`async-opcua-server/src/config/mod.rs` and `server.rs` builder), defaulting to 1000.
      [Cite: spec FR-010]
- [X] T018 [US3] Wire certificate validation results into the registry in
      `async-opcua-server/src/session/controller.rs` (open_secure_channel handler) and
      `async-opcua-server/src/session/manager.rs` (activate_session handler). Record
      CertificateValidation entries for both pass and fail outcomes. [Cite: OPC 10000-4 §6.5.5, §6.5.6]
- [X] T019 [US3] Wire user authentication results into the registry in
      `async-opcua-server/src/session/manager.rs` (activate_session handler). Record
      UserAuthentication entries for UserName, X509, and IssuedToken identity results.
      [Cite: OPC 10000-4 §6.5.6]
- [X] T020 [US3] Wire channel negotiation results into the registry in
      `async-opcua-server/src/session/controller.rs` (`open_secure_channel()` method). Record
      ChannelNegotiation entries for security policy/mode negotiation outcomes.
      [Cite: OPC 10000-4 §6.5.5]
- [X] T021 [US3] Add RBAC decision recording (gated behind `#[cfg(feature = "rbac")]`): when the
      rbac feature is enabled, record RbacDecision entries in
      `async-opcua-server/src/rbac/role_management.rs` or the enforcement point. When rbac is off,
      the category is simply never emitted (no compile error). [Cite: spec FR-012; OPC 10000-18 §6.3]
- [X] T022 [US3] Verify T014/T015 green. Manual test: start server, connect with untrusted cert,
      call `handle.security_checks()`, assert at least one CertificateValidation/Fail entry exists.

**Checkpoint**: Security check registry captures cert, auth, channel, and RBAC events; bounded; queryable.

## Phase 5: Polish & Final Verification

- [X] T023 Pre-push gate: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features`,
      `cargo test --workspace --all-features`, `RUSTFLAGS="-D warnings" cargo check -p async-opcua --no-default-features --features nano`.
- [X] T024 Verify cross-doc consistency: data-model.md, contracts/feature-flags.md, and
      `async-opcua/Cargo.toml` comments all match the shipped feature flag/alias set. Walk spec.md
      Success Criteria SC-001–SC-005 and check each off with evidence.

## Dependencies & Execution Order

- Setup (T001–T004) blocks everything.
- US1 (T005–T007) is independent of US2 and US3 — can run in parallel.
- US2 (T008–T013) is independent of US1 and US3 — can run in parallel.
- US3 (T014–T022) is independent of US1 and US2 — can run in parallel.
- Polish (T023–T024) requires all three stories complete.

## Parallel Opportunities

- T001 and T002 touch different files (Cargo.toml vs lib.rs) — parallel.
- T003 and T004 are in the same file but define independent types — sequential on that file.
- T008 and T009 share the same test file — sequential.
- US1, US2, and US3 are fully independent — can be dispatched to three agents simultaneously.

## Implementation Strategy

MVP = Phase 1 + Phase 2 (US1): after T007 the optional deps work ships. Each later story is independently
shippable. US3 is the largest; its T014–T015 (red tests) can be committed first, then T016–T022 implement.
