# Quickstart: GDS Pull Model Client-Side Fix (Run 2)

Manual verification steps, assuming `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml`
is present locally (per `schemas/companion/README.md`).

## 1. Confirm discovery resolves real NodeIds against a non-default namespace index

```sh
cargo test -p async-opcua-client --test gds_pull_client_discovery -- --nocapture
```

Expect: a real in-process server (GDS companion NodeSet imported) plus a
real client; `GdsClient::discover(&session)` succeeds and the resolved
`directory_object_id`/method NodeIds carry whatever namespace index the
server's companion import actually assigned (not `0`, not any value
hardcoded in test setup) — printed for manual inspection.

## 2. Confirm dispatch reaches the real handlers

Same test: `request_signing_csr` against an unregistered `ApplicationId`
returns `Bad_NotFound` (a real, spec-meaningful status from the server's
actual `StartSigningRequest` handler) rather than `Bad_NodeIdUnknown`/
`Bad_MethodInvalid` (which would mean discovery resolved to nothing real).
`register_application` returns `Bad_NotSupported` (no server-side callback
registered yet, tracked separately) rather than a NodeId-resolution error —
proving the Call reached real dispatch even though the business logic isn't
built.

## 3. Confirm fail-closed behavior against a non-GDS server

Run the same test's negative case: `GdsClient::discover` against a plain
server with no GDS companion NodeSet imported returns a specific `Error`
(not a panic), before any Call is attempted.

## 4. Confirm zero regression elsewhere

```sh
cargo test -p async-opcua-client --all-features
cargo test -p async-opcua-server --all-features
```

Expect: full green — this is a client-only change with no server-side
edits.

## 5. (Optional, manual) Confirm no accidental fabricated-default footgun remains

```sh
grep -rn "22384\|22385\|22388\|22400\|22402" async-opcua-client/src/gds/
```

Expect: no matches — every one of the old fabricated constants is gone.
