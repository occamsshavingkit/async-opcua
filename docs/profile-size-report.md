# Profile Size Report: Further Savings Opportunities

**Feature 054, Task T039** — ranked, non-overlapping suggestions for reducing
profile benchmark binary sizes beyond the current gate-based cuts. Each entry
includes the blocking constraint, measured evidence, risk/effort class, and
scope boundary.

**Provenance**: measurements from `specs/054-profile-polish/research-assets/size-accounting.md`
(section accounting, T038). Baseline: nano 6.45 MiB text, standard 15.38 MiB text.

---

## 1. Gate the `regex-automata` dependency behind a config-parsing feature

**Saving estimate**: ~200–300 KiB text (regex_automata::meta::strategy::new alone is ~22 KiB; total regex pull-in is larger).

**Evidence**: `regex_automata::meta::strategy::new` appears in the top-15 text symbols of ALL profile builds, including nano. Regex is used by the `der` crate (ASN.1 parsing for certificates) and potentially by config file parsing. A nano server with policy None and no config file doesn't need regex at all.

**Blocking constraint**: `der` parsing is needed for X509 certificate handling, which is pulled in by `async-opcua-crypto`. Even nano builds include crypto for the certificate store (though policy None never exercises it). The `CertificateStore::read_crl_dir` symbol (~41 KiB) is present in all builds.

**Risk/effort**: Medium effort. Requires either (a) making the entire crypto/cert-store optional behind a feature that nano/micro don't enable, or (b) replacing the `der` crate's regex usage with hand-written parsers. Option (a) is architecturally cleaner but has broad API impact.

**Scope boundary**: Future feature, not a gate. This changes the dependency tree, not just cfg gates.

## 2. Slim `tokio` to a minimal async runtime for nano/micro

**Saving estimate**: ~300–500 KiB text.

**Evidence**: tokio contributes scheduling, I/O driver, timers, and signal handling. Nano and micro servers use `current_thread` runtime and only need TCP accept + task spawning. The full tokio feature set (`["full"]`) is enabled transitively.

**Blocking constraint**: The server crate requests `tokio = { features = ["full"] }` in the workspace Cargo.toml. Changing this to a narrower feature set (`["rt", "net", "macros", "signal", "time"]`) requires auditing all tokio API usage. The subscription sampler uses `tokio::time::interval`; the connection loop uses `tokio::net`; signal handling uses `tokio::signal`.

**Risk/effort**: Low effort, low risk. Narrow tokio features are a standard embedded-Rust pattern. The main risk is missing a feature flag that a code path depends on, caught by `cargo check`.

**Scope boundary**: Workspace dependency change, not a gate. Affects all builds uniformly.

## 3. Replace `chrono` with a slimmer DateTime crate

**Saving estimate**: ~100–200 KiB text.

**Evidence**: `chrono` is pulled in by `async-opcua-types` for `DateTime` handling. The full chrono crate includes timezone database parsing, formatting, and arithmetic that an OPC UA server doesn't need (OPC UA DateTime is a simple i64 offset from 1601-01-01).

**Blocking constraint**: `chrono` is used pervasively for `DateTime::now()` and duration arithmetic in the session/controller layer. Replacing it requires touching the entire `DateTime` type and all its consumers.

**Risk/effort**: High effort, medium risk. The DateTime type is part of the public API. A custom or `time`-based replacement would need careful API compatibility.

**Scope boundary**: Types crate change, affects all consumers. Future feature.

## 4. Gate config-file parsing behind a `config-json`/`config-toml` feature

**Saving estimate**: ~150–250 KiB text (serde_json + config parsing machinery).

**Evidence**: `ServerBuilder::with_config_from(path)` reads and parses YAML/TOML config files. Nano and micro samples use programmatic builder configuration, never loading a config file. The serde + config parsing machinery is compiled but unused in these profiles.

**Blocking constraint**: `ServerConfig` deserialization is used by the `with_config_from` path, which is a public API. Gating it requires making `ServerConfig` fields programmatic-only when the feature is off, or providing a separate `ServerConfigBuilder`.

**Risk/effort**: Medium effort, low risk. The config-file path is additive — programmatic configuration already works without it.

**Scope boundary**: Server crate gate, similar to existing feature gates. Could be a 16th gate in the feature 054 framework.

## 5. Reduce monomorphization in the encoding/decoding layer

**Saving estimate**: ~200–400 KiB text.

**Evidence**: `<T as der::decode::Decode>::decode` is ~25 KiB per monomorphization. The OPC UA encoding layer generates separate decode functions for every request/response type, many of which are never exercised in nano/micro profiles (e.g., history-related types, event filter types).

**Blocking constraint**: The encoding layer uses a single `decode_by_object_id` dispatch function that monomorphizes over all known types. This is how OPC UA binary decoding works — the dispatch table must list all types even if some services are gated out. The `RequestMessage::decode_by_object_id` symbol (~66 KiB) is the single largest function in the nano binary.

**Risk/effort**: High effort, high risk. Requires restructuring the type registration system to only compile decoders for types whose services are enabled. This is a fundamental architecture change to the codegen layer.

**Scope boundary**: Codegen/types crate change. Future feature, possibly tied to feature 054's gate framework (per-service-type encoding).

## 6. Make panic/backtrace machinery optional via `panic = "abort"` + no unwind

**Saving estimate**: ~100–200 KiB text.

**Evidence**: `<std::backtrace_rs::symbolize::gimli::Cache>::with_global` is ~18 KiB and present in all builds. Panic unwinding machinery contributes additional overhead. The `embedded` cargo profile could set `panic = "abort"` to eliminate unwinding code.

**Blocking constraint**: `panic = "abort"` in the profile means `catch_unwind` doesn't work, which may affect session error isolation. The server uses `catch_unwind` in some places to prevent a single bad request from crashing the server.

**Risk/effort**: Low effort (one line in Cargo.toml profile), but medium risk due to `catch_unwind` interactions. Needs an audit of unwind usage.

**Scope boundary**: Build profile change. Can be offered as an opt-in profile (`[profile.embedded-no-unwind]`).

## 7. Prune the `moka` cache dependency

**Saving estimate**: ~80–120 KiB text.

**Evidence**: `moka::sync::base_cache::Inner::do_run_pending_tasks` is ~21 KiB in all builds. The moka cache is used by the certificate validation cache. For nano/micro profiles that use policy None and don't validate client certificates, this cache is unused.

**Blocking constraint**: The certificate store uses moka for caching validated certificate chains. Removing it requires either making the cert store optional (overlaps with suggestion 1) or replacing the cache with a simpler structure.

**Risk/effort**: Low effort if done as part of suggestion 1 (making crypto optional for nano/micro). Medium effort standalone.

**Scope boundary**: Crypto crate change. Overlaps with suggestion 1.
