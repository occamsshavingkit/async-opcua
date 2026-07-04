# SESSION HANDOFF — 2026-07-04

## Done — feature 057 (completeness closeout)

Merged to master via #262. 582 tests green.

| US | What |
|----|------|
| OCSP live fetch | new `ocsp` module in async-opcua-crypto, RFC 6960 codec, ureq HTTP, TTL cache, Off/Soft/Strict policy |
| Multi-cert server | per-endpoint certificate per Part 4 §5.5.4.1, backward-compatible, cert-PKI-dir resolution fixed for interop |
| LegacyCall removal | 24 static variants replace dynamic dispatch, all 29 call sites updated, 306 tests pass |
| Example servers | chat (cactuaroid model with Events), chaos (random mutation), filesystem bridge, reverse bridge |

Also shipped on master: live PubSub interop (Rust UADP publisher + subscriber binaries, C# side pending dotnet debugging), CI playbook (`tools/ci-playbook.sh`), backlogs cleaned up, 0.19.0 tag pushed.

## Current CI status (master)

All checks green except `release` — should be fixed now that 0.19.0 tag exists.

## Next — feature 058 (backlog closeout batch)

5 small items from the remaining backlogs:

| # | Item | Where |
|---|------|-------|
| 1 | OCSP responder infrastructure | `async-opcua-crypto/ocsp/` |
| 2 | SDK node-manager tooling | `async-opcua-server/` |
| 3 | RSA-KEM integration test | `async-opcua/tests/` |
| 4 | Embedded profile smoke test | `samples/foundation-profile-*/tests/` |
| 5 | Standard profile X509/RegisterServer2 tests | `samples/foundation-profile-standard-server/tests/` |

**Excluded**: CTT certification run (needs Windows + OPC Foundation CTT).

Start with `/speckit-specify` for feature 058.

## Commands

```bash
tools/ci-playbook.sh --ci    # pre-PR gate
```
