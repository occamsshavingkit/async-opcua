# Implementation Plan: Kerberos SSO Integration Test & Keytab Plumbing

**Branch**: `065-kerberos-finish` | **Date**: 2026-07-07 | **Spec**: [spec.md](./spec.md)

## Summary

Two deferred items from feature 064: add a CI integration test with a local MIT KDC, and plumb the `keytab_path` config through to GSSAPI instead of relying on `KRB5_KTNAME` env var.

## Technical Context

**Language/Version**: Rust (edition 2021)
**Primary Dependencies**: `libgssapi` 0.11, MIT Kerberos (`krb5-kdc`, `krb5-admin-server`)
**Storage**: Keytab file on disk (ephemeral for CI)
**Testing**: Integration test in `async-opcua-server/tests/kerberos_sso.rs`
**Target Platform**: Linux (CI: Ubuntu 24.04)
**Project Type**: library (workspace crate)
**Scope**: ~200 lines of new test code, ~50 lines of library changes

## Constitution Check

| Principle | Assessment | Status |
|-----------|-----------|--------|
| I. Correctness Over Completion | Integration test proves end-to-end. Keytab plumbing is fail-closed. | PASS |
| II. Do It Right Once | Direct GSSAPI API call instead of env var hack. One test covers the full flow. | PASS |
| IV. Security Is Paramount | Keytab file permissions validated at startup. Test uses dedicated KDC, not production. | PASS |
| V. Leave It Better Than You Found It | Test adds ~200 lines of coverage. Keytab path is cleaner than env var. | PASS |

## Project Structure

```text
async-opcua-crypto/src/identity/
├── kerberos_validator.rs    # MODIFY: use keytab_path in GSSAPI calls
async-opcua-server/tests/
├── kerberos_sso.rs           # NEW: end-to-end integration test
tools/
├── ci-playbook.sh            # MODIFY: setup_kerberos_kdc() + test job
specs/065-kerberos-finish/
├── plan.md, spec.md, tasks.md
```
