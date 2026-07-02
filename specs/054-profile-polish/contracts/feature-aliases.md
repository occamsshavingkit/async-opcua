# Contract: profile feature surface (054)

Two public, semver-relevant surfaces are added. Names are the contract; compositions may
evolve with profile grounding, removals/renames are breaking.

## 1. Facade aliases (`async-opcua`)

```toml
[dependencies.async-opcua]
default-features = false
features = ["nano"]   # or "micro" / "embedded" / "standard"
```

Guarantees:
1. Each alias builds standalone on stable Rust with `default-features = false`.
2. Compositions track the OPC UA 2017 server profile ladder (Nano ⊂ Micro ⊂ Embedded ⊂
   Standard) as grounded in research-assets/PROFILES-2017.md.
3. `nano`/`micro` exclude the generated core namespace and any crypto backend feature;
   `embedded`/`standard` include `generated-address-space` + `aws-lc-rs`.
4. A service excluded from a composition is answered with `Bad_ServiceUnsupported`
   (filters: `Bad_MonitoredItemFilterUnsupported`) — never a panic or hang.
5. `base-server` and `server` keep their pre-054 meaning (full subsystem surface,
   without/with the core nodeset).

Non-guarantees: profile-named builds are NOT OPC Foundation certification claims;
documented byte sizes are dated measurements, not stable guarantees; capacity minimums
are delivered via sample configuration, not compile surface.

## 2. Server-crate subsystem features (`async-opcua-server`, all default ON)

`subscriptions`, `subscriptions-standard`, `events`, `alarms`, `method-call`, `history`,
`history-aggregates`, `query`, `node-management`, `diagnostics`, `rbac`, `gds`, `fota`,
`programs`, `lds` — see data-model.md for the gate table with requires-relations and
fail-closed behaviors.

Guarantees:
1. Default feature set preserves today's full surface (no change for
   `cargo add async-opcua-server`).
2. Features are additive and freely composable; any subset compiles.
3. Requires-relations are enforced by cargo (`alarms = ["events", "method-call"]` etc.),
   never by compile errors in gated code.
4. With all features enabled, behavior is bit-for-bit today's behavior (full workspace
   test suite unchanged).
