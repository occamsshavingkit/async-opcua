# Quickstart: GDS Pull Model Fix (Run 1)

## Prerequisites

```bash
git clone https://github.com/OPCFoundation/UA-Nodeset.git schemas/companion
```

Build/test with `--features companion-gds` (or a feature set that includes
it). Without the companion XML present locally, or without the feature
enabled, none of this feature's behavior exists -- the server builds and
runs exactly as it did before this feature.

## Verify the certificate-issuance workflow works

1. Register a test application (this feature's minimal in-memory
   registration helper, not a full `RegisterApplication` call).
2. Call `StartNewKeyPairRequest` for that application. Confirm a
   `RequestId` is returned.
3. Call `FinishRequest` with that id. Confirm a real, valid certificate and
   a newly generated private key are returned (not placeholders).
4. Call `FinishRequest` again with the same id; confirm
   `Bad_InvalidArgument` (the request was consumed).
5. Call `StartSigningRequest` with a real DER PKCS#10 CSR whose
   `ApplicationUri` matches the registered application; confirm a
   `RequestId` is returned and `FinishRequest` yields a valid certificate
   with no private key.
6. Call `StartSigningRequest` with a CSR whose `ApplicationUri` does not
   match; confirm `Bad_CertificateUriInvalid`.

## Verify the pending/not-found paths

1. (Unit-test level) Construct a `GdsPullRequest` directly in the `Pending`
   state and call `FinishRequest`; confirm `Bad_NothingToDo`.
2. Call `FinishRequest` with an unrecognized `RequestId`; confirm
   `Bad_InvalidArgument`.
3. Call any method with an unregistered `ApplicationId`; confirm
   `Bad_NotFound`.

## Verify discovery/status methods

1. Call `GetCertificateGroups` for a registered application; confirm the
   real `DefaultApplicationGroup` NodeId is returned.
2. Call `GetTrustList` with that group id; confirm the real TrustList
   object NodeId is returned (and, cross-checked against Run 2's TrustList
   work, is a genuine, browsable `TrustListType` instance).
3. Call `GetCertificateStatus`; confirm it reports `UpdateRequired` based
   on real certificate state.

## Verify access control

1. Call each Mandatory method without the required role -> `Bad_UserAccessDenied`.
2. Call `StartSigningRequest`/`StartNewKeyPairRequest`/`FinishRequest` over
   a signed-only (not encrypted) channel -> `Bad_SecurityModeInsufficient`.
3. Call `GetCertificateGroups`/`GetTrustList`/`GetCertificateStatus` over an
   unauthenticated channel -> `Bad_SecurityModeInsufficient`.

## Verify companion-gds isolation

1. Build/test without the `companion-gds` feature; confirm the server
   builds, runs, and passes all other tests exactly as before this
   feature.
2. Enable `companion-gds` without the NodeSet2.xml file present locally;
   confirm the server still starts (the existing `import_companion_xml`
   "warn and return" behavior applies) with no Pull-model methods wired,
   rather than panicking.

## Full verification

```bash
cargo test -p async-opcua-server --features companion-gds --lib gds::pull_methods
cargo test -p async-opcua-server --features companion-gds --lib gds::directory_instance
cargo test -p async-opcua-server --features companion-gds --test gds_pull_companion_integration
cargo test -p async-opcua-server --all-features --lib gds::
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
tools/ci-playbook.sh --ci
```
