# Contracts: observable OPC UA service behavior (053)

The external interface is the OPC UA wire protocol; each row is a client-observable contract,
grounded in the cited spec section.

## US1 — ServerDiagnostics (Part 5 §6.3.3 Table 11)

| Operation | Precondition | Result |
|---|---|---|
| Browse `Server.ServerDiagnostics` | any session | children include `EnabledFlag` (i=2294), `SubscriptionDiagnosticsArray` (i=2290), `SessionsDiagnosticsSummary` (i=3706) with `SessionDiagnosticsArray` (i=3707) + `SessionSecurityDiagnosticsArray` (i=3708) |
| Read `EnabledFlag` | diagnostics-read permitted | Boolean = current collection state |
| Write `EnabledFlag` | privileged session | Good; toggles collection |
| Write `EnabledFlag` | unprivileged session | `Bad_UserAccessDenied` |
| Read `SubscriptionDiagnosticsArray` | enabled, permitted | `SubscriptionDiagnosticsDataType[]`, one per live subscription |
| Read `SessionDiagnosticsArray` | enabled, permitted | `SessionDiagnosticsDataType[]`, one per live session |
| Read `SessionSecurityDiagnosticsArray` | enabled, admin | `SessionSecurityDiagnosticsDataType[]`; non-admin → access denied |
| Read any array | `EnabledFlag == false` | empty array per Part 5 |

## US2 — Write validation (Part 4 §5.11.4; Part 8 §5.3.2.2, §5.3.3.3/.4)

| Write | Constraint | Result |
|---|---|---|
| numeric Value outside EURange | EURange property present | `Bad_OutOfRange`, stored value unchanged |
| numeric Value inside EURange | 〃 | Good |
| enum Value not in EnumDefinition | enum DataType with definition | `Bad_OutOfRange` |
| enum Value in definition | 〃 | Good |
| any Value | no EURange / no enum definition | unchanged behavior (no new rejection) |
| index-ranged element out of range | EURange present | `Bad_OutOfRange` |

## US3 — LocalizedText writes (Part 4 §5.11.4.1)

On non-Value LocalizedText attributes (DisplayName/Description/InverseName):

| Write | Result |
|---|---|
| text + supported locale | that locale added/overwritten; other locales retained; each session reads its negotiated locale |
| null text + stored locale | locale entry deleted; others retained |
| any text + unsupported/invalid locale | `Bad_LocaleNotSupported`, store unchanged |
| text + null locale | default/invariant text updated (not rejected) |

Value attribute: single-locale scalar semantics (documented server-specific per §5.11.4.1).

## US4 — Read maxAge (Part 4 §5.11.2.2 Table 47)

| Read | Source kind | Result |
|---|---|---|
| `maxAge = 0` | callback/sampled | fresh source read before answering |
| `maxAge ≥ 2147483647` | 〃 | cached value permitted |
| `0 < maxAge < max`, cache older | 〃 | refreshed before answering |
| `0 < maxAge`, cache younger | 〃 | cached value permitted |
| any valid maxAge | plain in-memory | current value (always fresh by construction) |
| `maxAge < 0` | any | `Bad_MaxAgeInvalid` (existing, unchanged) |

## US5 — EURange change (Part 8 §5.3.2.2, §5.2)

| Event | Result |
|---|---|
| EURange property written on a monitored Variable | percent-deadband filter uses the NEW range for subsequent evaluations |
| 〃 | next notification per affected item carries StatusCode `SemanticsChanged` bit, exactly once |
| flagged notification discarded by queue overflow | next queued notification carries the bit (Part 4 §7.38.1) |
| EURange change on unrelated node | no bit, no filter change on other items |
| EURange removed/invalid after create | item keeps functioning with last-known range (fail-safe) |

## US6 — AccessLevelEx (Part 3 §5.6.2, §8.60)

| Read attribute 27 | Result |
|---|---|
| on any Variable | UInt32; low byte == AccessLevel |
| on Variable with extended bits configured | corresponding bits set |
| on non-Variable node | `Bad_AttributeIdInvalid` (unchanged) |

## US7 — NamespaceMetadata (Part 5 §6.3.13/§6.3.14)

| Inspect | Result |
|---|---|
| Browse `Server.Namespaces` child | Object of `NamespaceMetadataType` |
| Browse its properties | Variables (PropertyType): NamespaceUri, NamespaceVersion, … |
