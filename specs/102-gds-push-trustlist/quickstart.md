# Quickstart: GDS Push Model TrustList Completion (Run 2)

## Verify the full read/write/apply cycle works

1. Connect a session over an authenticated channel with SecurityAdmin
   role.
2. Call `Open` (`ns=0;i=12647`) with mode `Read` (1) against the TrustList
   (`ns=0;i=12642`). Confirm a non-zero `FileHandle` is returned.
3. Call `Read` (`ns=0;i=12652`) with that handle. Decode the returned
   bytes as `TrustListDataType`; confirm it matches the server's actual
   current trusted/issuer certificates and CRLs.
4. Call `Close` (`ns=0;i=12650`).
5. Call `OpenWithMasks` (`ns=0;i=12663`) with `Masks=1`
   (`TrustedCertificates`). Read and confirm only the trusted-certificates
   field is populated.
6. Call `Open` with mode `Write | EraseExisting` (6). Call `Write`
   (`ns=0;i=12655`) with a new `TrustListDataType` (mask=1, a new trusted
   certificate set). Call `CloseAndUpdate` (`ns=0;i=12666`). Confirm
   `ApplyChangesRequired == true` and the server's actual trusted-cert
   store is unchanged so far.
7. Call `ApplyChanges` (`ns=0;i=12740`, from Run 1). Confirm success and
   that `CertificateStore::read_trusted_certs()` now reflects the new set.
8. Repeat step 6 with a different set, then call `CancelChanges`
   (`ns=0;i=25708`, from Run 1) instead of `ApplyChanges`. Confirm the
   server's trusted-cert store is unchanged.

## Verify AddCertificate / RemoveCertificate

1. Call `AddCertificate` (`ns=0;i=12668`) with a valid certificate's DER
   bytes and `IsTrustedCertificate=true`. Confirm it is immediately
   present in `CertificateStore::read_trusted_certs()`, with no
   `ApplyChanges` call.
2. Call `RemoveCertificate` (`ns=0;i=12670`) with that certificate's
   thumbprint. Confirm it is immediately removed.
3. Attempt to remove a CA certificate still needed to validate another
   certificate in the trusted list; confirm
   `Bad_CertificateChainIncomplete`.

## Verify access control and transaction interplay

1. Call each of the seven methods (excluding `Read`, which requires a
   prior `Open`) without SecurityAdmin role -> `Bad_UserAccessDenied`.
2. Call `Open` in Write mode from a second session while a first
   session's transaction (either a Run 1 certificate-rotation transaction
   or this run's TrustList transaction) is open -> `Bad_TransactionPending`.
3. Call `AddCertificate`/`RemoveCertificate` while a write-mode
   transaction is open on another session -> `Bad_TransactionPending`.
4. Leave a Write-mode handle open past `ActivityTimeout` without calling
   `Close`/`CloseAndUpdate`; confirm the handle is auto-closed and its
   buffer discarded (verified via the handle's `FileHandle` becoming
   invalid on the next `Read`/`Write`/`Close` call against it).

## Full verification

```bash
cargo test -p async-opcua-server --lib gds::trust_list
cargo test -p async-opcua-server --lib gds::push_methods
cargo test -p async-opcua-server --test gds_push_integration
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
tools/ci-playbook.sh --ci
```
