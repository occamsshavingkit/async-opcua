# Research: GDS Pull Model Client-Side Fix (Run 2)

## Current defect (re-verified this session)

`async-opcua-client/src/gds/registration.rs`/`csr.rs` hardcode:

- `GdsRegistrationClient::directory_object_id = NodeId::new(0, 22384)`, `register_method_id = NodeId::new(0, 22385)`
- `GdsCsrClient::certificate_manager_id = NodeId::new(0, 22388)`, `start_signing_request_id = NodeId::new(0, 22400)`, `finish_signing_request_id = NodeId::new(0, 22402)`

None of these resolve to anything on a real server — they're the exact same
class of fabricated constant this SDK's own server-side code had (features
101/103), just never fixed client-side. Worse: even if they *did* happen to
resolve on some specific external server by coincidence, using a fixed
namespace-0 index is definitionally wrong for a companion-spec type, since
namespace 0 is the core specification namespace and GDS is a companion
spec — every real GDS deployment assigns its own index to the GDS
namespace.

## Discovery mechanism: no new session capability needed

Confirmed by reading `async-opcua-client/src/session/mod.rs` fresh this
session (not assumed from memory of feature 103's server-side work):

- `Session::get_namespace_index(&self, url: &str) -> Result<u16, Error>`
  (`session/mod.rs:512`) already exists: checks a local cache first, else
  calls `read_namespace_array()` (`session/mod.rs:482`, a `Read` of
  `VariableId::Server_NamespaceArray`, ns=0;i=2255) and looks up the URI's
  index. Exactly the "find this deployment's index for the GDS namespace"
  primitive this feature needs — no new code required for this step.
- `Session::translate_browse_paths_to_node_ids(&self, browse_paths:
  &[BrowsePath]) -> Result<Vec<BrowsePathResult>, Error>`
  (`session/services/view.rs:595`) already exists and is a thin wrapper over
  the real `TranslateBrowsePathsToNodeIds` service (OPC UA Part 4 §5.8.4) —
  exactly the standard mechanism for resolving a node by a `RelativePath` of
  `(ReferenceType, BrowseName)` hops from a known starting node, without
  needing to know the target's NodeId in advance. `BrowsePath { starting_node:
  NodeId, relative_path: RelativePath { elements:
  Option<Vec<RelativePathElement>> } }`; each `RelativePathElement {
  reference_type_id, is_inverse, include_subtypes, target_name:
  QualifiedName }`. Result: `BrowsePathResult { status_code, targets:
  Option<Vec<BrowsePathTarget>> }`, `BrowsePathTarget { target_id:
  ExpandedNodeId, remaining_path_index }` — `ExpandedNodeId.node_id` gives
  the resolved `NodeId` directly when no cross-server redirection occurred
  (`server_index == 0`, the only case relevant here).

## Discovery plan

1. `gds_ns = session.get_namespace_index("http://opcfoundation.org/UA/GDS/").await?`
   — fails closed with a `BadNoMatch` `Error` (already the behavior of the
   existing method) if the server isn't a GDS at all. Satisfies FR-004/SC-002
   without new error-handling code.
2. One `translate_browse_paths_to_node_ids` call with 4 `BrowsePath`s, all
   starting at `ObjectsFolder` (`ns=0;i=85`, a well-known, universal
   NodeId — never namespace-shifted):
   - Directory: `[(Organizes, QualifiedName(gds_ns, "Directory"))]` (1 hop)
   - RegisterApplication: `[(Organizes, "Directory"), (HasComponent, "RegisterApplication")]` (2 hops)
   - StartSigningRequest: `[(Organizes, "Directory"), (HasComponent, "StartSigningRequest")]` (2 hops)
   - FinishRequest: `[(Organizes, "Directory"), (HasComponent, "FinishRequest")]` (2 hops, and note: the
     real method name is `FinishRequest`, not `FinishSigningRequest` as the
     current client code calls it — corrected as part of this fix)
   All hops forward (`is_inverse: false`), `include_subtypes: true` (a
   client cannot assume how the target server modeled the reference type;
   this project's own server-side dispatch code uses `false` for its own
   internal owns-this-reference check, a different, narrower concern — a
   discovery client should be more permissive).
3. For each `BrowsePathResult`, require `status_code.is_good()` and a
   non-empty `targets`, else fail closed with a specific "could not resolve
   <label>" `Error` naming which node failed (FR-004).
4. Construct `GdsRegistrationClient`/`GdsCsrClient` from the four resolved
   NodeIds and wrap them in a ready `GdsClient`.

## Architectural correction: one Directory object, not two

`GdsCsrClient::certificate_manager_id`'s doc comment calls it "the
CertificateManager object" as if distinct from `GdsRegistrationClient`'s
`directory_object_id`. Per OPC-10000-12 §7.9.2, "CertificateManager" is a
deployment *role* (a server hosting a `CertificateDirectoryType` instance
*is* the CertificateManager), not a separate child object — confirmed
independently by this SDK's own corrected server-side model (feature 104):
`StartSigningRequest`/`FinishRequest`/`RegisterApplication` are all
`HasComponent` children of the exact same real Directory object. Renamed
`certificate_manager_id` → `directory_object_id` in `GdsCsrClient` to match
reality; discovery resolves the Directory object once and both sub-clients
receive the same NodeId, rather than resolving (or fabricating) two.

## Testing: real client vs. real server, non-default namespace

`async-opcua-client/Cargo.toml` already lists `async-opcua-server` as a
dev-dependency, and existing tests (`tests/hostile_server.rs`) already spin
up a real in-process server for client-side tests — no new test
infrastructure pattern needed. The new integration test reuses feature
104's `register_gds_pull_methods_from_companion` to get a real server with
the GDS companion NodeSet imported (companion namespace assigned whatever
index the server's import machinery picks — confirmed in feature 103/104
this is dynamic, not namespace-0, which itself already proves the "every
deployment assigns its own index" premise this feature is grounded on), then
connects a real client and calls `GdsClient::discover` +
`register_application`/`request_signing_csr`/`poll_signing_request`.
`RegisterApplication` has no server-side business logic yet (tracked
separately, out of scope for features 103/104/this one) — its Call still
proves discovery + dispatch correctness by resolving to a real, registered
method node and returning a definitive `Bad_NotSupported` (no callback
registered) rather than `Bad_NodeIdUnknown`/`Bad_MethodInvalid` (node not
found at all), the same "prove the wire reaches something real" pattern
feature 103's own integration test used. `StartSigningRequest` does have
real server-side business logic (feature 103), so that leg can assert a
concrete, spec-meaningful status (`Bad_NotFound` for an unregistered
`ApplicationId`) — the strongest possible proof short of a full round trip
through an unbuilt `RegisterApplication` implementation.

## Alternatives considered

- *Cache discovered NodeIds keyed by session/server URI across `GdsClient`
  instances*: rejected as unnecessary — `GdsClient` already only discovers
  once per instance by construction (there is no code path that
  re-discovers after `discover()` returns), and cross-instance caching would
  add global mutable state for a cost (one Read + one
  TranslateBrowsePaths call) this feature's own spec explicitly frames as
  a one-time, non-hot-path cost.
- *Resolve each node with a separate `TranslateBrowsePathsToNodeIds` call*:
  rejected — batching into one call (4 `BrowsePath`s) is both more
  efficient and is exactly what the service was designed for (an array of
  paths in one request); no reason to make 4 round trips.

## Server-side infrastructure bugs found during testing (not planned in advance)

Writing the real end-to-end test (client vs. this SDK's own server) is what actually surfaced
these -- exactly the "verify empirically" discipline this project's GDS work has consistently
required, not something a design review would have caught.

### Bug 1: `Server_NamespaceArray` never reflects a runtime-imported namespace

`Server_NamespaceArray` (`node_manager/memory/core.rs:1065`, Part 5 §6.3.4) is built by calling
`namespaces_for_user()` on every node manager and merging the results. `InMemoryNodeManager::
namespaces_for_user` (`memory/mod.rs`) delegated purely to the wrapped impl's own **static**
`namespaces()` method -- e.g. `CoreNodeManagerImpl::namespaces()` hardcodes exactly
`["http://opcfoundation.org/UA/"]`, with a comment explicitly noting it doesn't read the live
address space. This is a *different* namespace-reporting path from the one feature 103's
`owns_node`/`refresh_namespaces` fix touched (that fix only affects internal dispatch routing);
this one feeds the array a **remote client reads to discover namespace indices** -- exactly what
this feature's own `GdsClient::discover` depends on. Without a fix, no client (this SDK's own or
any other) could ever discover a namespace added after server startup via any companion import,
making the entire premise of "real, dynamic discovery" this feature exists to build impossible to
actually exercise.

Fixed by changing `InMemoryNodeManager::namespaces_for_user` to merge in any namespace from `Self::
namespaces()` (the wrapper's own refreshable cache, added in feature 103) not already reported by
the impl -- reusing existing, already-correct infrastructure rather than adding new state.

### Bug 2: `AddressSpace`'s and `context.type_tree`'s namespace tables can independently collide

Even after Bug 1's fix, the GDS namespace still didn't appear in `Server_NamespaceArray` --
`DiagnosticsNodeManager::new` (`diagnostics/node_manager.rs:104`) claims a namespace index for the
server's own application URI via `context.type_tree.namespaces_mut().add_namespace(...)`, a
**separate `NamespaceMap`** from `AddressSpace.cold.namespaces` (`address_space/mod.rs:480`,
`self.cold.read().namespaces`). These two tables are maintained completely independently and can
disagree about which index means what. The companion import's namespace-seeding (feature 103's
fix) computes its "next free index" using only `AddressSpace::namespaces()`, which has no idea
`type_tree` already claimed index 1 for the app's own namespace -- so the GDS import also lands on
index 1, and `core.rs:1071`'s naive `HashMap`-collect over both tables' contributions silently
drops one of the two conflicting entries (last-write-wins by iteration order, not a real
resolution). This divergence was invisible before this session because feature 103 was the first
feature to ever actually exercise a runtime companion import in a live server -- the two-tables
problem existed already but nothing had triggered it.

Fixed narrowly: `register_gds_pull_methods_from_companion`'s signature now also accepts `type_tree:
&DefaultTypeTree`, and pre-seeds `AddressSpace`'s namespace table with any namespace `type_tree`
already knows (via the existing public `AddressSpace::add_namespace`) before calling `import_gds`
-- so the companion import's own seeding (which reads `AddressSpace::namespaces()`) sees the full,
accurate picture and can never allocate a colliding index. This is a small, targeted reconciliation
at the one call site that needs it, not a broader unification of the two namespace-tracking
systems (which would be a much larger, separately-scoped architectural change if ever needed
beyond this).

**Both fixes updated 2 existing call sites** (`gds/mod.rs`'s test, `gds_pull_companion_integration.
rs`) to pass `&server_handle.type_tree().read()`; this feature's own new test is the third,
original caller.

### `StartSigningRequest`'s bogus 5th argument (pre-existing client defect, unrelated to NodeIds)

Also found while writing the same test: `start_signing_request` sent 5 input arguments
(`ApplicationId`, `CertificateGroupId`, `CertificateTypeId`, `CertificateRequest`,
`regenerate_private_key: bool`), but the real `StartSigningRequest` (re-verified against
`Opc.Ua.Gds.NodeSet2.xml`'s `InputArguments` variable, `ArrayDimensions="4"`) takes exactly 4 --
there is no "regenerate private key" parameter in the real signature (that's not part of
§7.9.3 at all). The server's own generic argument-count validation
(`InMemoryNodeManager::validate_method_calls`) rejected the extra argument with
`Bad_TooManyArguments` *before* the handler's own auth check ever ran -- itself still valid proof
discovery worked (a `Bad_NodeIdUnknown`/`Bad_MethodInvalid` would mean it hadn't), just a different
status than initially expected. Removed the parameter from `start_signing_request` and
`GdsClient::request_signing_csr` (client-side only, no server change needed for this one).
