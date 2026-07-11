# Handoff — Feature 071 (Transport Asymmetric Crypto Offload)

**As of:** 2026-07-11 · **Branch:** `071-transport-crypto-offload` (clean, local-only — NOT pushed) · **Base:** `master` (has feature 072 merged)

## TL;DR

Feature 071 moves the asymmetric crypto of OpenSecureChannel + CreateSession onto `tokio::spawn_blocking` so a pre-auth handshake storm can't starve live sessions. **Behavior-preserving** (byte-identical wire, same fault codes). Spec → plan → tasks → analyze → remediate are all done and committed. Implementation is **in progress**: the foundational crypto cores and **US1 site 1 (the pre-auth DoS lever) are done, verified, and committed.** Sites 2–4 + all tests + US2/US3/polish remain.

Read these three first: `spec.md`, `plan.md` (Approach A "thin offload seam"), `tasks.md` (16 tasks). `contracts/crypto-offload-contracts.md` has the **corrected** core signatures (see "Discoveries" below). `research.md` R4 = the 4-site map.

## What's committed on the branch (newest first)
- `8aa9fdc97` — **US1 site 1 (T005)**: server inbound OSC decrypt offloaded. ✅ verified.
- `807024d2c` — **Foundational (T002–T004)**: the two owned-input crypto cores + G1 test. ✅ verified.
- `1f390ac10` — sign-core contract correction (ECC state).
- `cfd94694e`, `24d13655a`, `98b4b0665`, `d9ec71503` — tasks/plan/spec.

## Implementation state per task
- **T001 baseline** — implicitly done (existing suite is the equivalence net).
- **T002/T003 (cores)** — DONE. `asymmetric_sign_and_encrypt_owned` + `asymmetric_decrypt_and_verify_owned` are `pub(crate)` `Send+'static` free functions in `async-opcua-core/src/comms/secure_channel.rs`; the `&self` methods are thin wrappers.
- **T004 (independent test)** — DONE. `owned_crypto_cores_are_offloadable` in `async-opcua-core/src/tests/secure_channel.rs` proves G1. G2 (byte-identical) is proven by the pre-existing 91 crypto round-trip + ChannelThumbprint tests.
- **T005 (site 1, server inbound OSC decrypt — PRE-AUTH DoS lever)** — DONE + verified. Added `verify_and_remove_security_server_async` / `decrypt_open_secure_channel_async`, a `prepare_open_secure_channel_decrypt` (sync validation + owned-input extraction) and a shared `finish_open_secure_channel_decrypt`. `process_message`/`handle_incoming_message` in `async-opcua-server/src/transport/tcp.rs` are now `async`. Only the OSC asymmetric leaf offloads; None + symmetric stay inline.
- **T006–T016** — NOT STARTED (see "Remaining").

## Remaining work (in order)
- **T006** — site 2: server **outbound** OSC sign+encrypt → `spawn_blocking`. Seam: `SendBuffer::encode_next_chunk` (`async-opcua-core/src/comms/buffer.rs:164`) → `apply_security` (secure_channel.rs:874) OSC branch → `asymmetric_sign_and_encrypt_owned`. The send path in `tcp.rs` must `.await` the offloaded chunk before the next (sequence ordering, OPC-10000-6 §6.7.2.4 — but OSC is single-chunk so trivially safe). Client write-back of `first_request_signature` (sign core `.1`) happens after the offload.
- **T007** — site 4a: CreateSession server-signature RSA sign → `spawn_blocking` in `async-opcua-server/src/session/controller.rs` (`CreateSessionDraft::prepare_endpoint_preflight` ~:565, already outside the session-manager lock). OPC-10000-4 §5.7.2.
- **T008** — site 4b: ECC ephemeral keygen → `spawn_blocking` in `async-opcua-server/src/session/manager.rs` (`issue_server_ephemeral_key` ~:357 + renew ~:1539). OPC-10000-6 §6.8.2.
- **T009** — US1 structural proof (Claude/independent): OSC handshake on `#[tokio::test(flavor="current_thread")]` with the sole worker kept busy — completes only if crypto is off-thread (contract C6). Load-bearing.
- **T010** — `#[ignore]`'d 50-client storm test: established-session p99 under storm ≤ 2× its no-storm p99.
- **T011** — US2: all-policies handshake+session (RSA Basic256Sha256 + Aes256Sha256RsaPss, ECC NistP256/P384, Sign + SignAndEncrypt).
- **T012** — US2: conformance smoke (`async-opcua/tests/integration/conformance.rs`) green unchanged + byte-identical.
- **T013** — US3: client OSC offload in `async-opcua-client/src/transport/stream.rs:286`.
- **T014** — US3: crypto-failure-containment test (offloaded unit failure drops one connection, specific fault preserved).
- **T015/T016** — polish: verify symmetric path untouched (`hot_path_*` tests) + full `tools/ci-playbook.sh --ci` + retire the 070 T086–T090 deferral.

