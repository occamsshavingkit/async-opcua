# TODO

This is a list of things that are known to be missing, or ideas that could be implemented. Feel free to pick up any of these if you wish to contribute.

 - Flesh out the server and client SDK with tooling for ease if use.
   - Make it even easier to implement custom node managers.
 - ~~Add Nano/Micro/Embedded conformance-profile builds..~~ **Done** — feature 054 added `nano`/`micro`/`embedded`/`standard`
    facade aliases with 15 subsystem cfg gates. Measured: nano 6.45 MiB, micro 6.87 MiB,
    embedded 9.44 MiB, standard 15.97 MiB.
 - ~~Encrypted identity-token secrets: RSA-DH / authenticated-encryption variants~~ **Done** — feature 055
    added RSA-KEM decryption (OPC 10000-6 §6.7.3) with AES-256 Key Wrap. Integration tests
    (specs/055-optional-deps-security, T008-T009) are deferred — need two-phase client connect
    harness for RSA cert provisioning.
 - ~~Implement a better framework for security checks~~ **Done** — feature 055 added
    SecurityCheckRegistry (bounded ring buffer) with recording at cert validation, user
    authentication, channel negotiation, and RBAC decision points. Exposed via ServerHandle.
 - Write a sophisticated server example with a persistent store. This would be a great way to verify the flexibility of the server.
 - Write some "bad ideas" servers, it would be nice to showcase how flexible this is.
 - ~~Write a framework for method calls.~~ **Done** — `async-opcua-server`'s `node_manager::{MethodArg, IntoMethodOutputs, typed_method, typed_method_with_context}` (`method_typed.rs`). Write a method as a typed Rust closure (`typed_method(|name: String, n: i32| -> Result<(String,), StatusCode> { … })`); arguments decode via a `MethodArg` blanket impl over `TryFromVariant`, outputs marshal from a tuple (arity 0..=6), and the adapter returns the Part 4 Call status codes (`BadArgumentsMissing`/`BadTooManyArguments`/`BadInvalidArgument`). Additive over the existing `add_method_callback` path (raw callbacks still work). The demo server uses it (`samples/demo-server/src/methods.rs`).
 - ~~Implement `Query`.~~ **Done** — the server has QueryFirst/QueryNext handlers and the client exposes
   `Session::query_first` / `Session::query_next`; the in-memory/core node manager path has e2e coverage.

## Deferred integration tests (feature 055)

 - RSA-KEM encrypted UserName token integration test (`specs/055-optional-deps-security`, T008-T009):
   needs a full client+server setup with RSA certificates and two-phase secure client connect.
 - Embedded profile secure channel smoke test (feature 054, #[ignore]d): needs two-phase client connect.
 - Standard profile X509/RegisterServer2 tests (feature 054, #[ignore]d): need in-process LDS peer.

## Performance / bounded-time (Big-O) backlog

A complexity-cuts triage (bounded-time on attacker-influenced input) lives in
[`specs/complexity-cuts-backlog.md`](specs/complexity-cuts-backlog.md). Highest value first:

 - **Applied:** the real O(n²) retransmission / publish-request queue cleanup was reduced to O(n).
 - **Deferred unless measured:** retransmission key-indexing, `is_subtype_of` memoization, TranslateBrowsePaths indexing,
   client per-tick subscription recompute, CreateSession per-channel counters, priority-sort caching, and chunk-header reuse.
 - ~~make `async-opcua-pubsub` and `async-opcua-history-sqlite` optional facade deps.~~ **Done** —
    feature 055. Default ON (`default = [\"aws-lc-rs\", \"pubsub\", \"history\"]`), profile aliases
    exclude both. `cargo tree --no-default-features --features nano` shows zero pubsub/history-sqlite.
