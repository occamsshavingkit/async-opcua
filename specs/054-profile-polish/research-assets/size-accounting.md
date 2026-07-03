# Size accounting — feature 054 profile builds

Method: `cargo build --locked --profile embedded -p <pkg>` (ONE package per
invocation — see research.md R10), `stat -c %s`, x86-64 Linux, rustc 1.96.0.
Baselines (pre-054) from research.md R10 were measured on binaries that could not
actually run (no node manager — they exited at startup); post-054 numbers are
functional servers.

## Per-story measurements

| Build | Pre-054 baseline | Post-054 | Delta | Date |
|-------|------------------|----------|-------|------|
| nano (US1, T021) | 7,636,648 B (non-functional) | **6,765,888 B** (functional Nano 2017 surface) | −870,760 B (−11.4%) | 2026-07-02 |
| micro (US2, T026) | 7,636,664 B (non-functional) | **7,213,200 B** (functional Micro 2017 surface) | −423,464 B (−5.5%) | 2026-07-02 |
| embedded (US3, T030) | 9,906,256 B (non-functional) | **9,906,256 B** (functional Embedded 2017 surface) | 0 B (gated-out code was already LTO-dead-stripped) | 2026-07-02 |
| standard (US4, T031) | n/a (new) | **16,751,080 B** (functional Standard 2017 surface) | n/a | 2026-07-02 |
| minimal-server (contrast) | 7,631,864 B | unchanged surface (base-server, all gates) | | |
| simple-server (contrast) | 15,862,224 B | | | |

## Size ordering notes

- nano < micro < embedded: monotonic as expected (each rung adds compiled surface).
- standard (16,751,080 B) > simple-server (15,862,224 B): the `standard` alias
  includes `discovery-server-registration` which compiles client-side periodic
  registration code into the binary. simple-server uses the full `server` feature
  (all gates) but does NOT use `discovery-server-registration`. This is expected
  and documented — the profiles are a ladder of *profile surface*, not strictly
  of binary size.
- embedded (9,906,256 B) == pre-054 baseline (9,906,256 B): gated-out code
  (events, alarms, history, etc.) was already LTO-dead-stripped in the base-server
  build. The gates buy compile surface and capability honesty, not bytes.

## Guard notes (for T036 CI wiring)

## Section accounting (T038)

`size` output (text, data, bss — stripped binaries):

| Build | text | data | bss | total |
|-------|------|------|-----|-------|
| nano | 6,454,185 | 308,320 | 11,928 | 6,774,433 |
| micro | 6,894,401 | 315,360 | 11,272 | 7,221,033 |
| embedded | 9,539,033 | 363,460 | 20,396 | 9,922,889 |
| standard | 16,132,052 | 612,988 | 24,036 | 16,769,076 |

Top text-section hotspots (unstripped, `nm -C --size-sort`):

| Symbol | Size (B) | Present in |
|--------|----------|------------|
| `RequestMessage::decode_by_object_id` | ~66K | all |
| `CertificateStore::read_crl_dir` | ~41K | all |
| `SessionController::process_request` | ~30–35K | all |
| `main::{closure}` | ~33–53K | all |
| `regex_automata::meta::strategy::new` | ~22K | all |
| `moka::sync::base_cache::Inner::do_run_pending_tasks` | ~21K | all |
| `validate_certificate_chain` | ~20K | all |
| `X509::from_pkey` | ~19K | all |
| `MessageHandler::handle_message` | ~38K | micro+ (not nano) |
| `create_monitored_items::{closure}` | ~21K | micro+ (not nano) |
| `<ObjectId as Debug>::fmt` | ~24K | all |
| `<T as der::decode::Decode>::decode` | ~25K | all |

Key observations:
- nano → micro delta (~440K text): dominated by subscription machinery
  (`MessageHandler::handle_message`, `create_monitored_items`, sampler).
- micro → embedded delta (~2.6M text): generated core namespace
  (type system, standard nodes, method dispatch).
- embedded → standard delta (~6.6M text): `discovery-server-registration`
  pulls in the full client crate (periodic registration, secure channel,
  session management).
- Cross-cutting hotspots present in ALL profiles: crypto cert chain
  validation, regex (der parsing), moka cache, backtrace machinery.

- rbac sentinel: the rbac-off build mounts a zero-size stub at the SAME
  `crate::rbac` path (R6 one-code-shape design), so `opcua_server::rbac::` matches
  stub drop-glue. Use real-module-only sentinels:
  `opcua_server::rbac::role_management` / `opcua_server::rbac::defaults`.
- Verified-clean nano invocation (2026-07-02):
  `tools/check-profile-absence.sh async-opcua-foundation-profile-nano-server
  "subscriptions,subscriptions-standard,events,alarms,method-call,history,history-aggregates,query,node-management,diagnostics,rbac,gds,fota,programs,lds"
  "opcua_server::subscriptions::,opcua_server::alarms::,opcua_server::history::,opcua_server::gds::,opcua_server::rbac::role_management,opcua_server::rbac::defaults,opcua_server::programs::,opcua_server::fota::"`
- Symbol counts observed pre-gating (red state): 235 × subscriptions, 1 × history;
  alarms/gds/programs/fota were already LTO-dead-stripped in the nano binary —
  their gates buy compile surface and honesty, not nano bytes. The subscriptions
  gate is where the nano bytes came from.
