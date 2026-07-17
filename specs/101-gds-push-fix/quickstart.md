# Quickstart: GDS Push Model Fix + Completion (Run 1)

## Verify the bug is fixed and the real workflow works

1. Connect a session over an encrypted, authenticated channel with
   SecurityAdmin role.
2. Call `CreateSigningRequest` on `ServerConfiguration`
   (`ns=0;i=12637` / method `ns=0;i=12737`). Confirm a non-empty
   `CertificateRequest` is returned.
3. Build a certificate for the request's key (test fixture — a
   self-signed cert reusing the server's own key is sufficient to
   exercise the flow).
4. Call `UpdateCertificate` (`ns=0;i=13737`) with that certificate.
   Confirm `ApplyChangesRequired == true` and the server's active
   certificate is unchanged so far.
5. Call `ApplyChanges` (`ns=0;i=12740`). Confirm success and that the
   server's certificate (readable via `CertificateStore::read_own_cert`)
   has actually changed.
6. Repeat steps 3-4 with a different certificate, then call
   `CancelChanges` (`ns=0;i=25708`) instead of `ApplyChanges`. Confirm the
   server's certificate is unchanged.

## Verify access control

1. Call each of the six methods without SecurityAdmin role → `Bad_UserAccessDenied`.
2. Call `UpdateCertificate`/`CreateSigningRequest` over a signed-only (not
   encrypted) channel → `Bad_SecurityModeInsufficient`.
3. Attempt `UpdateCertificate` from a second session while a transaction
   from a first session is open → `Bad_TransactionPending`.
4. Call `ApplyChanges`/`CancelChanges` with no open transaction →
   `Bad_NothingToDo`.

## Verify GetRejectedList / ResetToServerDefaults

1. Cause a certificate to be rejected (connect with an untrusted client
   cert), then call `GetRejectedList` (`ns=0;i=12777`) and confirm it
   appears in the returned list.
2. Call `ResetToServerDefaults` (`ns=0;i=25709`) and confirm the server
   signals a pending shutdown with a warning message.

## Full verification

```bash
cargo test -p async-opcua-server --lib gds::push_methods
cargo test -p async-opcua-server --test gds_push_integration
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
tools/ci-playbook.sh --ci
```
