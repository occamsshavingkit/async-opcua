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
| standard (US4) | n/a (new) | TBD | | |
| minimal-server (contrast) | 7,631,864 B | unchanged surface (base-server, all gates) | | |
| simple-server (contrast) | 15,862,224 B | | | |

## Guard notes (for T036 CI wiring)

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
