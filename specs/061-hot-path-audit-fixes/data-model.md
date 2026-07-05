# Data Model: Hot Path Audit Fixes

No new persistent entities. All changes are internal optimization restructurings:

- **US1**: `Context` type gains `Arc<DecodingOptions>` instead of owned `DecodingOptions`. No API surface change.
- **US2**: `InMemoryNodeManager::init()` loses type-tree/rebuild calls. `Server::initialize_node_managers()` gains post-loop rebuild logic.
- **US3**: `SessionActor` gains two private fields: `cached_context: Option<Arc<RequestContextInner>>` and `context_version: u64`. No public API change.
- **US4**: `SecureChannel` gains `validated_security_policy: bool` field. `expect_supported_security_policy()` is removed or simplified.
- **US5**: `CertificateStore` gains `read_cert_async()` and `read_pkey_async()` methods. No public API removal.
