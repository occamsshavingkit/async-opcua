# Session handoff — conformance register tail CLOSED (sprint 053, 2026-07-02)

**State:** `master` clean at merge commit `7f6c3320c` (PR #255), all CI green, all feature branches
pruned (local + remote). Work is on the fork `occamsshavingkit/async-opcua` (merged via `gh`);
never push upstream FreeOpcUa (two private security disclosures still pending with Einar — no
upstream PRs until he replies; see memory `upstream-security-disclosure-pending`).

**Driving principle:** async-opcua is a *complete reference implementation* — build the spec
surface; do not defer spec-defined behavior on YAGNI/ponytail grounds (memory
`completeness-over-yagni`). Backlog: [`specs/completeness-backlog.md`](completeness-backlog.md).

## Headline

**The conformance-audit register (`specs/conformance-audit/FINDINGS.md`) has ZERO open, partial,
or deferred rows.** Sprint 053 closed the entire tail left by the 2026-07-01 reconciliation, as
one speckit feature with 7 independently-committed user stories (PR #255, 14 commits). Everything
before this window (features 047–052, PRs #245–#254) is merged and recorded in memory.

## Delivered this session (feature 053, PR #255 — MERGED)

One story per finding, red-first independent tests, every task citing its OPC UA Part/§
(artifacts: `specs/053-conformance-small-items/`):

- **US1 / P5-04 (Part 5 §6.3.3 Table 11)** — all mandatory `ServerDiagnosticsType` members served
  live, built on read: `EnabledFlag` (runtime `AtomicBool`, writable), `SubscriptionDiagnosticsArray`,
  `SessionsDiagnosticsSummary` + both session arrays. TWO new fail-closed `ServerUserToken`
  privileges: `write_diagnostics` (EnabledFlag write) and `read_security_diagnostics`
  (security array, §6.3.5/§7.15; requires read_diagnostics too). New
  `SessionManager::snapshot_sessions()`; `ServerContext` gained `session_manager`. The core read
  path is now two-phase (prepare under the address-space guard, await after) — reuse this pattern
  for any future async-built core value.
- **US2 / P4-ATTR-04** — `Bad_OutOfRange` on writes violating `EURange` or modeled
  `DataTypeDefinition::Enum` sets, in the shared `validate_node_write` Value arm (all node
  managers incl. the test NM); scalar/whole-array/index-ranged, rejected before storage.
- **US3 / P4-ATTR-03 (§5.11.4.1)** — locale rules completed on the feature-049 side table:
  null-text deletes that locale, null-locale sets invariant text KEEPING entries, null+null
  deletes all; Value attribute stays single-locale (documented server-specific choice, locked in).
- **US4 / P4-ATTR-02** — verify-before-fix: maxAge already conformant everywhere a refreshable
  source exists (read callbacks ARE the source, invoked per Read with the exact maxAge). Docs at
  the three sinks + lock-in tests incl. NaN/∞ no-panic. No behavior change.
- **US5 / P8-02 (Part 8 §5.2/§5.3.2.2; Part 4 §7.38.1)** — event-driven EURange refresh:
  property→item registry in `SubscriptionCache` (registered from `get_eu_range` at create/modify),
  `notify_data_change` routes `NotificationWorkItem::RangeChanged` — covers client writes AND
  server-side sets at O(changes). One-shot `SemanticsChanged` bit with the §7.38.1 overflow
  re-arm. **Independent test caught a real codex bug** (pre-change skipped samples consumed the
  one-shot); the ponytail deferral comment is retired.
- **US6 / P3-09 (Part 3 §5.6.2/§8.60)** — `AccessLevelEx` modeled as derived
  `(extended_bits << 8) | access_level` (low byte structurally cannot diverge); builder setter,
  WriteMask bit 25, zero migration for existing Variables.
- **US7 / P5-03** — closed **not-a-bug** (finding was inverted); browse lock-in test.

### CI-fix tail (same PR)
- **cfg-gating**: US1 helpers gated behind `generated-address-space` — their only consumer (core
  node manager) is absent in no-default/foundation-profile builds.
- **Security bumps** (two RUSTSEC advisories published mid-flight): quick-xml 0.37.5→0.41.0
  (RUSTSEC-2026-0194 quadratic attr check, -0195 NsReader memory DoS). 0.41 model: text events are
  never unescaped; entities arrive as `Event::GeneralRef` — the XML stream reader resolves numeric
  char refs + the five predefined entities itself and FAILS CLOSED (`UnknownEntityReference`)
  otherwise. anyhow →1.0.103 (RUSTSEC-2026-0190 unsoundness).
- **Codacy file-length**: write-validation cluster extracted to
  `async-opcua-server/src/address_space/write_validation.rs` (pure move, re-exported;
  utils.rs 1558→1163 lines).

### Process notes from this session
- Three `/speckit-analyze` lens passes before implement (general → atomicity → spec-citation) each
  caught real issues; the citation lens surfaced the Part 4 §7.38.1 overflow re-arm rule BEFORE
  implementation (memory `speckit-analyze-lens-passes`). Convention: report, then apply remediation
  + commit as its own `spec(NNN):` commit.
- Two-wave red-first authoring when tests need not-yet-existing config/builder APIs: wave 1
  compiles against existing APIs (behavior-red), wave 2 adds the privileged-positive cases after
  the API task lands.

## Conventions / gotchas (entry points for continuation)
- **codex sandbox cannot bind sockets** — codex verifies with `cargo check/build`; run ALL
  server-crate test binaries (not just `--lib`) and the `async-opcua` integration suite yourself.
  codex tasks must cite the OPC UA Part/§ for its reference MCP; one task per dispatch; codex
  never authors the tests that verify its own work (caught real bugs again this session).
- **Pre-push gate** (all bit us or CI at some point): `cargo fmt --check`; clippy
  `--workspace --all-targets --all-features`; `RUSTFLAGS="-D warnings" cargo check
  --no-default-features -p async-opcua -p async-opcua-types -p async-opcua-nodes
  -p async-opcua-server`; the three `foundation-profile-{nano,micro,embedded}-server` checks;
  `cargo deny check advisories`; full workspace tests. The facade-crate no-default clippy leg is
  NOT sufficient — the server crate has its own no-default surface.
- **codegen gate:** `verify-clean-codegen` regenerates 3 configs; if you touch
  `async-opcua-codegen`, regenerate + `cargo fmt --all` locally and commit the generated diff.
- **RSA Marvin (RUSTSEC-2023-0071):** left as-is — default build uses constant-time aws-lc-rs;
  only the pure-Rust `--no-default-features` path uses the vulnerable decrypt (documented accepted
  trade-off). No action.
- FINDINGS.md row updates ride in the same commit as the story that closes the row.

## What's next (decision-free candidates)

1. **Nano/Micro/Embedded profile polish** — user-requested (memory `todo-embedded-profiles`);
   the foundation-profile sample crates exist (feature 041) and now build warning-clean; remaining:
   size-matrix documentation, size-tuned release profile, any `nano`/`micro` feature-alias gaps.
2. **Conformance-tester phase 2** (`specs/conformance-tester/PLAN.md`) — address-space oracle:
   vendor Core NodeSet2.xml + CSVs, diff the live server against it. Biggest long-term lever.
3. **OCSP live fetching** — the validator already accepts supplied/stapled responses; remaining is
   online responder infrastructure.
4. **SDK/examples** (TODO.md): persistent-store example server, "bad ideas" servers, node-manager
   ergonomics tooling.

Needs a decision first: **multi-cert mixed server** (RSA+ECC per endpoint) — transport-layer
cert-selection refactor, LARGE, deferred since feature 012.