## The workflow (KEEP DOING THIS — user chose it, memory `codex-no-self-authored-tests`)
1. **Codex implements each site, one task per dispatch** (`mcp__codex__codex`, `sandbox: workspace-write`, `approval-policy: never`). Give it: the exact seam (file:line), the corrected core contract, byte-identical + fail-closed constraints, the JoinError arm spec, and a **NO-GIT GUARDRAIL** (memory `codex-worktree-branch-hazard`: "run no git command, don't switch branches"). Never batch tasks.
2. **After each dispatch, independently verify:** `git branch --show-current` (guardrail), `git status` (scope), read the diff of the delicate parts, and **run the handshake/integration tests UN-SANDBOXED yourself** — codex's sandbox denies TCP `bind`, so it CANNOT run `security_tests`/`create_session`/`connection` tests; that verification is yours.
3. **Claude writes the independent tests** (T004, T009, T014) — anchored to external behavior, not codex's code.
4. Commit per verified site (bisectable for a security change).

## Discoveries / gotchas (IMPORTANT)
- **The plan's contract C1 was incomplete** — both cores also thread ECC state the plan omitted: `is_client_role`, `apply_channel_thumbprint`, `first_request_signature` (Mutex, read+written for sign, read-only for decrypt — but decrypt DOES write a server-side store, handled in `finish`). `contracts/crypto-offload-contracts.md` is corrected. **Any new site touching ECC must thread these.**
- **The ECC `first_request_signature` server-store looks wrong but isn't** — the decrypt path stores it computed from `src` (pre-crypto) in `prepare`, not `dst` (post-crypto); this is equivalent because it only applies for ECC (`is_ecc && !is_client_role`) and ECC doesn't asymmetric-encrypt the OSC body (`src == dst` in range), and it's written in the shared `finish` only after the crypto `?` succeeds. Don't "fix" it.
- **CI only runs on PRs to `master`/`rewrite-master`** (`.github/workflows/main.yml`). A PR to a feature branch gets only Codacy/CodeRabbit. When ready, PR `071-...` → `master` for the full Rust CI.
- **Local vs CI flag gaps bite** (learned twice on 072): always run `cargo clippy --workspace --all-targets --all-features` and the `-Dwarnings` no-default legs locally before a PR — `cargo build`/`--lib` and no-`-Dwarnings` builds miss conditionally-unused imports and stale integration-test binaries.
- **`cargo clean` was run** this session — `target/` was wiped (was 179 GB). First build is a full rebuild (~minutes; LTO release builds exceed the 2-min Bash foreground timeout → use `timeout: 540000`).

## Context — feature 072 (this session, DONE)
Merged to `master` (PR #281, merge commit `ec848a6a4`): US1 per-request cuts (S1a/S2, S1b as hygiene) + US2 opt-in `sharded` thread-per-core run mode (`Server::run_sharded(cores)`, default-off, +11–18% real server). See memory `feature-072-hot-path-throughput`. The bench harness lives in `~/scratch/opcua-localhost-bench/` (bench server has a `sharded` feature wired in; task affinities were restored).

## Fastest way to resume
`git checkout 071-transport-crypto-offload` (should already be there), confirm `git branch --show-current` + `git status` clean, then dispatch **T006** to codex following the workflow above. The next natural checkpoint is end of US1 (after T005–T010).
