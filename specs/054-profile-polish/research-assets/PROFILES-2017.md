# OPC UA 2017 Server Profile grounding (054)

Source: OPC Foundation profile database (profiles.opcfoundation.org REST API,
`api/profile/uri2id` → `includedprofiles`/`includedconformanceunits`), fetched 2026-07-02.
Raw recursive dump: [profiles-2017.json](profiles-2017.json). Profile model semantics:
OPC 10000-7 §4.3/§4.5 (mandatory CUs required; included profiles are mandatory unless
flagged optional).

## Profile stack (2017 family)

| Profile | URI (`http://opcfoundation.org/UA-Profile/...`) | DB id |
|---------|--------------------------------------------------|-------|
| Nano Embedded Device 2017 Server | `Server/NanoEmbeddedDevice2017` | 1657 |
| Micro Embedded Device 2017 Server | `Server/MicroEmbeddedDevice2017` | 1659 |
| Embedded 2017 UA Server | `Server/EmbeddedUA2017` | 1661 |
| Standard 2017 UA Server | `Server/StandardUA2017` | 1663 |

## Nano Embedded Device 2017 Server Profile

Includes (mandatory): **Core 2017 Server Facet**, **UA-TCP UA-SC UA-Binary**.
Direct optional CUs: Base Info Custom Type System, Base Info Diagnostics.

Core 2017 Server Facet — mandatory CUs:
Address Space Atomicity, Address Space Base, **Address Space Full Array Only**,
**Attribute Read**, Base Info Core Structure, **Discovery Find Servers Self**,
**Discovery Get Endpoints**, **SecurityPolicy Support**, Session Base,
Session General Service Behaviour, Session Minimum 1, View Basic,
View Minimum Continuation Point 01, **View RegisterNodes**, **View TranslateBrowsePath**,
plus included facet **User Token – User Name Password Server** (Security Invalid user
token, Security User Name Password) — i.e. username/password tokens are MANDATORY even
at Nano.

Core 2017 Server Facet — optional CUs (selected):
**Attribute Write Values / Attribute Write Index** (write is OPTIONAL at Nano),
Base Info Server Capabilities, Base Services Diagnostics, Security Administration,
Security Role Server Authorization, Session Change User, Address Space Interfaces /
Dictionary Entries / AddIns, Base Info Currency / OptionSet / Selection List /
ValueAsText / Estimated Return Time / Placeholder Modelling Rules.

NOT in Nano at all: subscriptions/monitored items, method Call, events, alarms &
conditions, history, aggregates, Query, NodeManagement, GDS, programs, auditing,
security policies beyond the required policy-negotiation machinery (None acceptable),
multicast discovery.

## Micro Embedded Device 2017 Server Profile

= Nano + direct CU **Session Minimum 2 Parallel** + facet
**Embedded DataChange Subscription Server**:
Monitor Basic, Monitor Items 2, Monitor QueueSize_1, Monitor Value Change,
Subscription Basic, Subscription Minimum 1, Subscription Publish Min 02,
Subscription PublishRequest Queue Overflow.

Still NOT in Micro: deadband filters, triggering, method Call, events, security
policies with certificates (None still acceptable), everything listed as "NOT in Nano"
except basic data-change subscriptions.

## Embedded 2017 UA Server Profile

= Micro + facet **Standard DataChange Subscription 2017 Server**:
Base Info **GetMonitoredItems Method**, Base Info **ResendData Method** (⇒ the Call
service IS required at Embedded), Monitor Items 10/100, Monitor MinQueueSize_02,
**Monitor Triggering**, **Monitored Items Deadband Filter**, Subscription Minimum 02,
Subscription Publish Min 05.

Plus direct CUs: **Base Info Type System** (MANDATORY — types exposed in the address
space), **Security Default ApplicationInstance Certificate** (MANDATORY),
**Security Policy Required** (MANDATORY — a real security policy, not just None),
optional: Base Info Engineering Units, Security – No Application Authentication.

## Standard 2017 UA Server Profile

= Embedded 2017 + facet **Enhanced DataChange Subscription 2017 Server** (Monitor Items
500, Monitor MinQueueSize_05, Subscription Minimum 05, Subscription Publish Min 10 — all
capacity, no new code surface) + facet **User Token – X509 Certificate Server** (Security
User X509, Security Invalid user token) + direct CUs:
**Discovery Register** and **Discovery Register2** (⇒ registering with an LDS — our
existing `discovery-server-registration` feature — is MANDATORY at Standard),
**Session Cancel**, Session Minimum 50 Parallel, View Minimum Continuation Point 05;
optional: Attribute Write StatusCode & Timestamp.

Still NOT mandated even at Standard 2017: eventing/alarms & conditions (Standard Event
Subscription is a separate facet NOT included), method Call beyond
GetMonitoredItems/ResendData, history, aggregates, Query, NodeManagement, auditing, GDS,
programs, diagnostics (still optional). **The full async-opcua server exceeds Standard
2017 by a wide margin** — that surplus is exactly what a `standard` build can omit.

## Consequences for compile-time gating (what each build may omit)

| Subsystem in async-opcua-server | Nano | Micro | Embedded | Standard |
|---------------------------------|------|-------|----------|----------|
| Subscriptions/monitored items | OUT | IN (basic) | IN (+deadband/triggering) | IN (capacity ↑) |
| Method Call service | OUT | OUT | IN (GetMonitoredItems/ResendData) | IN |
| Events / alarms & conditions | OUT | OUT | OUT | OUT |
| History + aggregates | OUT | OUT | OUT | OUT |
| Query, NodeManagement | OUT | OUT | OUT | OUT |
| Diagnostics (Part 5 §6.3) | OUT (optional CU) | OUT | OUT | OUT (optional) |
| RBAC / role administration | OUT (optional CU) | OUT | OUT | OUT (optional) |
| GDS, FOTA, programs | OUT | OUT | OUT | OUT |
| Discovery registration (LDS) | OUT | OUT | OUT | IN (Register/Register2) |
| mDNS discovery | OUT | OUT | OUT | OUT |
| Certificates + security policies | OUT (None only) | OUT (None only) | IN | IN |
| Username/password identity token | IN | IN | IN | IN |
| X509 identity token | OUT | OUT | OUT | IN |
| Session Cancel | OUT | OUT | OUT | IN |
| Attribute Write | optional | optional | IN (kept: Type System-era servers write) | IN |
| Generated core address space | OUT (types optional) | OUT | partial: type system mandatory | partial |

Capacity-only CUs (Session Minimum N, Monitor Items N, queue/publish minimums,
continuation points) are runtime configuration, not compile-time surface — they influence
default limits in the profile samples, not gating.

Note (Embedded / Base Info Type System): the profile mandates exposing the type system,
not shipping the entire ~5,000-node generated nodeset; a reduced/curated type surface
satisfies it. How far that can shrink is part of this feature's "further savings"
analysis.
