# Completeness backlog

**Principle:** async-opcua is a complete OPC UA *reference* implementation. Spec-defined behavior gets
built — "the spec defines it" is sufficient reason.

_Last refreshed 2026-07-04 (after feature 057 completeness closeout)._

## Done

- **057 — Completeness Closeout** — OCSP live fetch per Part 4 §6.1.3; multi-cert mixed server per Part 4 §5.5.4.1; LegacyCall removed; four example servers.
- **056 — Complexity Cuts** — five Big-O improvements on hot paths.
- **FX (Parts 80/81/83)** — async-opcua-fx crate; full EstablishConnections/CloseConnections + Verify* + ControlGroup + NodeIdTranslation + SetSecurityKeys.
- **A&C (Part 9)** — AddComment, DialogConditionType, condition history, ConditionRefresh + Acknowledge/Confirm, Discrete/OffNormal, Shelving/Suppression, branching, EURange limits, limit alarms with deadband, automatic source monitoring. **COMPLETE.**
- **Aggregates (Part 13)** — full HistoryRead aggregate set; MultipleValues; AggregateFilter on MonitoredItems.
- **PubSub (Part 14)** — secured UADP NetworkMessage; writable config Methods.
- **Node management (Part 4 §5.7)** — full 9-node-class AddNodes + NodeManagement.
- **Audit (Part 3/4 §A)** — full Audit*EventType hierarchy.
- **RBAC / Authorization (Part 3/18)** — 106/106: RolePermissions/AccessRestrictions, RoleSet, identity→role resolution, opt-in enforcement, secure preset.
- **Historical Access write (Part 11 HistoryUpdate)** — 77/77: UpdateData/DeleteRawModified/DeleteAtTime/UpdateStructureData/UpdateEvent/DeleteEvent + modified-history read, on sqlite AND in-memory backends.
- **FindServersOnNetwork (Part 12)** — LDS-ME multicast advertise + discover behind `discovery-mdns` feature.
- **ECC security policies (Parts 4/6)** — EccNistP256/P384 secure channels, EphemeralKey exchange, EccEncryptedSecret identity tokens, ChannelThumbprint binding.
- **Certificate validation (Part 4 §6.1.3)** — CA chain building, CRL revocation, supplied/stapled OCSP, live OCSP fetch (feature 057).
- **Foundation profiles** — nano, micro, embedded, standard profile builds with subsystem feature gates.
- **Security checks** — SecurityCheckRegistry for cert validation, user authentication, channel negotiation.

## Remaining

- **CTT certification run**: run the demo server against the OPC Foundation Compliance Test Tool for behavioral gap discovery.
