# Completeness backlog (un-defer the YAGNI deferrals)

**Principle:** async-opcua is a complete OPC UA *reference* implementation. Spec-defined behavior gets
built — "the spec defines it" is sufficient reason. (User direction 2026-06-25; see memory
`completeness-over-yagni`.) ponytail still governs HOW (shortest correct diff, no needless
abstraction), not WHETHER. One feature per PR: codex implements (MCP-grounded), Claude writes
independent tests.

_Last refreshed 2026-07-04 (after feature 057 completeness closeout)._

## Done

- **057 — Completeness Closeout** — OCSP live fetch per Part 4 §6.1.3 (RFC 6960 codec + HTTP fetch via ureq, three-mode policy Off/Soft/Strict, TTL cache, CertificateStore integration); multi-cert mixed server per Part 4 §5.5.4.1 (per-endpoint certificate and private key, backward-compatible); LegacyCall removed from subscription actor (24 statically-typed variants); four "bad ideas" example servers (chat, chaos, filesystem bridge, reverse bridge). 582 tests green.
- **056 — Complexity Cuts** — five Big-O improvements on hot paths: is_subtype_of memoization, TranslateBrowsePaths index, per-channel CreateSession counters, subscription priority cache, chunk header single-parse.
- **FX (Parts 80/81/83)** — async-opcua-fx crate; full EstablishConnections/CloseConnections + Verify*
  + ControlGroup + NodeIdTranslation + SetSecurityKeys (PRs #160–168; memory `feature-fx-completion`).
- **A&C (Part 9)** — AddComment (#169), DialogConditionType + Respond/Respond2 (#170), condition history /
  HistoryRead-events (#171), GeneralModelChangeEvent (#173), ConditionRefresh + Acknowledge/Confirm
  (#138), Discrete/OffNormal + Shelving/Suppression + branching + EURange limits (#152–#155), Exclusive/
  NonExclusive limit alarms w/ deadband (#139), **automatic source monitoring** (033, PRs #206–#209):
  bind an alarm to its InputNode → client Write / set_source_value / opt-in sampler auto-re-evaluates +
  dispatches the event (no manual update_value). **A&C COMPLETE.**
- **Aggregates (Part 13)** — full HistoryRead aggregate set (#142–#146); MultipleValues + status-bit edges
  (#175); AggregateFilter on MonitoredItems / aggregate subscriptions (#187/#188).
- **PubSub (Part 14)** — secured UADP NetworkMessage (#56); writable config Methods: connection/group
  (#178/#180) + PublishedDataSet (#181).
- **Node management (Part 4 §5.7)** — full 9-node-class AddNodes + NodeManagement (#172, #52).
- **Audit (Part 3/4 §A)** — full Audit*EventType hierarchy: write/method/node-mgmt (#174/#176/#177),
  session/channel/cert/cancel (#182–#186).
- **RBAC / Authorization (Part 3/18)** — feature 031, 106/106 (PRs #187–#196): node-level RolePermissions/
  AccessRestrictions, RoleSet, identity→role resolution, opt-in enforcement, secure preset.
- **Historical Access write (Part 11 HistoryUpdate)** — feature 032, 77/77 (PRs #198–#205): UpdateData/
  DeleteRawModified/DeleteAtTime/UpdateStructureData/UpdateEvent/DeleteEvent + modified-history read, on
  the sqlite backend AND a new in-memory store. Memory `feature-032-historyupdate-write`.

## To build (spec features)

All YAGNI-deferred items are now complete. Aggregates, Security/PKI, multi-cert, OCSP live fetch, and
example servers are done (features 034, 035, 042, 013, 014, 016, 057). Remaining items below are
operational/infrastructure.

## Remaining (operational / deferred)

- **OCSP live fetching — operational infrastructure**: the core validator accepts supplied/stapled OCSP
  responses and performs live fetching. Remaining scope is live OCSP responder infrastructure if an
  online-revocation deployment needs it.
- **async-delivery actor phases 2 & 4** — migrate management services off LegacyCall, which is now
  removed. The actor uses statically-typed variants for all operations. Phases 2 & 4 were about
  migrating remaining LegacyCall users — now complete via feature 057.
- **Perf backlog Tier 2/3** — all addressed by features 056 and 057 (complexity cuts on hot paths).
- **SDK tooling / example servers** — persistent-store example exists; four "bad ideas" servers added
  (chat, chaos, filesystem bridge, reverse bridge) via feature 057.
