# OPC UA Foundation CU Coverage Report

Status labels are evidence categories, not certification claims. Evidence for
`implemented`/`partial`/`gap` entries comes from a 2026-07-15 code audit (7
independent passes over the codebase, one per subsystem cluster); see the
`Evidence` column for the specific file:line citation behind each verdict.

## Canonical Server Profiles

### Nano Embedded Device 2025 Server Profile

- OPC profile id: `2266`
- URI: `http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2025`
- CU closure size: 51

| CU | Name | Status | Evidence |
|---:|---|---|---|
| 2317 | View TranslateBrowsePath | implemented | TranslateBrowsePathsToNodeIds handler async-opcua-server/src/session/services/view.rs:388; test async-opcua/tests/integration/tier_a.rs:141 |
| 2328 | Discovery Get Endpoints | implemented | get_endpoints_with_filters incl profile-uri filter info.rs:342-378; tests core_tests.rs:100,358,366 |
| 2352 | Discovery Find Servers Self | implemented | FindServers handled async-opcua-server/src/session/controller.rs:716; tests async-opcua/tests/integration/discovery.rs:83,119 |
| 2371 | Protocol UA TCP | implemented | Hello/Ack+TCP codec async-opcua-core/src/comms/tcp_types.rs:244,373; exercised by full opc.tcp integration suite |
| 2389 | Attribute Write Values | implemented | Write handler async-opcua-server/src/session/message_handler.rs:820-852; tests async-opcua/tests/integration/write.rs |
| 2400 | Session Change User | implemented | ActivateSession identity-change + revalidate_monitored_items_for_user manager.rs:1565,1591-1598; test manager.rs:2234-2253 |
| 2407 | Security Administration | implemented | builder.rs: add_user_token:567, SecurityPolicy::None/Sign/SignAndEncrypt:140-195, trust_client_certs:397-398, pki_dir:494; tested security_tests.rs. |
| 2446 | Address Space AddIn Reference | implemented | HasAddIn ReferenceType via generated core nodeset nodeset_19.rs:822, loaded by default address_space/mod.rs:11 |
| 2447 | Address Space AddIn DefaultInstanceBrowsename | implemented | DefaultInstanceBrowseName Property via generated nodeset_21.rs:2832, loaded by default node_manager/memory/core.rs:172 |
| 2476 | Base Info LocalTime | partial | Real computed LocalTime (chrono->TimeZoneDataType) node_manager/memory/core.rs:989-997; no test reads Server_LocalTime attribute |
| 2478 | Time Sync - OS based support | implemented | OsClockSource default TimeSyncSource impl async-opcua-server/src/time_sync.rs:112-124; unit test time_sync.rs:130-137 |
| 2479 | Time Sync - IEEE 1588 (PTP) | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2480 | Time Sync - IEEE 802.1AS | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2600 | SecurityPolicy Support | implemented | 10+ SecurityPolicy variants incl None async-opcua-crypto/src/security_policy.rs:125-150; extensively tested + CI conformance matrix |
| 2711 | Base Info Selection List | implemented | base_info::create_selection_list_variable instantiates SelectionListType with Selections/SelectionDescriptions/RestrictToList; test base_info.rs::selection_list_exposes_selections_descriptions_and_restrict_flag |
| 2786 | Time Sync - NTP | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2808 | Security Role Server Authorization | implemented | Opt-in RBAC enforcement async-opcua-server/src/rbac/decision.rs:46-81; dedicated suite async-opcua/tests/integration/rbac.rs |
| 2809 | Address Space Atomicity | implemented | AccessLevelExType NonatomicRead/Write async-opcua-nodes/src/variable.rs:62,827-837; unit test variable.rs:990-997 |
| 2820 | Address Space Full Array Only | implemented | validate_node_write_inner (address_space/write_validation.rs) rejects an IndexRange Write to AttributeId::Value with Bad_WriteNotSupported when AccessLevelExType::WriteFullArrayOnly is set; test write.rs::write_index_range_rejected_when_write_full_array_only |
| 2837 | UA Binary Encoding | implemented | BinaryEncodable/BinaryDecodable traits async-opcua-types/src/encoding.rs:445-482, pervasive derive use; tests encoding.rs:919 |
| 2853 | UA Secure Conversation | implemented | SecureChannel/OpenSecureChannel comms/secure_channel.rs:657; tests secure_channel.rs:136-663, integration secure_channel.rs:15 |
| 2936 | Attribute Write StatusCode & Timestamp | implemented | write_node_value (address_space/utils.rs) threads client status/source_timestamp/server_timestamp through to Variable::set_value_range (fixed a real bug: server_timestamp was hardcoded to now()); test write.rs::write_status_code_and_timestamps_round_trip |
| 2969 | Base Info ValueAsText | implemented | base_info::create_enum_variable_with_value_as_text/update_enum_value attach a ValueAsText property kept in sync with an enumerated Variable's Value; test base_info.rs::value_as_text_tracks_enumerated_value_changes |
| 3072 | Attribute Read | implemented | Read applies IndexRange via NumericRange::range_of node_manager/memory/core.rs:1079-1080; tests read.rs:1425,794 |
| 3073 | View RegisterNodes | implemented | RegisterNodes/UnregisterNodes handler session/services/view.rs:540, memory_mgr_impl.rs:1608; e2e test browse.rs:675 |
| 3080 | Security Default ApplicationInstance Certificate | implemented | CertificateStore::create_and_store_application_instance_cert certificate_store.rs:265, default builder.rs:119; test crypto.rs:46 |
| 3127 | Base Info OptionSet | implemented | base_info::create_option_set_variable instantiates OptionSetType with OptionSetValues/BitMask; test base_info.rs::option_set_exposes_per_bit_values_and_bitmask |
| 3147 | Attribute Write Index | implemented | Variant::set_range_of variant/mod.rs:1641 via Variable::set_value_range variable.rs:746; test write.rs:688,1008 |
| 3175 | Session Base | implemented | CreateSession/ActivateSession/CloseSession session/manager.rs; SecurityMode::None optional cert/nonce manager.rs:283-300; test :47,90 |
| 3184 | Base Info Core Structure 2 | implemented | Root/Objects/Server + ServerArray/NamespaceArray/ServiceLevel node_manager/memory/core.rs:986-1063; tests browse.rs:35, read.rs:42-43 |
| 3186 | Base Info Core Views Folder | implemented | ViewsFolder entry point address_space/mod.rs:774-779; test at same location |
| 3192 | Base Info Diagnostics | implemented | EnabledFlag/ServerDiagnosticsSummary/SubscriptionDiagnosticsArray diagnostics/server.rs, core.rs:501-509; e2e read.rs:1604-1841 |
| 3198 | Base Info Estimated Return Time | implemented | ServerStatusWrapper::schedule_shutdown/estimated_return_time (server_status.rs) + ServerHandle::shutdown_after_with_return_time (server_handle.rs) extend the existing shutdown mechanism; wired core.rs get_attribute; test base_info.rs::estimated_return_time_reflects_scheduled_shutdown_and_is_null_otherwise |
| 3201 | Base Info Custom Type System | partial | custom-codegen sample (samples/custom-codegen) demonstrates a full custom-type inheritance tree + generated Encoding Objects via async-opcua-codegen (types/encoding_ids.rs, types/gen.rs); no completeness e2e test proving all custom EventTypes are exposed alongside their encoding objects. Distinct from CU 5801 (which covers standard-nodeset type completeness, closed as a byproduct of the many typed-instantiation CUs) -- this one is specifically about CUSTOM (non-standard) types and remains open |
| 3530 | View Basic 2 | implemented | Browse/BrowseNext w/ continuation points view.rs:213; tests browse.rs:252, :757 (Bad_ContinuationPointInvalid) |
| 3545 | Base Info Namespace Metadata | implemented | Dynamic per-namespace NamespaceMetaData objects diagnostics/node_manager.rs:583-650; e2e test browse.rs:942-967 |
| 3554 | Address Space Base | implemented | Core AddressSpace all NodeClasses address_space/mod.rs (1454 LOC, unit tests) + opcua-nodes crate; e2e browse.rs:144-167 |
| 3560 | Address Space Interfaces | implemented | base_info::add_ordered_object attaches HasInterface from each OrderedListType child to IOrderedObjectType (a byproduct of closing CU 2512); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 3721 | Security ECC Policy | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3802 | Time Sync - Configure Clock Skew | implemented | ServerConfig::max_acceptable_clock_skew_ns config/server.rs:669,998-1002; tests config/server.rs:375-432 |
| 3808 | Documentation - Core Capacities | implemented | docs/server-capacity-limits.md enumerates every Limits/SubscriptionLimits/OperationalLimits field with its default and configuration method, cross-checked against config/limits.rs's Default impls and the server_conf_limits_match_struct_field_names test |
| 3912 | Base Info Server Capabilities 2 | implemented | core.rs get_attribute wires MaxSessions to Limits.max_sessions (was the only unwired node in this CU per prior audit); test read.rs::server_capabilities_max_sessions_reports_configured_limit |
| 3983 | Base Services Diagnostics | implemented | result.rs:17-58 filter_diagnostic_info masks diag bits; wired attribute.rs/node_management.rs; test per_op_diagnostics.rs |
| 3985 | Session General Service Behaviour | implemented | controller.rs:396 auth-token check, response.rs:207 requestHandle echo, deadline_queue:971-1016 BadTimeout; e2e read.rs:1400-1408 |
| 4053 | Base Info Locations Object | implemented | Locations object (i=31915, nodeset_16.rs:918-943) confirmed reachable via Browse from ObjectsFolder; test browse.rs::locations_object_is_reachable_from_objects_folder |
| 4237 | Address Space NonVolatile and Constant | implemented | NonVolatile/Constant bits defined enums.rs:15-19, generic get/set variable.rs:826-838; test write.rs::access_level_ex_non_volatile_and_constant_round_trip |
| 5240 | Base Info Currency | implemented | base_info::create_currency_variable attaches a CurrencyUnit property (CurrencyUnitType) to a monetary DataVariable; test base_info.rs::currency_unit_property_reports_iso4217_fields |
| 5505 | Time Sync - UA based support | implemented | UaHeaderTimeSyncSource polls ResponseHeader.timestamp (time_sync_ua.rs:52-80), configurable builder.rs:258-262; test time_sync.rs:33 |
| 5592 | Missing from normalized CU list | source-issue | Referenced by closure but absent from conformance_units. |
| 5793 | Time Sync - Support | implemented | OsClockSource (time_sync.rs:112-124) + UA-based source satisfy facet; docs/time-synchronization.md:9-17; tests time_sync.rs:11-22 |
| 5814 | Security - No Application Authentication | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |

### Micro Embedded Device 2025 Server Profile

- OPC profile id: `2267`
- URI: `http://opcfoundation.org/UA-Profile/Server/MicroEmbeddedDevice2025`
- CU closure size: 62

| CU | Name | Status | Evidence |
|---:|---|---|---|
| 2317 | View TranslateBrowsePath | implemented | TranslateBrowsePathsToNodeIds handler async-opcua-server/src/session/services/view.rs:388; test async-opcua/tests/integration/tier_a.rs:141 |
| 2328 | Discovery Get Endpoints | implemented | get_endpoints_with_filters incl profile-uri filter info.rs:342-378; tests core_tests.rs:100,358,366 |
| 2352 | Discovery Find Servers Self | implemented | FindServers handled async-opcua-server/src/session/controller.rs:716; tests async-opcua/tests/integration/discovery.rs:83,119 |
| 2371 | Protocol UA TCP | implemented | Hello/Ack+TCP codec async-opcua-core/src/comms/tcp_types.rs:244,373; exercised by full opc.tcp integration suite |
| 2389 | Attribute Write Values | implemented | Write handler async-opcua-server/src/session/message_handler.rs:820-852; tests async-opcua/tests/integration/write.rs |
| 2400 | Session Change User | implemented | ActivateSession identity-change + revalidate_monitored_items_for_user manager.rs:1565,1591-1598; test manager.rs:2234-2253 |
| 2407 | Security Administration | implemented | builder.rs: add_user_token:567, SecurityPolicy::None/Sign/SignAndEncrypt:140-195, trust_client_certs:397-398, pki_dir:494; tested security_tests.rs. |
| 2446 | Address Space AddIn Reference | implemented | HasAddIn ReferenceType via generated core nodeset nodeset_19.rs:822, loaded by default address_space/mod.rs:11 |
| 2447 | Address Space AddIn DefaultInstanceBrowsename | implemented | DefaultInstanceBrowseName Property via generated nodeset_21.rs:2832, loaded by default node_manager/memory/core.rs:172 |
| 2476 | Base Info LocalTime | partial | Real computed LocalTime (chrono->TimeZoneDataType) node_manager/memory/core.rs:989-997; no test reads Server_LocalTime attribute |
| 2478 | Time Sync - OS based support | implemented | OsClockSource default TimeSyncSource impl async-opcua-server/src/time_sync.rs:112-124; unit test time_sync.rs:130-137 |
| 2479 | Time Sync - IEEE 1588 (PTP) | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2480 | Time Sync - IEEE 802.1AS | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2600 | SecurityPolicy Support | implemented | 10+ SecurityPolicy variants incl None async-opcua-crypto/src/security_policy.rs:125-150; extensively tested + CI conformance matrix |
| 2711 | Base Info Selection List | implemented | base_info::create_selection_list_variable instantiates SelectionListType with Selections/SelectionDescriptions/RestrictToList; test base_info.rs::selection_list_exposes_selections_descriptions_and_restrict_flag |
| 2786 | Time Sync - NTP | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2808 | Security Role Server Authorization | implemented | Opt-in RBAC enforcement async-opcua-server/src/rbac/decision.rs:46-81; dedicated suite async-opcua/tests/integration/rbac.rs |
| 2809 | Address Space Atomicity | implemented | AccessLevelExType NonatomicRead/Write async-opcua-nodes/src/variable.rs:62,827-837; unit test variable.rs:990-997 |
| 2820 | Address Space Full Array Only | implemented | validate_node_write_inner (address_space/write_validation.rs) rejects an IndexRange Write to AttributeId::Value with Bad_WriteNotSupported when AccessLevelExType::WriteFullArrayOnly is set; test write.rs::write_index_range_rejected_when_write_full_array_only |
| 2837 | UA Binary Encoding | implemented | BinaryEncodable/BinaryDecodable traits async-opcua-types/src/encoding.rs:445-482, pervasive derive use; tests encoding.rs:919 |
| 2853 | UA Secure Conversation | implemented | SecureChannel/OpenSecureChannel comms/secure_channel.rs:657; tests secure_channel.rs:136-663, integration secure_channel.rs:15 |
| 2936 | Attribute Write StatusCode & Timestamp | implemented | write_node_value (address_space/utils.rs) threads client status/source_timestamp/server_timestamp through to Variable::set_value_range (fixed a real bug: server_timestamp was hardcoded to now()); test write.rs::write_status_code_and_timestamps_round_trip |
| 2963 | Monitor Basic | implemented | create/modify/delete_monitored_items + set_monitoring_mode (session/services/monitored_items.rs:170-573); tested subscriptions.rs. |
| 2969 | Base Info ValueAsText | implemented | base_info::create_enum_variable_with_value_as_text/update_enum_value attach a ValueAsText property kept in sync with an enumerated Variable's Value; test base_info.rs::value_as_text_tracks_enumerated_value_changes |
| 3072 | Attribute Read | implemented | Read applies IndexRange via NumericRange::range_of node_manager/memory/core.rs:1079-1080; tests read.rs:1425,794 |
| 3073 | View RegisterNodes | implemented | RegisterNodes/UnregisterNodes handler session/services/view.rs:540, memory_mgr_impl.rs:1608; e2e test browse.rs:675 |
| 3080 | Security Default ApplicationInstance Certificate | implemented | CertificateStore::create_and_store_application_instance_cert certificate_store.rs:265, default builder.rs:119; test crypto.rs:46 |
| 3127 | Base Info OptionSet | implemented | base_info::create_option_set_variable instantiates OptionSetType with OptionSetValues/BitMask; test base_info.rs::option_set_exposes_per_bit_values_and_bitmask |
| 3143 | Subscription PublishRequest Queue Overflow | implemented | enqueue_publish_request pops oldest on overflow, returns BadTooManyPublishRequests (session_subscriptions.rs:767); test :1581. |
| 3147 | Attribute Write Index | implemented | Variant::set_range_of variant/mod.rs:1641 via Variable::set_value_range variable.rs:746; test write.rs:688,1008 |
| 3175 | Session Base | implemented | CreateSession/ActivateSession/CloseSession session/manager.rs; SecurityMode::None optional cert/nonce manager.rs:283-300; test :47,90 |
| 3184 | Base Info Core Structure 2 | implemented | Root/Objects/Server + ServerArray/NamespaceArray/ServiceLevel node_manager/memory/core.rs:986-1063; tests browse.rs:35, read.rs:42-43 |
| 3186 | Base Info Core Views Folder | implemented | ViewsFolder entry point address_space/mod.rs:774-779; test at same location |
| 3192 | Base Info Diagnostics | implemented | EnabledFlag/ServerDiagnosticsSummary/SubscriptionDiagnosticsArray diagnostics/server.rs, core.rs:501-509; e2e read.rs:1604-1841 |
| 3196 | Base Info Fixed SamplingInterval | implemented | CU is conditional on the Server using a fixed set of sampling intervals (OPC-10000-5 SS7.9/SS12.8); this server negotiates a continuously-variable client-requested interval per monitored item (sanitize_sampling_interval, subscriptions/monitored_item.rs:299-311), so the precondition never holds and non-exposure of SamplingIntervalDiagnosticsArray is spec-conformant, not a gap -- documented in docs/server-capacity-limits.md |
| 3198 | Base Info Estimated Return Time | implemented | ServerStatusWrapper::schedule_shutdown/estimated_return_time (server_status.rs) + ServerHandle::shutdown_after_with_return_time (server_handle.rs) extend the existing shutdown mechanism; wired core.rs get_attribute; test base_info.rs::estimated_return_time_reflects_scheduled_shutdown_and_is_null_otherwise |
| 3201 | Base Info Custom Type System | partial | custom-codegen sample (samples/custom-codegen) demonstrates a full custom-type inheritance tree + generated Encoding Objects via async-opcua-codegen (types/encoding_ids.rs, types/gen.rs); no completeness e2e test proving all custom EventTypes are exposed alongside their encoding objects. Distinct from CU 5801 (which covers standard-nodeset type completeness, closed as a byproduct of the many typed-instantiation CUs) -- this one is specifically about CUSTOM (non-standard) types and remains open |
| 3530 | View Basic 2 | implemented | Browse/BrowseNext w/ continuation points view.rs:213; tests browse.rs:252, :757 (Bad_ContinuationPointInvalid) |
| 3545 | Base Info Namespace Metadata | implemented | Dynamic per-namespace NamespaceMetaData objects diagnostics/node_manager.rs:583-650; e2e test browse.rs:942-967 |
| 3554 | Address Space Base | implemented | Core AddressSpace all NodeClasses address_space/mod.rs (1454 LOC, unit tests) + opcua-nodes crate; e2e browse.rs:144-167 |
| 3560 | Address Space Interfaces | implemented | base_info::add_ordered_object attaches HasInterface from each OrderedListType child to IOrderedObjectType (a byproduct of closing CU 2512); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 3721 | Security ECC Policy | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3727 | Subscription Basic | implemented | CreateSubscription/Publish/Republish/SetPublishingMode etc implemented (subscriptions/session_subscriptions.rs); tested subscriptions.rs. |
| 3802 | Time Sync - Configure Clock Skew | implemented | ServerConfig::max_acceptable_clock_skew_ns config/server.rs:669,998-1002; tests config/server.rs:375-432 |
| 3808 | Documentation - Core Capacities | implemented | docs/server-capacity-limits.md enumerates every Limits/SubscriptionLimits/OperationalLimits field with its default and configuration method, cross-checked against config/limits.rs's Default impls and the server_conf_limits_match_struct_field_names test |
| 3911 | Base Info Server Capabilities Subscriptions | implemented | core.rs get_attribute now wires MaxMonitoredItemsPerSubscription/MaxSubscriptionsPerSession to their SubscriptionLimits config fields, and MaxSubscriptions/MaxMonitoredItems (no server-wide cap exists) report spec-valid 0 per OPC-10000-5 SS6.3.2; tests read.rs::server_capabilities_max_monitored_items_per_subscription_and_max_subscriptions_per_session, ::server_capabilities_server_wide_max_subscriptions_and_max_monitored_items_are_zero |
| 3912 | Base Info Server Capabilities 2 | implemented | core.rs get_attribute wires MaxSessions to Limits.max_sessions (was the only unwired node in this CU per prior audit); test read.rs::server_capabilities_max_sessions_reports_configured_limit |
| 3913 | Subscription Publish Basic | implemented | max_publish_requests_per_subscription=4 (server/src/lib.rs:227); Publish exercised across tests/integration/subscriptions.rs. |
| 3922 | Base Info SemanticChange Bit | implemented | SemanticsChanged bit set monitored_item.rs:1012-1042 via EU-range writes session_subscriptions.rs:1238,1290; tests :1668 |
| 3923 | Session Multiple | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3983 | Base Services Diagnostics | implemented | result.rs:17-58 filter_diagnostic_info masks diag bits; wired attribute.rs/node_management.rs; test per_op_diagnostics.rs |
| 3985 | Session General Service Behaviour | implemented | controller.rs:396 auth-token check, response.rs:207 requestHandle echo, deadline_queue:971-1016 BadTimeout; e2e read.rs:1400-1408 |
| 4053 | Base Info Locations Object | implemented | Locations object (i=31915, nodeset_16.rs:918-943) confirmed reachable via Browse from ObjectsFolder; test browse.rs::locations_object_is_reachable_from_objects_folder |
| 4055 | Base Info Server Capabilities MaxMonitoredItemsQueueSize | implemented | core.rs get_attribute wires MaxMonitoredItemsQueueSize to SubscriptionLimits.max_monitored_item_queue_size, the same limit already enforced at monitored_item.rs:314; test read.rs::server_capabilities_max_monitored_items_queue_size_reports_configured_limit |
| 4237 | Address Space NonVolatile and Constant | implemented | NonVolatile/Constant bits defined enums.rs:15-19, generic get/set variable.rs:826-838; test write.rs::access_level_ex_non_volatile_and_constant_round_trip |
| 5207 | Monitor Items 2 | implemented | No per-subscription item cap below 2 found (server/src/config/limits.rs); 2+ Double items trivially exercised in subscriptions.rs. |
| 5208 | Monitor Value Change V2 | partial | IndexRange applied to sample monitored_item.rs:931-940 (Variant::range_of); logic tested via read.rs:794-827, no MonitoredItem-level test |
| 5240 | Base Info Currency | implemented | base_info::create_currency_variable attaches a CurrencyUnit property (CurrencyUnitType) to a monetary DataVariable; test base_info.rs::currency_unit_property_reports_iso4217_fields |
| 5505 | Time Sync - UA based support | implemented | UaHeaderTimeSyncSource polls ResponseHeader.timestamp (time_sync_ua.rs:52-80), configurable builder.rs:258-262; test time_sync.rs:33 |
| 5592 | Missing from normalized CU list | source-issue | Referenced by closure but absent from conformance_units. |
| 5793 | Time Sync - Support | implemented | OsClockSource (time_sync.rs:112-124) + UA-based source satisfy facet; docs/time-synchronization.md:9-17; tests time_sync.rs:11-22 |
| 5814 | Security - No Application Authentication | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |

### Embedded 2025 UA Server Profile

- OPC profile id: `2268`
- URI: `http://opcfoundation.org/UA-Profile/Server/EmbeddedUA2025`
- CU closure size: 119

| CU | Name | Status | Evidence |
|---:|---|---|---|
| 2231 | Push Model for Global Certificate and TrustList Management | implemented | Feature 101 (Run 1): fixed fabricated-NodeId/wrong-node-manager bugs in push_methods.rs; real ServerConfigurationType handlers wired to verified NodeIds: CreateSigningRequest (ns=0;i=12737, real PKCS#10 CSR), UpdateCertificate (13737), ApplyChanges (12740), CancelChanges (25708), GetRejectedList (12777), ResetToServerDefaults (25709); e2e-proven via gds_push_integration.rs + unit tests in push_methods.rs. CreateSelfSignedCertificate/DeleteCertificate/GetCertificates confirmed absent from the generated nodeset (SourceIssue). Feature 102 (Run 2): closes the remaining TrustList/CertificateGroup surface for DefaultApplicationGroup -- new gds/trust_list.rs wires Open (12647), OpenWithMasks (12663), Read (12652), Write (12655), GetPosition (12657), SetPosition (12660), Close (12650), CloseAndUpdate (12666), AddCertificate (12668), RemoveCertificate (12670), all empirically verified live against DefaultApplicationGroup.TrustList (ns=0;i=12642); PushTransaction extended to share ApplyChanges/CancelChanges with the TrustList's pending change; CertificateStore gained write-side trusted/issuer cert+CRL helpers (store_trusted_cert/store_issuer_cert/remove_trusted_cert/remove_issuer_cert/store_trusted_crl/store_issuer_crl/replace_*); 30 unit tests across push_methods.rs+trust_list.rs plus an extended gds_push_integration.rs wire-dispatch proof. DefaultHttpsGroup/DefaultUserTokenGroup CertificateGroups exist in the generated nodeset but are out of scope for this feature (documented follow-up); CertificateGroupType.GetRejectedList remains absent (SourceIssue, satisfied via ServerConfiguration.GetRejectedList). Sibling bug in pull_methods.rs (CU 3582, same fabricated-NodeId pattern) fixed by Feature 103 (Run 1). |
| 2317 | View TranslateBrowsePath | implemented | TranslateBrowsePathsToNodeIds handler async-opcua-server/src/session/services/view.rs:388; test async-opcua/tests/integration/tier_a.rs:141 |
| 2328 | Discovery Get Endpoints | implemented | get_endpoints_with_filters incl profile-uri filter info.rs:342-378; tests core_tests.rs:100,358,366 |
| 2352 | Discovery Find Servers Self | implemented | FindServers handled async-opcua-server/src/session/controller.rs:716; tests async-opcua/tests/integration/discovery.rs:83,119 |
| 2371 | Protocol UA TCP | implemented | Hello/Ack+TCP codec async-opcua-core/src/comms/tcp_types.rs:244,373; exercised by full opc.tcp integration suite |
| 2389 | Attribute Write Values | implemented | Write handler async-opcua-server/src/session/message_handler.rs:820-852; tests async-opcua/tests/integration/write.rs |
| 2400 | Session Change User | implemented | ActivateSession identity-change + revalidate_monitored_items_for_user manager.rs:1565,1591-1598; test manager.rs:2234-2253 |
| 2407 | Security Administration | implemented | builder.rs: add_user_token:567, SecurityPolicy::None/Sign/SignAndEncrypt:140-195, trust_client_certs:397-398, pki_dir:494; tested security_tests.rs. |
| 2423 | Base Info Rational Number | implemented | RationalNumberType present schemas/1.05/Opc.Ua.NodeSet2.xml, generated types/rational_number.rs; exposed via CoreNamespace import. |
| 2446 | Address Space AddIn Reference | implemented | HasAddIn ReferenceType via generated core nodeset nodeset_19.rs:822, loaded by default address_space/mod.rs:11 |
| 2447 | Address Space AddIn DefaultInstanceBrowsename | implemented | DefaultInstanceBrowseName Property via generated nodeset_21.rs:2832, loaded by default node_manager/memory/core.rs:172 |
| 2476 | Base Info LocalTime | partial | Real computed LocalTime (chrono->TimeZoneDataType) node_manager/memory/core.rs:989-997; no test reads Server_LocalTime attribute |
| 2478 | Time Sync - OS based support | implemented | OsClockSource default TimeSyncSource impl async-opcua-server/src/time_sync.rs:112-124; unit test time_sync.rs:130-137 |
| 2479 | Time Sync - IEEE 1588 (PTP) | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2480 | Time Sync - IEEE 802.1AS | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2481 | Base Info NormalizedString DataType | implemented | NormalizedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import (core.rs:147). |
| 2482 | Base Info DecimalString DataType | implemented | DecimalString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2483 | Base Info Date DataTypes | implemented | DurationString/TimeString/DateString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2484 | Base Info BitFieldMaskDataType | implemented | BitFieldMaskDataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2485 | Base Info KeyValuePair | implemented | KeyValuePair in nodeset + generated types/key_value_pair.rs; used by published_data_set_data_type.rs. |
| 2490 | Base Info Subvariables of Structures | implemented | HasStructuredComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2491 | Base Info AssociatedWith | implemented | AssociatedWith present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2500 | Base Info EUInformation | implemented | EUInformation used/tested: tests/integration/custom_types.rs, async-opcua-types/src/tests/json.rs:344. |
| 2512 | Base Info OrderedList | implemented | base_info::create_ordered_list_in_address_space/add_ordered_object instantiate OrderedListType with HasOrderedComponent children implementing IOrderedObjectType via HasInterface (NumberInList is the authoritative order signal, not Browse response order, per OPC-10000-5 SS6.11's own rationale); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 2513 | Base Info Audio Type | implemented | AudioVariableType/AudioDataType present schemas/1.05/Opc.Ua.NodeSet2.xml; type-level exposure via CoreNamespace import. |
| 2514 | Base Info Spatial Data | implemented | VectorType/CartesianCoordinatesType/OrientationType/FrameType present in schemas/1.05; exposed via CoreNamespace import. |
| 2516 | Base Info HasOrderedComponent | implemented | HasOrderedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2517 | Base Info Deprecated Information | implemented | IsDeprecated present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2518 | Base Info Image DataTypes | implemented | ImageBMP/GIF/JPG/PNG present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2536 | Base Info ContentFilter | implemented | ContentFilter/Element DataTypes+encodings (node_ids.rs:168,7132); real WhereClause use+tests in where_clause.rs:13-56, select.rs:14-79. |
| 2600 | SecurityPolicy Support | implemented | 10+ SecurityPolicy variants incl None async-opcua-crypto/src/security_policy.rs:125-150; extensively tested + CI conformance matrix |
| 2711 | Base Info Selection List | implemented | base_info::create_selection_list_variable instantiates SelectionListType with Selections/SelectionDescriptions/RestrictToList; test base_info.rs::selection_list_exposes_selections_descriptions_and_restrict_flag |
| 2786 | Time Sync - NTP | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2808 | Security Role Server Authorization | implemented | Opt-in RBAC enforcement async-opcua-server/src/rbac/decision.rs:46-81; dedicated suite async-opcua/tests/integration/rbac.rs |
| 2809 | Address Space Atomicity | implemented | AccessLevelExType NonatomicRead/Write async-opcua-nodes/src/variable.rs:62,827-837; unit test variable.rs:990-997 |
| 2820 | Address Space Full Array Only | implemented | validate_node_write_inner (address_space/write_validation.rs) rejects an IndexRange Write to AttributeId::Value with Bad_WriteNotSupported when AccessLevelExType::WriteFullArrayOnly is set; test write.rs::write_index_range_rejected_when_write_full_array_only |
| 2823 | Security Invalid user token | partial | Fixed 100ms tarpit on every auth failure (session/negotiate.rs:16,28-40; tested security_tests.rs:2429); no escalating lockout. |
| 2837 | UA Binary Encoding | implemented | BinaryEncodable/BinaryDecodable traits async-opcua-types/src/encoding.rs:445-482, pervasive derive use; tests encoding.rs:919 |
| 2853 | UA Secure Conversation | implemented | SecureChannel/OpenSecureChannel comms/secure_channel.rs:657; tests secure_channel.rs:136-663, integration secure_channel.rs:15 |
| 2863 | Security Policy Required | implemented | Modern policies default-on, legacy Basic128Rsa15/Basic256 opt-in behind legacy-crypto feature builder.rs:142-166; matrix test |
| 2928 | Monitored Items Deadband Filter | implemented | Absolute DataChangeFilter deadband subscriptions/monitored_item/filters.rs:128-137; unit test filters.rs:175 |
| 2936 | Attribute Write StatusCode & Timestamp | implemented | write_node_value (address_space/utils.rs) threads client status/source_timestamp/server_timestamp through to Variable::set_value_range (fixed a real bug: server_timestamp was hardcoded to now()); test write.rs::write_status_code_and_timestamps_round_trip |
| 2940 | Base Info GetMonitoredItems Method | implemented | GetMonitoredItems method node_manager/memory/core.rs:1195-1207; test methods.rs:291-332 call_get_monitored_items |
| 2963 | Monitor Basic | implemented | create/modify/delete_monitored_items + set_monitoring_mode (session/services/monitored_items.rs:170-573); tested subscriptions.rs. |
| 2969 | Base Info ValueAsText | implemented | base_info::create_enum_variable_with_value_as_text/update_enum_value attach a ValueAsText property kept in sync with an enumerated Variable's Value; test base_info.rs::value_as_text_tracks_enumerated_value_changes |
| 3072 | Attribute Read | implemented | Read applies IndexRange via NumericRange::range_of node_manager/memory/core.rs:1079-1080; tests read.rs:1425,794 |
| 3073 | View RegisterNodes | implemented | RegisterNodes/UnregisterNodes handler session/services/view.rs:540, memory_mgr_impl.rs:1608; e2e test browse.rs:675 |
| 3080 | Security Default ApplicationInstance Certificate | implemented | CertificateStore::create_and_store_application_instance_cert certificate_store.rs:265, default builder.rs:119; test crypto.rs:46 |
| 3127 | Base Info OptionSet | implemented | base_info::create_option_set_variable instantiates OptionSetType with OptionSetValues/BitMask; test base_info.rs::option_set_exposes_per_bit_values_and_bitmask |
| 3143 | Subscription PublishRequest Queue Overflow | implemented | enqueue_publish_request pops oldest on overflow, returns BadTooManyPublishRequests (session_subscriptions.rs:767); test :1581. |
| 3146 | Monitor Triggering | implemented | SetTriggering handler message_handler.rs:676, actor.rs:104/392/704; e2e tests triggering.rs:43,160 |
| 3147 | Attribute Write Index | implemented | Variant::set_range_of variant/mod.rs:1641 via Variable::set_value_range variable.rs:746; test write.rs:688,1008 |
| 3175 | Session Base | implemented | CreateSession/ActivateSession/CloseSession session/manager.rs; SecurityMode::None optional cert/nonce manager.rs:283-300; test :47,90 |
| 3184 | Base Info Core Structure 2 | implemented | Root/Objects/Server + ServerArray/NamespaceArray/ServiceLevel node_manager/memory/core.rs:986-1063; tests browse.rs:35, read.rs:42-43 |
| 3185 | Base Info Core Types Folders | implemented | Types/ObjectTypes/DataTypes/VariableTypes/ReferenceTypes folders exposed via default CoreNamespace import (core.rs:147). |
| 3186 | Base Info Core Views Folder | implemented | ViewsFolder entry point address_space/mod.rs:774-779; test at same location |
| 3188 | Base Info Base Types | implemented | Base built-in types present in schemas/1.05; imported via core.rs:147, exercised by address_space/mod.rs test suite. |
| 3189 | Base Info ServerType | implemented | ServerType is the root of the default AddressSpace; exercised across suite e.g. tests/integration/browse.rs. |
| 3192 | Base Info Diagnostics | implemented | EnabledFlag/ServerDiagnosticsSummary/SubscriptionDiagnosticsArray diagnostics/server.rs, core.rs:501-509; e2e read.rs:1604-1841 |
| 3196 | Base Info Fixed SamplingInterval | implemented | CU is conditional on the Server using a fixed set of sampling intervals (OPC-10000-5 SS7.9/SS12.8); this server negotiates a continuously-variable client-requested interval per monitored item (sanitize_sampling_interval, subscriptions/monitored_item.rs:299-311), so the precondition never holds and non-exposure of SamplingIntervalDiagnosticsArray is spec-conformant, not a gap -- documented in docs/server-capacity-limits.md |
| 3198 | Base Info Estimated Return Time | implemented | ServerStatusWrapper::schedule_shutdown/estimated_return_time (server_status.rs) + ServerHandle::shutdown_after_with_return_time (server_handle.rs) extend the existing shutdown mechanism; wired core.rs get_attribute; test base_info.rs::estimated_return_time_reflects_scheduled_shutdown_and_is_null_otherwise |
| 3201 | Base Info Custom Type System | partial | custom-codegen sample (samples/custom-codegen) demonstrates a full custom-type inheritance tree + generated Encoding Objects via async-opcua-codegen (types/encoding_ids.rs, types/gen.rs); no completeness e2e test proving all custom EventTypes are exposed alongside their encoding objects. Distinct from CU 5801 (which covers standard-nodeset type completeness, closed as a byproduct of the many typed-instantiation CUs) -- this one is specifically about CUSTOM (non-standard) types and remains open |
| 3207 | Base Info OptionSet DataType | implemented | OptionSet DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3214 | Base Info Range DataType | implemented | Range in nodeset + generated types/range.rs; used as EURange in datachange_overflow.rs, alarms.rs. |
| 3530 | View Basic 2 | implemented | Browse/BrowseNext w/ continuation points view.rs:213; tests browse.rs:252, :757 (Bad_ContinuationPointInvalid) |
| 3532 | Monitor Queueing | implemented | queue_size clamp monitored_item.rs:314-336, overflow:1067-1110; test datachange_overflow.rs:33-141 (size=2 discardOldest) |
| 3534 | Subscription Multiple | implemented | tests/integration/subscriptions.rs:476-509 creates >=2 subscriptions in one session, asserts BadTooManySubscriptions on next |
| 3535 | Subscription Retransmission Queue | implemented | RetransmissionQueue (retransmission_queue.rs, sized session_subscriptions.rs:1100) + Republish; test subscriptions.rs:1229 |
| 3536 | Security User Name Password 2 | implemented | Username/Password encrypted per policy (negotiate.rs:94-207 decrypt_identity_token_secret); tests negotiate.rs:259-330. |
| 3544 | Base Info ResendData Method | partial | ResendData method core.rs:1209-1220, wired subscription.rs:341-342,757; no test found (searched methods.rs, subscriptions.rs) |
| 3545 | Base Info Namespace Metadata | implemented | Dynamic per-namespace NamespaceMetaData objects diagnostics/node_manager.rs:583-650; e2e test browse.rs:942-967 |
| 3547 | Base Info UaBinary File | implemented | UABinaryFileDataType + Description types present in schemas/1.05; type-level exposure via CoreNamespace import. |
| 3550 | Base Info StatusResult DataType | implemented | StatusResult in nodeset + generated types/status_result.rs; exposed via CoreNamespace import. |
| 3551 | Base Info UriString | implemented | UriString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3554 | Address Space Base | implemented | Core AddressSpace all NodeClasses address_space/mod.rs (1454 LOC, unit tests) + opcua-nodes crate; e2e browse.rs:144-167 |
| 3560 | Address Space Interfaces | implemented | base_info::add_ordered_object attaches HasInterface from each OrderedListType child to IOrderedObjectType (a byproduct of closing CU 2512); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 3641 | Base Info Method Argument DataType | implemented | DataTypeId::Argument used building Method args async-opcua-nodes/src/method.rs:92; asserted in address_space/mod.rs:1320. |
| 3644 | Base Info SemanticVersionString | implemented | SemanticVersionString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3645 | Security User Token Unencrypted | implemented | SecurityPolicy::None UserTokenPolicy supported (authenticator.rs:397,415); tested authenticator.rs:492-518. |
| 3721 | Security ECC Policy | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3727 | Subscription Basic | implemented | CreateSubscription/Publish/Republish/SetPublishingMode etc implemented (subscriptions/session_subscriptions.rs); tested subscriptions.rs. |
| 3747 | Base Info IsExecutableOn | implemented | IsExecutableOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3748 | Base Info IsExecutingOn | implemented | IsExecutingOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3749 | Base Info Controls | implemented | Controls present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3750 | Base Info Utilizes | implemented | Utilizes present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3751 | Base Info Requires | implemented | Requires present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3752 | Base Info IsPhysicallyConnectedTo | implemented | IsPhysicallyConnectedTo present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3753 | Base Info RepresentsSameEntityAs | implemented | RepresentsSameEntityAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3754 | Base Info RepresentsSameHardwareAs | implemented | RepresentsSameHardwareAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3755 | Base Info RepresentsSameFunctionalityAs | implemented | RepresentsSameFunctionalityAs present schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3756 | Base Info IsHostedBy | implemented | IsHostedBy present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3757 | Base Info HasPhysicalComponent | implemented | HasPhysicalComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3758 | Base Info HasContainedComponent | implemented | HasContainedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3759 | Base Info HasAttachedComponent | implemented | HasAttachedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3802 | Time Sync - Configure Clock Skew | implemented | ServerConfig::max_acceptable_clock_skew_ns config/server.rs:669,998-1002; tests config/server.rs:375-432 |
| 3808 | Documentation - Core Capacities | implemented | docs/server-capacity-limits.md enumerates every Limits/SubscriptionLimits/OperationalLimits field with its default and configuration method, cross-checked against config/limits.rs's Default impls and the server_conf_limits_match_struct_field_names test |
| 3911 | Base Info Server Capabilities Subscriptions | implemented | core.rs get_attribute now wires MaxMonitoredItemsPerSubscription/MaxSubscriptionsPerSession to their SubscriptionLimits config fields, and MaxSubscriptions/MaxMonitoredItems (no server-wide cap exists) report spec-valid 0 per OPC-10000-5 SS6.3.2; tests read.rs::server_capabilities_max_monitored_items_per_subscription_and_max_subscriptions_per_session, ::server_capabilities_server_wide_max_subscriptions_and_max_monitored_items_are_zero |
| 3912 | Base Info Server Capabilities 2 | implemented | core.rs get_attribute wires MaxSessions to Limits.max_sessions (was the only unwired node in this CU per prior audit); test read.rs::server_capabilities_max_sessions_reports_configured_limit |
| 3913 | Subscription Publish Basic | implemented | max_publish_requests_per_subscription=4 (server/src/lib.rs:227); Publish exercised across tests/integration/subscriptions.rs. |
| 3922 | Base Info SemanticChange Bit | implemented | SemanticsChanged bit set monitored_item.rs:1012-1042 via EU-range writes session_subscriptions.rs:1238,1290; tests :1668 |
| 3923 | Session Multiple | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3983 | Base Services Diagnostics | implemented | result.rs:17-58 filter_diagnostic_info masks diag bits; wired attribute.rs/node_management.rs; test per_op_diagnostics.rs |
| 3985 | Session General Service Behaviour | implemented | controller.rs:396 auth-token check, response.rs:207 requestHandle echo, deadline_queue:971-1016 BadTimeout; e2e read.rs:1400-1408 |
| 3996 | Base Info ReferenceDescription | implemented | base_info::attach_reference_description instantiates ReferenceDescriptionVariableType via HasReferenceDescription, documenting a real Reference's SourceNode/ReferenceType/IsForward/TargetNode (OPC-10000-23 SS5, not Part 3/5); test base_info.rs::reference_description_documents_a_real_reference |
| 4052 | Base Info TrimmedString | implemented | TrimmedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 4053 | Base Info Locations Object | implemented | Locations object (i=31915, nodeset_16.rs:918-943) confirmed reachable via Browse from ObjectsFolder; test browse.rs::locations_object_is_reachable_from_objects_folder |
| 4054 | Base Info Handle DataType | implemented | Handle DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 4055 | Base Info Server Capabilities MaxMonitoredItemsQueueSize | implemented | core.rs get_attribute wires MaxMonitoredItemsQueueSize to SubscriptionLimits.max_monitored_item_queue_size, the same limit already enforced at monitored_item.rs:314; test read.rs::server_capabilities_max_monitored_items_queue_size_reports_configured_limit |
| 4237 | Address Space NonVolatile and Constant | implemented | NonVolatile/Constant bits defined enums.rs:15-19, generic get/set variable.rs:826-838; test write.rs::access_level_ex_non_volatile_and_constant_round_trip |
| 4426 | Base Info Decimal DataType | implemented | Decimal in nodeset + generated types/decimal_data_type.rs; encoded generically as a Structure DataType. |
| 5207 | Monitor Items 2 | implemented | No per-subscription item cap below 2 found (server/src/config/limits.rs); 2+ Double items trivially exercised in subscriptions.rs. |
| 5208 | Monitor Value Change V2 | partial | IndexRange applied to sample monitored_item.rs:931-940 (Variant::range_of); logic tested via read.rs:794-827, no MonitoredItem-level test |
| 5240 | Base Info Currency | implemented | base_info::create_currency_variable attaches a CurrencyUnit property (CurrencyUnitType) to a monetary DataVariable; test base_info.rs::currency_unit_property_reports_iso4217_fields |
| 5505 | Time Sync - UA based support | implemented | UaHeaderTimeSyncSource polls ResponseHeader.timestamp (time_sync_ua.rs:52-80), configurable builder.rs:258-262; test time_sync.rs:33 |
| 5592 | Missing from normalized CU list | source-issue | Referenced by closure but absent from conformance_units. |
| 5793 | Time Sync - Support | implemented | OsClockSource (time_sync.rs:112-124) + UA-based source satisfy facet; docs/time-synchronization.md:9-17; tests time_sync.rs:11-22 |
| 5801 | Base Info Type Information | implemented | Not a standalone feature -- this server always imports the complete standard 1.05 nodeset (every ObjectType/VariableType/ReferenceType/DataType, their supertypes, and Encoding Objects for Structured DataTypes are generated nodeset nodes), so any instance referencing a standard TypeDefinition automatically satisfies this CU. Demonstrated cumulatively across every 'instantiate VariableType X' CU this project closes (e.g. feature 097's base_info.rs OrderedListType/SelectionListType/OptionSetType/ReferenceDescriptionVariableType, feature 100's data_access.rs TwoStateDiscreteType/MultiStateDiscreteType/MultiStateValueDiscreteType/ArrayItemType family) -- each instantiation test proves its referenced TypeDefinition node resolves in the AddressSpace, not just an isolated e2e check |
| 5814 | Security - No Application Authentication | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 5868 | Base Info Portable IDs | implemented | PortableQualifiedName/PortableNodeId present schemas/1.05 + generated types/portable_node_id.rs; exposed via CoreNamespace import. |

### Standard 2025 UA Server Profile

- OPC profile id: `2269`
- URI: `http://opcfoundation.org/UA-Profile/Server/StandardUA2022`
- CU closure size: 123

| CU | Name | Status | Evidence |
|---:|---|---|---|
| 2190 | Session Cancel | implemented | Cancel now reaches into SessionSubscriptions::publish_request_queue (session_subscriptions.rs::cancel_publish_requests) and resolves matching queued Publish requests with Bad_RequestCancelledByClient, routed via SubscriptionCommand::CancelPublishRequests through the per-session actor (message_handler.rs Cancel arm); test core_tests.rs::cancel_aborts_a_queued_publish_request |
| 2231 | Push Model for Global Certificate and TrustList Management | implemented | Feature 101 (Run 1): fixed fabricated-NodeId/wrong-node-manager bugs in push_methods.rs; real ServerConfigurationType handlers wired to verified NodeIds: CreateSigningRequest (ns=0;i=12737, real PKCS#10 CSR), UpdateCertificate (13737), ApplyChanges (12740), CancelChanges (25708), GetRejectedList (12777), ResetToServerDefaults (25709); e2e-proven via gds_push_integration.rs + unit tests in push_methods.rs. CreateSelfSignedCertificate/DeleteCertificate/GetCertificates confirmed absent from the generated nodeset (SourceIssue). Feature 102 (Run 2): closes the remaining TrustList/CertificateGroup surface for DefaultApplicationGroup -- new gds/trust_list.rs wires Open (12647), OpenWithMasks (12663), Read (12652), Write (12655), GetPosition (12657), SetPosition (12660), Close (12650), CloseAndUpdate (12666), AddCertificate (12668), RemoveCertificate (12670), all empirically verified live against DefaultApplicationGroup.TrustList (ns=0;i=12642); PushTransaction extended to share ApplyChanges/CancelChanges with the TrustList's pending change; CertificateStore gained write-side trusted/issuer cert+CRL helpers (store_trusted_cert/store_issuer_cert/remove_trusted_cert/remove_issuer_cert/store_trusted_crl/store_issuer_crl/replace_*); 30 unit tests across push_methods.rs+trust_list.rs plus an extended gds_push_integration.rs wire-dispatch proof. DefaultHttpsGroup/DefaultUserTokenGroup CertificateGroups exist in the generated nodeset but are out of scope for this feature (documented follow-up); CertificateGroupType.GetRejectedList remains absent (SourceIssue, satisfied via ServerConfiguration.GetRejectedList). Sibling bug in pull_methods.rs (CU 3582, same fabricated-NodeId pattern) fixed by Feature 103 (Run 1). |
| 2271 | Discovery Register | implemented | Client::register_server (async-opcua-client/src/session/client.rs:818) + server-side periodic_discovery_server_registration (discovery.rs:86-117) calling it over a client-selected highest-security endpoint; test discovery.rs uses secured_endpoint() (SignAndEncrypt) throughout, e.g. discovery.rs:114 |
| 2317 | View TranslateBrowsePath | implemented | TranslateBrowsePathsToNodeIds handler async-opcua-server/src/session/services/view.rs:388; test async-opcua/tests/integration/tier_a.rs:141 |
| 2328 | Discovery Get Endpoints | implemented | get_endpoints_with_filters incl profile-uri filter info.rs:342-378; tests core_tests.rs:100,358,366 |
| 2352 | Discovery Find Servers Self | implemented | FindServers handled async-opcua-server/src/session/controller.rs:716; tests async-opcua/tests/integration/discovery.rs:83,119 |
| 2371 | Protocol UA TCP | implemented | Hello/Ack+TCP codec async-opcua-core/src/comms/tcp_types.rs:244,373; exercised by full opc.tcp integration suite |
| 2389 | Attribute Write Values | implemented | Write handler async-opcua-server/src/session/message_handler.rs:820-852; tests async-opcua/tests/integration/write.rs |
| 2400 | Session Change User | implemented | ActivateSession identity-change + revalidate_monitored_items_for_user manager.rs:1565,1591-1598; test manager.rs:2234-2253 |
| 2407 | Security Administration | implemented | builder.rs: add_user_token:567, SecurityPolicy::None/Sign/SignAndEncrypt:140-195, trust_client_certs:397-398, pki_dir:494; tested security_tests.rs. |
| 2423 | Base Info Rational Number | implemented | RationalNumberType present schemas/1.05/Opc.Ua.NodeSet2.xml, generated types/rational_number.rs; exposed via CoreNamespace import. |
| 2446 | Address Space AddIn Reference | implemented | HasAddIn ReferenceType via generated core nodeset nodeset_19.rs:822, loaded by default address_space/mod.rs:11 |
| 2447 | Address Space AddIn DefaultInstanceBrowsename | implemented | DefaultInstanceBrowseName Property via generated nodeset_21.rs:2832, loaded by default node_manager/memory/core.rs:172 |
| 2476 | Base Info LocalTime | partial | Real computed LocalTime (chrono->TimeZoneDataType) node_manager/memory/core.rs:989-997; no test reads Server_LocalTime attribute |
| 2478 | Time Sync - OS based support | implemented | OsClockSource default TimeSyncSource impl async-opcua-server/src/time_sync.rs:112-124; unit test time_sync.rs:130-137 |
| 2479 | Time Sync - IEEE 1588 (PTP) | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2480 | Time Sync - IEEE 802.1AS | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2481 | Base Info NormalizedString DataType | implemented | NormalizedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import (core.rs:147). |
| 2482 | Base Info DecimalString DataType | implemented | DecimalString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2483 | Base Info Date DataTypes | implemented | DurationString/TimeString/DateString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2484 | Base Info BitFieldMaskDataType | implemented | BitFieldMaskDataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2485 | Base Info KeyValuePair | implemented | KeyValuePair in nodeset + generated types/key_value_pair.rs; used by published_data_set_data_type.rs. |
| 2490 | Base Info Subvariables of Structures | implemented | HasStructuredComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2491 | Base Info AssociatedWith | implemented | AssociatedWith present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2500 | Base Info EUInformation | implemented | EUInformation used/tested: tests/integration/custom_types.rs, async-opcua-types/src/tests/json.rs:344. |
| 2512 | Base Info OrderedList | implemented | base_info::create_ordered_list_in_address_space/add_ordered_object instantiate OrderedListType with HasOrderedComponent children implementing IOrderedObjectType via HasInterface (NumberInList is the authoritative order signal, not Browse response order, per OPC-10000-5 SS6.11's own rationale); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 2513 | Base Info Audio Type | implemented | AudioVariableType/AudioDataType present schemas/1.05/Opc.Ua.NodeSet2.xml; type-level exposure via CoreNamespace import. |
| 2514 | Base Info Spatial Data | implemented | VectorType/CartesianCoordinatesType/OrientationType/FrameType present in schemas/1.05; exposed via CoreNamespace import. |
| 2516 | Base Info HasOrderedComponent | implemented | HasOrderedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2517 | Base Info Deprecated Information | implemented | IsDeprecated present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2518 | Base Info Image DataTypes | implemented | ImageBMP/GIF/JPG/PNG present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2536 | Base Info ContentFilter | implemented | ContentFilter/Element DataTypes+encodings (node_ids.rs:168,7132); real WhereClause use+tests in where_clause.rs:13-56, select.rs:14-79. |
| 2600 | SecurityPolicy Support | implemented | 10+ SecurityPolicy variants incl None async-opcua-crypto/src/security_policy.rs:125-150; extensively tested + CI conformance matrix |
| 2711 | Base Info Selection List | implemented | base_info::create_selection_list_variable instantiates SelectionListType with Selections/SelectionDescriptions/RestrictToList; test base_info.rs::selection_list_exposes_selections_descriptions_and_restrict_flag |
| 2786 | Time Sync - NTP | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2808 | Security Role Server Authorization | implemented | Opt-in RBAC enforcement async-opcua-server/src/rbac/decision.rs:46-81; dedicated suite async-opcua/tests/integration/rbac.rs |
| 2809 | Address Space Atomicity | implemented | AccessLevelExType NonatomicRead/Write async-opcua-nodes/src/variable.rs:62,827-837; unit test variable.rs:990-997 |
| 2820 | Address Space Full Array Only | implemented | validate_node_write_inner (address_space/write_validation.rs) rejects an IndexRange Write to AttributeId::Value with Bad_WriteNotSupported when AccessLevelExType::WriteFullArrayOnly is set; test write.rs::write_index_range_rejected_when_write_full_array_only |
| 2823 | Security Invalid user token | partial | Fixed 100ms tarpit on every auth failure (session/negotiate.rs:16,28-40; tested security_tests.rs:2429); no escalating lockout. |
| 2837 | UA Binary Encoding | implemented | BinaryEncodable/BinaryDecodable traits async-opcua-types/src/encoding.rs:445-482, pervasive derive use; tests encoding.rs:919 |
| 2853 | UA Secure Conversation | implemented | SecureChannel/OpenSecureChannel comms/secure_channel.rs:657; tests secure_channel.rs:136-663, integration secure_channel.rs:15 |
| 2863 | Security Policy Required | implemented | Modern policies default-on, legacy Basic128Rsa15/Basic256 opt-in behind legacy-crypto feature builder.rs:142-166; matrix test |
| 2928 | Monitored Items Deadband Filter | implemented | Absolute DataChangeFilter deadband subscriptions/monitored_item/filters.rs:128-137; unit test filters.rs:175 |
| 2936 | Attribute Write StatusCode & Timestamp | implemented | write_node_value (address_space/utils.rs) threads client status/source_timestamp/server_timestamp through to Variable::set_value_range (fixed a real bug: server_timestamp was hardcoded to now()); test write.rs::write_status_code_and_timestamps_round_trip |
| 2940 | Base Info GetMonitoredItems Method | implemented | GetMonitoredItems method node_manager/memory/core.rs:1195-1207; test methods.rs:291-332 call_get_monitored_items |
| 2963 | Monitor Basic | implemented | create/modify/delete_monitored_items + set_monitoring_mode (session/services/monitored_items.rs:170-573); tested subscriptions.rs. |
| 2969 | Base Info ValueAsText | implemented | base_info::create_enum_variable_with_value_as_text/update_enum_value attach a ValueAsText property kept in sync with an enumerated Variable's Value; test base_info.rs::value_as_text_tracks_enumerated_value_changes |
| 3072 | Attribute Read | implemented | Read applies IndexRange via NumericRange::range_of node_manager/memory/core.rs:1079-1080; tests read.rs:1425,794 |
| 3073 | View RegisterNodes | implemented | RegisterNodes/UnregisterNodes handler session/services/view.rs:540, memory_mgr_impl.rs:1608; e2e test browse.rs:675 |
| 3080 | Security Default ApplicationInstance Certificate | implemented | CertificateStore::create_and_store_application_instance_cert certificate_store.rs:265, default builder.rs:119; test crypto.rs:46 |
| 3125 | Security User X509 | implemented | X509 user cert validated incl. POP sig (info.rs:1291-1332); tests security_tests.rs:1565-1863 (untrusted/expired/revoked). |
| 3127 | Base Info OptionSet | implemented | base_info::create_option_set_variable instantiates OptionSetType with OptionSetValues/BitMask; test base_info.rs::option_set_exposes_per_bit_values_and_bitmask |
| 3143 | Subscription PublishRequest Queue Overflow | implemented | enqueue_publish_request pops oldest on overflow, returns BadTooManyPublishRequests (session_subscriptions.rs:767); test :1581. |
| 3146 | Monitor Triggering | implemented | SetTriggering handler message_handler.rs:676, actor.rs:104/392/704; e2e tests triggering.rs:43,160 |
| 3147 | Attribute Write Index | implemented | Variant::set_range_of variant/mod.rs:1641 via Variable::set_value_range variable.rs:746; test write.rs:688,1008 |
| 3170 | Discovery Register2 | implemented | Client::register_server2 (async-opcua-client/src/session/client.rs:879) + client-callable discovery-configuration support; tests discovery.rs::register_server2_mdns_config_result_matches_feature_support and :303 over secured_endpoint() (SignAndEncrypt) |
| 3175 | Session Base | implemented | CreateSession/ActivateSession/CloseSession session/manager.rs; SecurityMode::None optional cert/nonce manager.rs:283-300; test :47,90 |
| 3184 | Base Info Core Structure 2 | implemented | Root/Objects/Server + ServerArray/NamespaceArray/ServiceLevel node_manager/memory/core.rs:986-1063; tests browse.rs:35, read.rs:42-43 |
| 3185 | Base Info Core Types Folders | implemented | Types/ObjectTypes/DataTypes/VariableTypes/ReferenceTypes folders exposed via default CoreNamespace import (core.rs:147). |
| 3186 | Base Info Core Views Folder | implemented | ViewsFolder entry point address_space/mod.rs:774-779; test at same location |
| 3188 | Base Info Base Types | implemented | Base built-in types present in schemas/1.05; imported via core.rs:147, exercised by address_space/mod.rs test suite. |
| 3189 | Base Info ServerType | implemented | ServerType is the root of the default AddressSpace; exercised across suite e.g. tests/integration/browse.rs. |
| 3192 | Base Info Diagnostics | implemented | EnabledFlag/ServerDiagnosticsSummary/SubscriptionDiagnosticsArray diagnostics/server.rs, core.rs:501-509; e2e read.rs:1604-1841 |
| 3196 | Base Info Fixed SamplingInterval | implemented | CU is conditional on the Server using a fixed set of sampling intervals (OPC-10000-5 SS7.9/SS12.8); this server negotiates a continuously-variable client-requested interval per monitored item (sanitize_sampling_interval, subscriptions/monitored_item.rs:299-311), so the precondition never holds and non-exposure of SamplingIntervalDiagnosticsArray is spec-conformant, not a gap -- documented in docs/server-capacity-limits.md |
| 3198 | Base Info Estimated Return Time | implemented | ServerStatusWrapper::schedule_shutdown/estimated_return_time (server_status.rs) + ServerHandle::shutdown_after_with_return_time (server_handle.rs) extend the existing shutdown mechanism; wired core.rs get_attribute; test base_info.rs::estimated_return_time_reflects_scheduled_shutdown_and_is_null_otherwise |
| 3201 | Base Info Custom Type System | partial | custom-codegen sample (samples/custom-codegen) demonstrates a full custom-type inheritance tree + generated Encoding Objects via async-opcua-codegen (types/encoding_ids.rs, types/gen.rs); no completeness e2e test proving all custom EventTypes are exposed alongside their encoding objects. Distinct from CU 5801 (which covers standard-nodeset type completeness, closed as a byproduct of the many typed-instantiation CUs) -- this one is specifically about CUSTOM (non-standard) types and remains open |
| 3207 | Base Info OptionSet DataType | implemented | OptionSet DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3214 | Base Info Range DataType | implemented | Range in nodeset + generated types/range.rs; used as EURange in datachange_overflow.rs, alarms.rs. |
| 3530 | View Basic 2 | implemented | Browse/BrowseNext w/ continuation points view.rs:213; tests browse.rs:252, :757 (Bad_ContinuationPointInvalid) |
| 3532 | Monitor Queueing | implemented | queue_size clamp monitored_item.rs:314-336, overflow:1067-1110; test datachange_overflow.rs:33-141 (size=2 discardOldest) |
| 3534 | Subscription Multiple | implemented | tests/integration/subscriptions.rs:476-509 creates >=2 subscriptions in one session, asserts BadTooManySubscriptions on next |
| 3535 | Subscription Retransmission Queue | implemented | RetransmissionQueue (retransmission_queue.rs, sized session_subscriptions.rs:1100) + Republish; test subscriptions.rs:1229 |
| 3536 | Security User Name Password 2 | implemented | Username/Password encrypted per policy (negotiate.rs:94-207 decrypt_identity_token_secret); tests negotiate.rs:259-330. |
| 3544 | Base Info ResendData Method | partial | ResendData method core.rs:1209-1220, wired subscription.rs:341-342,757; no test found (searched methods.rs, subscriptions.rs) |
| 3545 | Base Info Namespace Metadata | implemented | Dynamic per-namespace NamespaceMetaData objects diagnostics/node_manager.rs:583-650; e2e test browse.rs:942-967 |
| 3547 | Base Info UaBinary File | implemented | UABinaryFileDataType + Description types present in schemas/1.05; type-level exposure via CoreNamespace import. |
| 3550 | Base Info StatusResult DataType | implemented | StatusResult in nodeset + generated types/status_result.rs; exposed via CoreNamespace import. |
| 3551 | Base Info UriString | implemented | UriString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3554 | Address Space Base | implemented | Core AddressSpace all NodeClasses address_space/mod.rs (1454 LOC, unit tests) + opcua-nodes crate; e2e browse.rs:144-167 |
| 3560 | Address Space Interfaces | implemented | base_info::add_ordered_object attaches HasInterface from each OrderedListType child to IOrderedObjectType (a byproduct of closing CU 2512); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 3641 | Base Info Method Argument DataType | implemented | DataTypeId::Argument used building Method args async-opcua-nodes/src/method.rs:92; asserted in address_space/mod.rs:1320. |
| 3644 | Base Info SemanticVersionString | implemented | SemanticVersionString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3645 | Security User Token Unencrypted | implemented | SecurityPolicy::None UserTokenPolicy supported (authenticator.rs:397,415); tested authenticator.rs:492-518. |
| 3721 | Security ECC Policy | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3727 | Subscription Basic | implemented | CreateSubscription/Publish/Republish/SetPublishingMode etc implemented (subscriptions/session_subscriptions.rs); tested subscriptions.rs. |
| 3747 | Base Info IsExecutableOn | implemented | IsExecutableOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3748 | Base Info IsExecutingOn | implemented | IsExecutingOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3749 | Base Info Controls | implemented | Controls present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3750 | Base Info Utilizes | implemented | Utilizes present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3751 | Base Info Requires | implemented | Requires present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3752 | Base Info IsPhysicallyConnectedTo | implemented | IsPhysicallyConnectedTo present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3753 | Base Info RepresentsSameEntityAs | implemented | RepresentsSameEntityAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3754 | Base Info RepresentsSameHardwareAs | implemented | RepresentsSameHardwareAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3755 | Base Info RepresentsSameFunctionalityAs | implemented | RepresentsSameFunctionalityAs present schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3756 | Base Info IsHostedBy | implemented | IsHostedBy present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3757 | Base Info HasPhysicalComponent | implemented | HasPhysicalComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3758 | Base Info HasContainedComponent | implemented | HasContainedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3759 | Base Info HasAttachedComponent | implemented | HasAttachedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3802 | Time Sync - Configure Clock Skew | implemented | ServerConfig::max_acceptable_clock_skew_ns config/server.rs:669,998-1002; tests config/server.rs:375-432 |
| 3808 | Documentation - Core Capacities | implemented | docs/server-capacity-limits.md enumerates every Limits/SubscriptionLimits/OperationalLimits field with its default and configuration method, cross-checked against config/limits.rs's Default impls and the server_conf_limits_match_struct_field_names test |
| 3911 | Base Info Server Capabilities Subscriptions | implemented | core.rs get_attribute now wires MaxMonitoredItemsPerSubscription/MaxSubscriptionsPerSession to their SubscriptionLimits config fields, and MaxSubscriptions/MaxMonitoredItems (no server-wide cap exists) report spec-valid 0 per OPC-10000-5 SS6.3.2; tests read.rs::server_capabilities_max_monitored_items_per_subscription_and_max_subscriptions_per_session, ::server_capabilities_server_wide_max_subscriptions_and_max_monitored_items_are_zero |
| 3912 | Base Info Server Capabilities 2 | implemented | core.rs get_attribute wires MaxSessions to Limits.max_sessions (was the only unwired node in this CU per prior audit); test read.rs::server_capabilities_max_sessions_reports_configured_limit |
| 3913 | Subscription Publish Basic | implemented | max_publish_requests_per_subscription=4 (server/src/lib.rs:227); Publish exercised across tests/integration/subscriptions.rs. |
| 3922 | Base Info SemanticChange Bit | implemented | SemanticsChanged bit set monitored_item.rs:1012-1042 via EU-range writes session_subscriptions.rs:1238,1290; tests :1668 |
| 3923 | Session Multiple | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3983 | Base Services Diagnostics | implemented | result.rs:17-58 filter_diagnostic_info masks diag bits; wired attribute.rs/node_management.rs; test per_op_diagnostics.rs |
| 3985 | Session General Service Behaviour | implemented | controller.rs:396 auth-token check, response.rs:207 requestHandle echo, deadline_queue:971-1016 BadTimeout; e2e read.rs:1400-1408 |
| 3996 | Base Info ReferenceDescription | implemented | base_info::attach_reference_description instantiates ReferenceDescriptionVariableType via HasReferenceDescription, documenting a real Reference's SourceNode/ReferenceType/IsForward/TargetNode (OPC-10000-23 SS5, not Part 3/5); test base_info.rs::reference_description_documents_a_real_reference |
| 4052 | Base Info TrimmedString | implemented | TrimmedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 4053 | Base Info Locations Object | implemented | Locations object (i=31915, nodeset_16.rs:918-943) confirmed reachable via Browse from ObjectsFolder; test browse.rs::locations_object_is_reachable_from_objects_folder |
| 4054 | Base Info Handle DataType | implemented | Handle DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 4055 | Base Info Server Capabilities MaxMonitoredItemsQueueSize | implemented | core.rs get_attribute wires MaxMonitoredItemsQueueSize to SubscriptionLimits.max_monitored_item_queue_size, the same limit already enforced at monitored_item.rs:314; test read.rs::server_capabilities_max_monitored_items_queue_size_reports_configured_limit |
| 4237 | Address Space NonVolatile and Constant | implemented | NonVolatile/Constant bits defined enums.rs:15-19, generic get/set variable.rs:826-838; test write.rs::access_level_ex_non_volatile_and_constant_round_trip |
| 4426 | Base Info Decimal DataType | implemented | Decimal in nodeset + generated types/decimal_data_type.rs; encoded generically as a Structure DataType. |
| 5207 | Monitor Items 2 | implemented | No per-subscription item cap below 2 found (server/src/config/limits.rs); 2+ Double items trivially exercised in subscriptions.rs. |
| 5208 | Monitor Value Change V2 | partial | IndexRange applied to sample monitored_item.rs:931-940 (Variant::range_of); logic tested via read.rs:794-827, no MonitoredItem-level test |
| 5240 | Base Info Currency | implemented | base_info::create_currency_variable attaches a CurrencyUnit property (CurrencyUnitType) to a monetary DataVariable; test base_info.rs::currency_unit_property_reports_iso4217_fields |
| 5505 | Time Sync - UA based support | implemented | UaHeaderTimeSyncSource polls ResponseHeader.timestamp (time_sync_ua.rs:52-80), configurable builder.rs:258-262; test time_sync.rs:33 |
| 5592 | Missing from normalized CU list | source-issue | Referenced by closure but absent from conformance_units. |
| 5793 | Time Sync - Support | implemented | OsClockSource (time_sync.rs:112-124) + UA-based source satisfy facet; docs/time-synchronization.md:9-17; tests time_sync.rs:11-22 |
| 5801 | Base Info Type Information | implemented | Not a standalone feature -- this server always imports the complete standard 1.05 nodeset (every ObjectType/VariableType/ReferenceType/DataType, their supertypes, and Encoding Objects for Structured DataTypes are generated nodeset nodes), so any instance referencing a standard TypeDefinition automatically satisfies this CU. Demonstrated cumulatively across every 'instantiate VariableType X' CU this project closes (e.g. feature 097's base_info.rs OrderedListType/SelectionListType/OptionSetType/ReferenceDescriptionVariableType, feature 100's data_access.rs TwoStateDiscreteType/MultiStateDiscreteType/MultiStateValueDiscreteType/ArrayItemType family) -- each instantiation test proves its referenced TypeDefinition node resolves in the AddressSpace, not just an isolated e2e check |
| 5814 | Security - No Application Authentication | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 5868 | Base Info Portable IDs | implemented | PortableQualifiedName/PortableNodeId present schemas/1.05 + generated types/portable_node_id.rs; exposed via CoreNamespace import. |

## Additional Server Facets (Summary)

One row per facet not already covered by the four canonical profiles above. Counts are per-status within that facet's own CU closure; a CU counted here may also appear in another facet or in the Full CU Ledger below.

| Facet | OPC id | Closure | Implemented | Partial | Gap | Needs-proof | Extensible | Source-issue |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| A & C Acknowledgeable Alarm 2022 Server Facet | 1565 | 34 | 20 | 3 | 11 | 0 | 0 | 0 |
| A & C Address Space Instance 2022 Server Facet | 1562 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| A & C Alarm 2022 Server Facet | 1502 | 84 | 43 | 4 | 37 | 0 | 0 | 0 |
| A & C Alarm Auditing Server Facet | 1503 | 8 | 3 | 0 | 5 | 0 | 0 | 0 |
| A & C AlarmMetrics Server Facet | 887 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| A & C Base Condition 2022 Server Facet | 1551 | 26 | 16 | 3 | 7 | 0 | 0 | 0 |
| A & C CertificateExpiration 2022 Server Facet | 1566 | 32 | 22 | 3 | 7 | 0 | 0 | 0 |
| A & C Dialog 2022 Server Facet | 1504 | 32 | 17 | 4 | 11 | 0 | 0 | 0 |
| A & C Enable 2022 Server Facet | 1563 | 31 | 19 | 3 | 9 | 0 | 0 | 0 |
| A & C Exclusive Alarming 2022 Server Facet | 1500 | 100 | 51 | 4 | 45 | 0 | 0 | 0 |
| A & C Non-Exclusive Alarming 2022 Server Facet | 1501 | 103 | 54 | 4 | 45 | 0 | 0 | 0 |
| A & C Previous Instances 2022 Server Facet | 1564 | 27 | 17 | 3 | 7 | 0 | 0 | 0 |
| A & C Refresh2 2022 Server Facet | 1568 | 27 | 17 | 3 | 7 | 0 | 0 | 0 |
| A & E Wrapper 2022 Facet | 1346 | 18 | 13 | 2 | 3 | 0 | 0 | 0 |
| Address Space Notifier Server Facet  | 744 | 2 | 0 | 1 | 1 | 0 | 0 | 0 |
| Aggregate Subscription 2022 Server Facet | 1582 | 57 | 52 | 2 | 3 | 0 | 0 | 0 |
| Attribute WriteMask Server 2023 Facet  | 1996 | 8 | 6 | 0 | 2 | 0 | 0 | 0 |
| Attribute WriteMask Server Facet | 1997 | 7 | 5 | 0 | 2 | 0 | 0 | 0 |
| Auditing 2022 Server Facet | 1328 | 30 | 17 | 6 | 7 | 0 | 0 | 0 |
| Authorization Service Server Facet | 1629 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| Base Historical Event 2022 Server Facet | 1577 | 3 | 3 | 0 | 0 | 0 | 0 | 0 |
| Base Server Behaviour Facet | 1715 | 4 | 3 | 0 | 1 | 0 | 0 | 0 |
| Client Redundancy Server Facet | 661 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| ComplexType 2017 Server Facet | 1725 | 6 | 3 | 3 | 0 | 0 | 0 | 0 |
| Data Access Server Facet | 1505 | 22 | 14 | 0 | 8 | 0 | 0 | 0 |
| Dictionary Reference Server Facet | 1524 | 3 | 0 | 0 | 3 | 0 | 0 | 0 |
| Documentation Server Facet | 768 | 6 | 4 | 0 | 2 | 0 | 0 | 0 |
| Durable Subscription 2022 Server Facet | 2098 | 3 | 1 | 0 | 2 | 0 | 0 | 0 |
| Embedded DataChange Subscription 2022 Server Facet | 2250 | 10 | 9 | 1 | 0 | 0 | 0 | 0 |
| Exposes Type System Server Facet | 1219 | 46 | 46 | 0 | 0 | 0 | 0 | 0 |
| File Access Server Facet | 1348 | 3 | 0 | 2 | 1 | 0 | 0 | 0 |
| Global Certificate Management Server Facet | 1631 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| Global Discovery Server 2022 Profile | 1343 | 69 | 59 | 3 | 3 | 0 | 3 | 1 |
| Global Discovery and Certificate Mgmt 2022 Server | 1344 | 94 | 67 | 12 | 11 | 0 | 3 | 1 |
| Global Service Authorization Request Server Facet | 1026 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| Global Service KeyCredential Pull Facet | 1027 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| Historical Access Modified Data 2022 Server Facet | 1709 | 4 | 4 | 0 | 0 | 0 | 0 | 0 |
| Historical Access Structured Data 2022 Server Facet | 1710 | 10 | 3 | 0 | 7 | 0 | 0 | 0 |
| Historical Aggregate 2022 Server Facet | 1708 | 44 | 40 | 0 | 4 | 0 | 0 | 0 |
| Historical Annotation 2022 Server Facet | 1572 | 6 | 6 | 0 | 0 | 0 | 0 | 0 |
| Historical Data AtTime 2022 Server Facet | 1707 | 4 | 3 | 0 | 1 | 0 | 0 | 0 |
| Historical Data Delete 2022 Server Facet | 1576 | 3 | 3 | 0 | 0 | 0 | 0 | 0 |
| Historical Data Insert 2022 Server Facet | 1574 | 4 | 3 | 1 | 0 | 0 | 0 | 0 |
| Historical Data Replace 2022 Server Facet | 1575 | 4 | 3 | 1 | 0 | 0 | 0 | 0 |
| Historical Data Update 2022 Server Facet | 1573 | 4 | 3 | 1 | 0 | 0 | 0 | 0 |
| Historical Event Delete 2022 Server Facet | 1581 | 3 | 3 | 0 | 0 | 0 | 0 | 0 |
| Historical Event Insert 2022 Server Facet | 1579 | 3 | 3 | 0 | 0 | 0 | 0 | 0 |
| Historical Event Replace 2022 Server Facet | 1580 | 3 | 3 | 0 | 0 | 0 | 0 | 0 |
| Historical Event Update 2022 Server Facet | 1578 | 3 | 2 | 1 | 0 | 0 | 0 | 0 |
| Historical Raw Data 2022 Server Facet | 1571 | 5 | 4 | 1 | 0 | 0 | 0 | 0 |
| KeyCredential Service Server Facet | 2113 | 5 | 0 | 0 | 5 | 0 | 0 | 0 |
| Method 2022 Server Facet | 1639 | 6 | 3 | 2 | 1 | 0 | 0 | 0 |
| Model Change Event Server Facet | 1733 | 3 | 1 | 0 | 2 | 0 | 0 | 0 |
| Node Management 2022 Server Facet | 1329 | 54 | 52 | 0 | 2 | 0 | 0 | 0 |
| Redundancy Transparent Server Facet | 2249 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| Redundancy Visible Server Facet | 2252 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| Request State Change Server Facet | 1633 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| Reverse Connect Server Facet | 1632 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| Scheduler Base Server Facet | 1875 | 8 | 3 | 2 | 3 | 0 | 0 | 0 |
| Scheduler Configuration Server Facet | 1876 | 10 | 3 | 2 | 5 | 0 | 0 | 0 |
| Sessionless Server Facet | 1630 | 2 | 0 | 0 | 2 | 0 | 0 | 0 |
| Standard DataChange Subscription 2022 Server Facet | 1324 | 17 | 15 | 2 | 0 | 0 | 0 | 0 |
| Standard Event Subscription 2022 Server Facet | 2085 | 22 | 14 | 3 | 5 | 0 | 0 | 0 |
| State Machine 2022 Server Facet | 1638 | 30 | 14 | 5 | 11 | 0 | 0 | 0 |
| Subnet Discovery Server Facet | 2069 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| Temporary File Access Server Facet | 1525 | 5 | 0 | 0 | 5 | 0 | 0 | 0 |
| User Role Base 2022 Server Facet | 1351 | 3 | 2 | 1 | 0 | 0 | 0 | 0 |
| User Role Management 2022 Server Facet | 2080 | 14 | 6 | 4 | 4 | 0 | 0 | 0 |
| User Token - Anonymous Server Facet | 1691 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| User Token - JWT Server Facet | 1697 | 7 | 2 | 1 | 4 | 0 | 0 | 0 |
| User Token - User Name Password Server Facet | 1695 | 3 | 2 | 1 | 0 | 0 | 0 | 0 |
| User Token - X509 Certificate Server Facet | 1696 | 2 | 1 | 1 | 0 | 0 | 0 | 0 |

## Full CU Ledger

492 distinct CUs referenced by any server profile or facet in this snapshot.

| CU | Name | Status | Evidence |
|---:|---|---|---|
| 2163 | Address Space UserWriteMask | gap | UserWriteMask is a static field (nodes/base.rs:73,117), never per-user computed unlike UserAccessLevel (utils.rs:414); no test. |
| 2165 | Aggregate Subscription - AnnotationCount | implemented | agg_annotation_count engine.rs:1084 (id 2351); test aggregates_tests.rs:758,771-776 |
| 2166 | Aggregate Subscription - Maximum2 | implemented | agg_maximum2 engine.rs:905 (id 11287); test aggregates_tests.rs:580-598 (Maximum2=20) |
| 2175 | Aggregate - MinimumActualTime2 | implemented | engine.rs dispatch_aggregate AGG_MINIMUM_ACTUAL_TIME2=11305 (aggregates/engine.rs:1487-1489); test aggregates_tests.rs:605 phase_d_min_actual_time2_uses_bound_timestamp |
| 2178 | Aggregate Subscription - VariancePopulation | implemented | agg_variance_population engine.rs:1045 (11429); test phase_b_variance_and_stddev aggregates_tests.rs:369-393 |
| 2180 | A & C Dialog | implemented | Respond via dialog.rs:190-198 + methods.rs:301-317; tested alarms.rs:1725 dialog_condition_respond_ends_dialog_and_validates |
| 2184 | Aggregate Subscription - Total2 | implemented | agg_total2 engine.rs:733 (11304); test phase_d_time_average2_total2 aggregates_tests.rs:621 |
| 2185 | Historical Access Structured Data Insert | gap | data_history.rs:305-308 update_structure_data rejects non-Annotation ExtensionObjects (BadTypeMismatch); same in sqlite backend.rs:1030; test history_data_inmemory.rs:421 proves only Annotation type accepted, contra CU's exclusion of annotation-only support |
| 2188 | Aggregate - Maximum2 | implemented | engine.rs dispatch AGG_MAXIMUM2=11287 (engine.rs:1483); aggregates_tests.rs references 11287 (id present, phase_d family covers Minimum2/Maximum2 pattern) |
| 2189 | A & C ConditionClasses | gap | AlarmEvent (opcua-core/src/events.rs:9-35) has no condition-class field; BaseEventType.condition_class_id never set by alarms code (0 hits) |
| 2190 | Session Cancel | implemented | Cancel now reaches into SessionSubscriptions::publish_request_queue (session_subscriptions.rs::cancel_publish_requests) and resolves matching queued Publish requests with Bad_RequestCancelledByClient, routed via SubscriptionCommand::CancelPublishRequests through the per-session actor (message_handler.rs Cancel arm); test core_tests.rs::cancel_aborts_a_queued_publish_request |
| 2194 | Aggregate Subscription - DeltaBounds | implemented | agg_delta_bounds engine.rs:1329 (11507); test phase_c_start_end_delta_bounds aggregates_tests.rs:486-503 |
| 2201 | Aggregate Subscription - WorstQuality | implemented | agg_worst_quality engine.rs:1247 (2364); test aggregates_tests.rs:401, worst_quality_is_value_type_independent:1141 |
| 2202 | A & C Enable | implemented | ConditionType_Enable/Disable Methods registered (methods.rs register_condition_methods); handle_condition_enable/disable call set_enabled; test alarms.rs::enable_disable_methods_toggle_enabled_state |
| 2203 | Attribute Write Complex | partial | write_node_value accepts any Variant (address_space/utils.rs:473) but no test writes a structured/ExtensionObject value; only Read tested. |
| 2207 | Aggregate Subscription - EndBound | implemented | agg_end_bound engine.rs:1321 (11506); test phase_c_start_end_delta_bounds aggregates_tests.rs:486-503 |
| 2210 | Aggregate - Total2 | implemented | engine.rs dispatch AGG_TOTAL2=11304 (engine.rs:1486); test aggregates_tests.rs:621 phase_d_time_average2_total2_match_stepped_area |
| 2220 | Aggregate - DurationInStateZero | implemented | engine.rs dispatch AGG_DURATION_IN_STATE_ZERO=11307 (engine.rs:1504-1506); test aggregates_tests.rs:1169 duration_in_state_boolean_splits_false_and_true |
| 2223 | Aggregate - DurationInStateNonZero | implemented | engine.rs dispatch AGG_DURATION_IN_STATE_NON_ZERO=11308 (engine.rs:1507-1509); test aggregates_tests.rs:1169,1187 duration_in_state_* tests |
| 2224 | Historical Access Replace Event | implemented | event_history.rs:195-201 update_event PerformUpdateType::Replace; tests history_events_inmemory.rs:91 + sqlite history_events.rs:60 update_event_insert_replace_and_read |
| 2231 | Push Model for Global Certificate and TrustList Management | implemented | Feature 101 (Run 1): fixed fabricated-NodeId/wrong-node-manager bugs in push_methods.rs; real ServerConfigurationType handlers wired to verified NodeIds: CreateSigningRequest (ns=0;i=12737, real PKCS#10 CSR), UpdateCertificate (13737), ApplyChanges (12740), CancelChanges (25708), GetRejectedList (12777), ResetToServerDefaults (25709); e2e-proven via gds_push_integration.rs + unit tests in push_methods.rs. CreateSelfSignedCertificate/DeleteCertificate/GetCertificates confirmed absent from the generated nodeset (SourceIssue). Feature 102 (Run 2): closes the remaining TrustList/CertificateGroup surface for DefaultApplicationGroup -- new gds/trust_list.rs wires Open (12647), OpenWithMasks (12663), Read (12652), Write (12655), GetPosition (12657), SetPosition (12660), Close (12650), CloseAndUpdate (12666), AddCertificate (12668), RemoveCertificate (12670), all empirically verified live against DefaultApplicationGroup.TrustList (ns=0;i=12642); PushTransaction extended to share ApplyChanges/CancelChanges with the TrustList's pending change; CertificateStore gained write-side trusted/issuer cert+CRL helpers (store_trusted_cert/store_issuer_cert/remove_trusted_cert/remove_issuer_cert/store_trusted_crl/store_issuer_crl/replace_*); 30 unit tests across push_methods.rs+trust_list.rs plus an extended gds_push_integration.rs wire-dispatch proof. DefaultHttpsGroup/DefaultUserTokenGroup CertificateGroups exist in the generated nodeset but are out of scope for this feature (documented follow-up); CertificateGroupType.GetRejectedList remains absent (SourceIssue, satisfied via ServerConfiguration.GetRejectedList). Sibling bug in pull_methods.rs (CU 3582, same fabricated-NodeId pattern) fixed by Feature 103 (Run 1). |
| 2232 | GDS Application Directory | gap | Directory RegisterApplication/QueryServers unimplemented: method.rs:98-104,131-135 maps to BadServiceUnsupported; no callbacks registered |
| 2233 | GDS LDS-ME Connectivity | gap | Searched "LDS-ME","LdsMe","lds_me" - only unrelated mdns.rs hits; no GDS-to-LDS-ME semi-automatic registration config found |
| 2236 | A & C CertificateExpiration | implemented | CertificateExpirationAlarm (alarms/certificate_expiration.rs) evaluates ExpirationDate/ExpirationLimit, register_certificate_expiration_alarm; test alarms.rs::certificate_expiration_alarm_activates_within_expiration_limit_and_clears_on_renewal |
| 2239 | A & C SystemOffNormal | implemented | DiscreteAlarmKind::SystemOffNormal (discrete.rs) parameterizes DiscreteAlarm to report SystemOffNormalAlarmType, same OffNormalAlarmType evaluation; test alarms.rs::system_off_normal_alarm_reports_type_definition_and_activates |
| 2256 | Aggregate Subscription - Delta | implemented | agg_delta engine.rs:776 (2359); test phase_b_count_average_range_delta aggregates_tests.rs:320-339 |
| 2258 | Redundancy Server | gap | Searched "redundancy"/"RedundancySupport"/"redundant_server" — only generated DataType stub (redundant_server_data_type.rs); no failover/clustering logic anywhere. |
| 2263 | Aggregate - Count | implemented | engine.rs dispatch AGG_COUNT=2352 (engine.rs:1494); tests aggregates_tests.rs:1003-1059 count_* family (3 tests) |
| 2264 | Historical Access Replace Value | implemented | data_history.rs:239-252 update_data PerformUpdateType::Replace; tests history_data_inmemory.rs:79-91 + hda.rs:353-389 e2e_replace_then_read_modified |
| 2267 | Aggregate - StartBound | implemented | engine.rs dispatch AGG_START_BOUND=11505 (engine.rs:1510); test aggregates_tests.rs:486 part13_start_end_and_delta_bounds_use_simple_bounds |
| 2271 | Discovery Register | implemented | Client::register_server (async-opcua-client/src/session/client.rs:818) + server-side periodic_discovery_server_registration (discovery.rs:86-117) calling it over a client-selected highest-security endpoint; test discovery.rs uses secured_endpoint() (SignAndEncrypt) throughout, e.g. discovery.rs:114 |
| 2273 | Aggregate - TimeAverage2 | implemented | engine.rs dispatch AGG_TIME_AVERAGE2=11285 (engine.rs:1481); test aggregates_tests.rs:621 phase_d_time_average2_total2_match_stepped_area |
| 2275 | A & C Trip | partial | discrete.rs:22,182-186 implements Trip via DiscreteAlarmKind::Trip; grep shows Trip kind never used in any test (only OffNormal is) |
| 2276 | Historical Access Annotations | implemented | annotations.rs attach_annotations_property + data_history.rs update_structure_data/read_annotations; simple.rs:658-718 history_read_annotations; test history_data_inmemory.rs:368 round-trip insert/replace/remove/read. Uses ReadAnnotationDataDetails not ReadRawModifiedDetails, but OPC-10000-11 5.1.2 confirms both are spec-valid |
| 2281 | Aggregate Subscription - VarianceSample | implemented | agg_variance_sample engine.rs:1021 (11428); test phase_b_variance_and_stddev aggregates_tests.rs:369-393 |
| 2282 | Aggregate - EndBound | implemented | engine.rs dispatch AGG_END_BOUND=11506 (engine.rs:1511); test aggregates_tests.rs:486 part13_start_end_and_delta_bounds_use_simple_bounds |
| 2289 | Historical Access Update Event | partial | event_history.rs:202-211 implements PerformUpdateType::Update (upsert) match arm; no dedicated test exercises Update mode for events (history_events_inmemory.rs + sqlite history_events.rs only test Insert/Replace) |
| 2291 | Attribute Read Complex | implemented | custom_types.rs test_data_type_tree_builder reads a DynamicStructure e2e (tests/integration/custom_types.rs:61). |
| 2302 | Aggregate Subscription - Minimum2 | implemented | agg_minimum2 engine.rs:809 (11286); test phase_d_minimum2_includes_simple_bound aggregates_tests.rs:580 |
| 2303 | Aggregate - PercentGood | implemented | engine.rs dispatch AGG_PERCENT_GOOD=2362 (engine.rs:1501); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad |
| 2305 | Aggregate - TimeAverage | implemented | engine.rs dispatch AGG_TIME_AVERAGE=2343 (engine.rs:1474); test aggregates_tests.rs:203 test_calculate_aggregate_average + phase_c/d family |
| 2309 | Historical Access Insert Event | implemented | event_history.rs:188-194 update_event PerformUpdateType::Insert; tests history_events_inmemory.rs:56 + sqlite history_events.rs:60 |
| 2314 | Aggregate - DurationBad | implemented | engine.rs dispatch AGG_DURATION_BAD=2361 (engine.rs:1500); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad |
| 2315 | A & C Refresh2 | implemented | handle_condition_refresh2 methods.rs:369-382; tested alarms.rs:584 condition_refresh2_targets_a_single_monitored_item |
| 2317 | View TranslateBrowsePath | implemented | TranslateBrowsePathsToNodeIds handler async-opcua-server/src/session/services/view.rs:388; test async-opcua/tests/integration/tier_a.rs:141 |
| 2318 | Monitor QueueSize_ServerMax | partial | Clamp (monitored_item.rs:314-336 sanitize_queue_size) caps queuesize to max but 0 dedicated test; comment admits event handling is "Future" |
| 2319 | Security Certificate Administration | implemented | ServerBuilder certificate_path/private_key_path (builder.rs:359-366), pki_dir (builder.rs:494-495); tested security_tests.rs:421-568. |
| 2323 | A & C Exclusive RateOfChange | implemented | RateOfChangeAlarm (alarms/rate_of_change.rs) reuses LimitAlarm evaluator against a computed per-second rate, register_rate_of_change_alarm; test alarms.rs::rate_of_change_alarm_reports_type_definition_and_activates_on_fast_change |
| 2328 | Discovery Get Endpoints | implemented | get_endpoints_with_filters incl profile-uri filter info.rs:342-378; tests core_tests.rs:100,358,366 |
| 2330 | Aggregate Subscription - StartBound | implemented | agg_start_bound engine.rs:1289 (11505); test aggregates_tests.rs:486, part13_start_end_and_delta_bounds:1342 |
| 2332 | Historical Access Structured Data Read Raw | gap | data_history.rs read_raw_modified (lines 131-180) reads only the raw_values map; annotation_values (structured data) is a separate store never reachable via ReadRawModifiedDetails for any NodeId |
| 2333 | A & C Instances | implemented | create_in_address_space inserts Object+state vars (state_machine.rs:97-377); demo-server/alarms.rs uses it; test limit.rs:1093 |
| 2335 | Aggregate - Delta | implemented | engine.rs dispatch AGG_DELTA=2359 (engine.rs:1498); test aggregates_tests.rs:320 phase_b_count_average_range_delta |
| 2338 | Aggregate - Custom | gap | only the standard Part 13 aggregate set (35 IDs, engine.rs supported_aggregates()/dispatch_aggregate) is implemented; default match arm (engine.rs:1521) returns BadAggregateNotSupported; no vendor/custom aggregate function found anywhere in aggregates/ module |
| 2339 | Aggregate - Start | gap | AggregateFunction_Start (i=2357 per schemas/1.05/NodeIds.csv:991) is absent from engine.rs AGG_ constants and dispatch_aggregate match (searched, no hit); only the distinct StartBound(11505) aggregate is implemented |
| 2343 | A & C Branch | implemented | Branch/create_branch/ack_branch/confirm_branch (state_machine.rs:9-28,396-452); test alarms.rs:1384 condition_branch_preserves_unacked |
| 2345 | Attribute Alternate Encoding | implemented | encode_value_as_xml/json (async-opcua-nodes/src/variable.rs:322); tests value_encodes_structure_as_default_xml/json (variable.rs:1165). |
| 2346 | Aggregate - Minimum | implemented | engine.rs dispatch AGG_MINIMUM=2346 (engine.rs:1476); test aggregates_tests.rs:228 test_calculate_aggregate_min_max |
| 2350 | Aggregate - DeltaBounds | implemented | engine.rs dispatch AGG_DELTA_BOUNDS=11507 (engine.rs:1512); tests aggregates_tests.rs:486 phase_c_start_end_delta_bounds, :1342 part13_* |
| 2352 | Discovery Find Servers Self | implemented | FindServers handled async-opcua-server/src/session/controller.rs:716; tests async-opcua/tests/integration/discovery.rs:83,119 |
| 2353 | Subscription Transfer | implemented | TransferSubscriptions handler subscriptions/mod.rs:1671-1787, dispatched message_handler.rs:368; e2e async-opcua/tests/integration/subscriptions.rs:632,790. |
| 2354 | Discovery Configuration | gap | Only inbound RegisterServer (LDS role, info.rs) + self-published discovery_urls found; no outbound "register self with external Discovery Server URL" config or disable switch. |
| 2358 | Aggregate Subscription - StandardDeviationSample | implemented | agg_std_dev_sample engine.rs:975; calculate_std_dev_sample math tested aggregates_tests.rs:173-182 |
| 2361 | Data Access TwoState | implemented | create_two_state_discrete_variable (data_access.rs) instantiates TwoStateDiscreteType with mandatory TrueState/FalseState; test data_access.rs::two_state_discrete_exposes_true_false_states_and_value |
| 2362 | Address Space Method | implemented | Method Nodes pervasive via MethodBuilder (async-opcua-nodes/src/method.rs); tested async-opcua/tests/integration/methods.rs. |
| 2371 | Protocol UA TCP | implemented | Hello/Ack+TCP codec async-opcua-core/src/comms/tcp_types.rs:244,373; exercised by full opc.tcp integration suite |
| 2375 | Aggregate Subscription - Average | implemented | agg_average engine.rs:684 (id 2342); test phase_b_count_average_range_delta aggregates_tests.rs:320-333 |
| 2376 | Aggregate Subscription - Minimum | implemented | agg_minimum engine.rs:792 (2346); test test_calculate_aggregate_min_max aggregates_tests.rs:228 |
| 2377 | Aggregate Subscription - Range | implemented | agg_range engine.rs:738 (2350); test phase_b_count_average_range_delta aggregates_tests.rs:320-336 |
| 2380 | Node Management Add Node | implemented | add_nodes_impl (node_manager/memory/memory_mgr_impl.rs:142); opt-in via clients_can_modify_address_space; tested node_management.rs. |
| 2381 | Aggregate Subscription - Maximum | implemented | agg_maximum engine.rs:888 (2347); test test_calculate_aggregate_min_max aggregates_tests.rs:228 |
| 2382 | Aggregate - Minimum2 | implemented | engine.rs dispatch AGG_MINIMUM2=11286 (engine.rs:1482); test aggregates_tests.rs:580 phase_d_minimum2_includes_simple_bound |
| 2383 | Historical Access Insert Value | implemented | data_history.rs:228-238 update_data PerformUpdateType::Insert; test hda.rs:320-351 e2e_inmemory_update_then_read_roundtrip |
| 2384 | Aggregate - WorstQuality2 | implemented | engine.rs dispatch AGG_WORST_QUALITY2=11292 (engine.rs:1485); test aggregates_tests.rs:1141 worst_quality_is_value_type_independent |
| 2389 | Attribute Write Values | implemented | Write handler async-opcua-server/src/session/message_handler.rs:820-852; tests async-opcua/tests/integration/write.rs |
| 2390 | A & C Non-Exclusive Deviation | implemented | DeviationAlarm (alarms/deviation.rs) reuses LimitAlarm evaluator against processValue-setpointValue for the non-exclusive path, register_deviation_alarm; test alarms.rs::deviation_alarm_reports_deviation_type_definition_and_activates_on_setpoint_deviation |
| 2391 | Method Call | implemented | Call service handled in session/message_handler.rs:411; tested call_trivial/call_args in async-opcua/tests/integration/methods.rs:26,61. |
| 2394 | Node Management Delete Node | implemented | delete_nodes_impl (memory_mgr_impl.rs:329); tested tests/integration/node_management.rs. |
| 2399 | Data Access Complex Number | gap | Searched 'ComplexNumberType' - zero hits outside generated nodeset. |
| 2400 | Session Change User | implemented | ActivateSession identity-change + revalidate_monitored_items_for_user manager.rs:1565,1591-1598; test manager.rs:2234-2253 |
| 2407 | Security Administration | implemented | builder.rs: add_user_token:567, SecurityPolicy::None/Sign/SignAndEncrypt:140-195, trust_client_certs:397-398, pki_dir:494; tested security_tests.rs. |
| 2408 | Aggregate Subscription - WorstQuality2 | implemented | agg_worst_quality2 engine.rs:1266 (11292); test worst_quality_is_value_type_independent aggregates_tests.rs:1153-1156 |
| 2422 | Auditing Secure Communication | partial | Audit events ride negotiated SecureChannel (Sign/SignAndEncrypt supported) but nothing specifically enforces/verifies encrypted delivery |
| 2423 | Base Info Rational Number | implemented | RationalNumberType present schemas/1.05/Opc.Ua.NodeSet2.xml, generated types/rational_number.rs; exposed via CoreNamespace import. |
| 2426 | Data Access DiscreteItemType | implemented | DiscreteItemType is abstract (OPC-10000-8 §5.3.3.1, 'no instances of this type can exist'); satisfied by any concrete subtype -- TwoStateDiscreteType/MultiStateDiscreteType/MultiStateValueDiscreteType (data_access.rs), tested in data_access.rs |
| 2446 | Address Space AddIn Reference | implemented | HasAddIn ReferenceType via generated core nodeset nodeset_19.rs:822, loaded by default address_space/mod.rs:11 |
| 2447 | Address Space AddIn DefaultInstanceBrowsename | implemented | DefaultInstanceBrowseName Property via generated nodeset_21.rs:2832, loaded by default node_manager/memory/core.rs:172 |
| 2454 | Method Call Complex | partial | Call passes arbitrary Vec<Variant> incl ExtensionObject generically (node_manager/method.rs) but no test uses a Structure argument. |
| 2474 | Data Access MultiStateDictionaryEntryDBT | gap | Investigated (feature 100): type exists in generated nodeset (nodeset_51.rs, ns=0;i=19077, from current schema snapshot) but is undocumented in both the local OPC-10000-8 v1.05.07 PDF and reference.opcfoundation.org -- deferred rather than implemented against unverifiable semantics, per spec.md Assumptions |
| 2476 | Base Info LocalTime | partial | Real computed LocalTime (chrono->TimeZoneDataType) node_manager/memory/core.rs:989-997; no test reads Server_LocalTime attribute |
| 2478 | Time Sync - OS based support | implemented | OsClockSource default TimeSyncSource impl async-opcua-server/src/time_sync.rs:112-124; unit test time_sync.rs:130-137 |
| 2479 | Time Sync - IEEE 1588 (PTP) | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2480 | Time Sync - IEEE 802.1AS | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2481 | Base Info NormalizedString DataType | implemented | NormalizedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import (core.rs:147). |
| 2482 | Base Info DecimalString DataType | implemented | DecimalString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2483 | Base Info Date DataTypes | implemented | DurationString/TimeString/DateString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2484 | Base Info BitFieldMaskDataType | implemented | BitFieldMaskDataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2485 | Base Info KeyValuePair | implemented | KeyValuePair in nodeset + generated types/key_value_pair.rs; used by published_data_set_data_type.rs. |
| 2486 | Base Info History Read Capabilities | implemented | core.rs:838-843 Server_ServerCapabilities_MaxHistoryContinuationPoints wired to limits.max_history_continuation_points; consumed by continuation.rs cache |
| 2487 | Base Info History UpdateEvents Capabilities | implemented | core.rs:882-887 MaxNodesPerHistoryUpdateEvents wired to limits.operational.max_nodes_per_history_update |
| 2488 | Base Info History UpdateData Capabilities | implemented | core.rs:876-881 MaxNodesPerHistoryUpdateData wired to limits.operational.max_nodes_per_history_update |
| 2489 | Base Info Node Management Capabilities | implemented | MaxNodesPerNodeManagement live-wired (node_manager/memory/core.rs:894-898), node-management feature. |
| 2490 | Base Info Subvariables of Structures | implemented | HasStructuredComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2491 | Base Info AssociatedWith | implemented | AssociatedWith present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2500 | Base Info EUInformation | implemented | EUInformation used/tested: tests/integration/custom_types.rs, async-opcua-types/src/tests/json.rs:344. |
| 2512 | Base Info OrderedList | implemented | base_info::create_ordered_list_in_address_space/add_ordered_object instantiate OrderedListType with HasOrderedComponent children implementing IOrderedObjectType via HasInterface (NumberInList is the authoritative order signal, not Browse response order, per OPC-10000-5 SS6.11's own rationale); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 2513 | Base Info Audio Type | implemented | AudioVariableType/AudioDataType present schemas/1.05/Opc.Ua.NodeSet2.xml; type-level exposure via CoreNamespace import. |
| 2514 | Base Info Spatial Data | implemented | VectorType/CartesianCoordinatesType/OrientationType/FrameType present in schemas/1.05; exposed via CoreNamespace import. |
| 2515 | Address Space Events 2 | implemented | Server EventNotifier=1 (nodeset_16.rs:989); BaseEventType/GeneratesEvent/EventTypes in nodeset; used in subscriptions.rs tests |
| 2516 | Base Info HasOrderedComponent | implemented | HasOrderedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2517 | Base Info Deprecated Information | implemented | IsDeprecated present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2518 | Base Info Image DataTypes | implemented | ImageBMP/GIF/JPG/PNG present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 2526 | Base Info History ReadData Capabilities | implemented | core.rs:864-869 MaxNodesPerHistoryReadData wired; enforced+tested in async-opcua/tests/integration/read.rs:1183-1208 (BadTooManyOperations) |
| 2527 | Base Info History ReadEvents Capabilities | implemented | core.rs:870-875 MaxNodesPerHistoryReadEvents wired to limits.operational.max_nodes_per_history_read_events; enforced in session/services/attribute.rs:126-134 |
| 2536 | Base Info ContentFilter | implemented | ContentFilter/Element DataTypes+encodings (node_ids.rs:168,7132); real WhereClause use+tests in where_clause.rs:13-56, select.rs:14-79. |
| 2539 | Address Space Dictionary Entries | gap | Searched 'HasDictionaryEntry' - only a type node in generated nodeset; no server code links dictionary entries. |
| 2600 | SecurityPolicy Support | implemented | 10+ SecurityPolicy variants incl None async-opcua-crypto/src/security_policy.rs:125-150; extensively tested + CI conformance matrix |
| 2649 | Base Info Choice States | gap | Searched 'GuardVariable'/'ChoiceState' - zero hits repo-wide. |
| 2664 | Historical Access Structured Data Read Modified | gap | no "modified" tracking exists for the annotation_values store (data_history.rs record_modified is only invoked from update_data, never update_structure_data at lines 290-355); ReadModified=true has no structured-data source |
| 2705 | Azure Identity Provider Authority Profile | gap | No "Azure" match in src; no authorityProfileURI concept exists anywhere (grep -r confirmed). |
| 2709 | OPC UA Authority Profile | gap | No Part-12 GetAccessToken Methods/AuthorizationServices instantiated; only unused generated defs (nodeset_18.rs, node_ids.rs). |
| 2711 | Base Info Selection List | implemented | base_info::create_selection_list_variable instantiates SelectionListType with Selections/SelectionDescriptions/RestrictToList; test base_info.rs::selection_list_exposes_selections_descriptions_and_restrict_flag |
| 2726 | A & C First in Group Alarm | gap | FirstInGroup only in generated nodeset (node_ids.rs, nodeset_23/8.rs); zero server code in async-opcua-server/src |
| 2730 | Aggregate - Range2 | implemented | engine.rs dispatch AGG_RANGE2=11288 (engine.rs:1484); aggregates_tests.rs references 11288 (phase_d family) |
| 2740 | Historical Access Structured Data Delete | gap | delete_raw_modified/delete_at_time (data_history.rs:357-466) only operate on raw_values/modified_values, never annotation_values; structured-data removal is only reachable via UpdateStructureDataDetails(Remove), itself restricted to Annotation-typed values (line 305), so still fails the generic-structured-data bar |
| 2743 | Aggregate Subscription - End | gap | AggregateFunction_End=2358 (node_ids.rs:7365) NOT in SUPPORTED_AGGREGATE_IDS engine.rs:44-79; only EndBound(11506) implemented |
| 2746 | A & C Exclusive Level | implemented | LimitAlarmKind::Level (limit.rs) parameterizes create_exclusive_in_address_space to report ExclusiveLevelAlarmType via register_level_alarm; test alarms.rs::level_alarm_reports_level_type_definition_not_generic_limit_and_activates |
| 2747 | Base Info System Status Underlying System | gap | Only structural ObjectType (nodeset_19.rs); grep of async-opcua-server/src for SystemStatusChangeEventType shutdown-event emission: empty |
| 2754 | Aggregate Subscription - Interpolative | implemented | agg_interpolative engine.rs:1344 (2341); tests aggregates_tests.rs:505,534,927 |
| 2759 | Aggregate - MinimumActualTime | implemented | engine.rs dispatch AGG_MINIMUM_ACTUAL_TIME=2348 (engine.rs:1478); test aggregates_tests.rs:345 phase_b_actual_time_returns_value_timestamp_not_interval_start |
| 2772 | Data Access Semantic Changes | gap | Searched 'SemanticChange' - no StatusCode info-bit constant exists anywhere in async-opcua-types. |
| 2776 | Data Access ValueAsDictionaryEntries Property | gap | Depends on MultiStateDictionaryEntryDiscreteBaseType (2474, deferred -- see that entry); the property node exists in generated nodeset (ns=0;i=19083) but same undocumented-semantics blocker applies. |
| 2781 | Address Space WriteMask | implemented | is_writable() enforces WriteMask per attribute (utils.rs:64-128); tests utils.rs:917-1017 + write.rs:538 (BadNotWritable). |
| 2785 | Protocol Configuration | implemented | ServerBuilder host()/port()/endpoint config: async-opcua-server/src/builder.rs:531,543,548. |
| 2786 | Time Sync - NTP | extensible | Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093). |
| 2802 | Security Role Server Management | implemented | AddRole/RemoveRole (role_management.rs:375-469), SecurityAdmin-gated; unit test :627; e2e rbac.rs:706,921 (wire-level pass+deny). |
| 2806 | Security Role Server RolePermissions | gap | No runtime Write path sets RolePermissions: SimpleNodeManager::write rejects non-Value attrs (simple.rs:1178); only set at node-creation. |
| 2808 | Security Role Server Authorization | implemented | Opt-in RBAC enforcement async-opcua-server/src/rbac/decision.rs:46-81; dedicated suite async-opcua/tests/integration/rbac.rs |
| 2809 | Address Space Atomicity | implemented | AccessLevelExType NonatomicRead/Write async-opcua-nodes/src/variable.rs:62,827-837; unit test variable.rs:990-997 |
| 2811 | Base Info State Machine Instance | partial | ProgramStateMachine (programs/state.rs) + ShelvingStateMachine (alarms/state_machine.rs) real+tested, but no GeneratesEvent wiring found. |
| 2813 | Base Info Available States and Transitions | gap | Searched 'AvailableStates'/'AvailableTransitions' - zero hits outside generated type def. |
| 2814 | Base Info Finite State Machine Instance | partial | ProgramStateMachine/ShelvingStateMachine real instances w/ tests, but AvailableStates/AvailableTransitions not populated. |
| 2817 | Security User JWT Token Policy | gap | UserTokenPolicy.issuer_endpoint_url hardcoded UAString::null() (authenticator.rs:327,341,353; session/manager.rs:2742) — never set. |
| 2818 | Monitor Complex Value | partial | Monitored-item sampling reuses Read's Variant pipeline (subscriptions/mod.rs:1230) but no test monitors a structured value. |
| 2820 | Address Space Full Array Only | implemented | validate_node_write_inner (address_space/write_validation.rs) rejects an IndexRange Write to AttributeId::Value with Bad_WriteNotSupported when AccessLevelExType::WriteFullArrayOnly is set; test write.rs::write_index_range_rejected_when_write_full_array_only |
| 2822 | Base Info Device Failure | gap | DeviceFailureEventType only structural (nodeset_19.rs); no server code constructs/fires it (grep across async-opcua-server/src empty) |
| 2823 | Security Invalid user token | partial | Fixed 100ms tarpit on every auth failure (session/negotiate.rs:16,28-40; tested security_tests.rs:2429); no escalating lockout. |
| 2831 | Data Access MultiStateValueDiscrete | implemented | create_multi_state_value_discrete_variable (data_access.rs) instantiates MultiStateValueDiscreteType with mandatory EnumValues/ValueAsText (non-contiguous codes); test data_access.rs::multi_state_value_discrete_tracks_non_contiguous_enum_values |
| 2837 | UA Binary Encoding | implemented | BinaryEncodable/BinaryDecodable traits async-opcua-types/src/encoding.rs:445-482, pervasive derive use; tests encoding.rs:919 |
| 2845 | Base Info RequestServerStateChange Method | gap | Only generated NodeId constants (ServerType_RequestServerStateChange, node_ids.rs:1103-1105); no add_method_cb handler found anywhere. |
| 2852 | A & C Condition Sub-Classes | gap | condition_sub_class_id field exists on BaseEventType only (events/event.rs:70-73); never set anywhere in async-opcua-server/src |
| 2853 | UA Secure Conversation | implemented | SecureChannel/OpenSecureChannel comms/secure_channel.rs:657; tests secure_channel.rs:136-663, integration secure_channel.rs:15 |
| 2861 | A & C Discrepancy | implemented | DiscrepancyAlarm (alarms/discrepancy.rs) tracks process-vs-TargetValueNode discrepancy beyond Tolerance persisting past ExpectedTime, register_discrepancy_alarm; test alarms.rs::discrepancy_alarm_activates_after_expected_time_and_clears_at_target |
| 2863 | Security Policy Required | implemented | Modern policies default-on, legacy Basic128Rsa15/Basic256 opt-in behind legacy-crypto feature builder.rs:142-166; matrix test |
| 2867 | Protocol Reverse Connect Server | implemented | async-opcua-server/src/reverse_connect.rs (ReverseConnectionManager etc.); e2e async-opcua/tests/integration/reverse_connect.rs:16-17 test_reverse_connect. |
| 2871 | Discovery Get Endpoints SessionLess | gap | GetEndpoints handler (session/controller.rs:697-707) has no Transport-URI "SL" query-string filter/sessionless-endpoint logic. |
| 2873 | Security Role Server DefaultRolePermissions | gap | DefaultRolePermissions only settable via ServerBuilder config pre-startup (builder.rs:439); no live Write path (core.rs:1104). |
| 2877 | A & C On-Off Delay | implemented | ConditionStateMachine::gate_active + LimitAlarm.{on,off}_delay_ms (state_machine.rs, limit.rs::with_delays) defer ActiveState commit until the delay elapses; test alarms.rs::on_delay_and_off_delay_defer_activation_and_deactivation |
| 2879 | A & C Re-Alarming | implemented | ConditionStateMachine::maybe_re_alarm/reset_re_alarm + LimitAlarm::with_re_alarm (state_machine.rs, limit.rs); ReAlarmRepeatCount server-maintained, resets on return to normal per spec text (corrected from task's initial ack-reset assumption); test alarms.rs::re_alarm_time_renotifies_while_active_and_resets_on_return_to_normal |
| 2881 | A & C Audible Sound | implemented | ConditionStateMachine::recompute_audible_enabled (state_machine.rs) computes AudibleEnabled from active/acked/silenced; AudibleSound modeled as a plain property (AudioDataType has no generated Rust type); Acknowledge now also auto-silences (transitions.rs) per spec; test alarms.rs::audible_enabled_tracks_active_unacked_unsilenced_and_acknowledge_auto_silences |
| 2893 | A & C Suppression by Operator | implemented | AlarmConditionType_Suppress/Unsuppress Methods registered (methods.rs); handle_condition_suppress/unsuppress call set_suppressed; test alarms.rs::suppress_unsuppress_methods_toggle_suppressed_state |
| 2896 | A & C Silencing | implemented | SilenceState variable added (state_machine.rs) + AlarmConditionType_Silence Method registered; handle_condition_silence calls set_silenced; test alarms.rs::silence_method_toggles_silence_state_and_is_idempotent |
| 2897 | A & C Suppression | implemented | SuppressedState var+get/set_suppressed wired to SuppressedOrShelved (state_machine.rs), now tested via alarms.rs::suppress_unsuppress_methods_toggle_suppressed_state |
| 2902 | OAuth2 Authority Profile | gap | Server validates OAuth2 JWTs (crypto/identity/jwt_validator.rs) but no HTTPS token-fetch flow to an OAuth2 authority exists. |
| 2918 | Address Space Source Hierarchy | partial | ObjectBuilder::has_event_source exists (async-opcua-nodes/src/object.rs:49-56) but zero call sites building a hierarchy; alarms wire HasCondition only (alarms/limit.rs:351), not HasEventSource. |
| 2921 | A & C Alarm | implemented | Active/Acked/Confirmed/Retain/Severity/Message/branch mechanics (state_machine.rs, transitions.rs); test alarms.rs:64 |
| 2927 | A & C Acknowledge | implemented | handle_ack_method methods.rs:65-150 + AcknowledgeableConditionType_Acknowledge registered methods.rs:654-658; tested alarms.rs:64,706 |
| 2928 | Monitored Items Deadband Filter | implemented | Absolute DataChangeFilter deadband subscriptions/monitored_item/filters.rs:128-137; unit test filters.rs:175 |
| 2929 | Historical Access Modified Values | implemented | data_history.rs:79-120 read_modified_values + record_modified (raw data); tests history_data_inmemory.rs:285 replace_is_readable_as_modified_replace, :303 deletes_are_readable_as_modified_delete, :331 never_modified_value_has_no_modified_entry |
| 2936 | Attribute Write StatusCode & Timestamp | implemented | write_node_value (address_space/utils.rs) threads client status/source_timestamp/server_timestamp through to Variable::set_value_range (fixed a real bug: server_timestamp was hardcoded to now()); test write.rs::write_status_code_and_timestamps_round_trip |
| 2937 | Historical Access Structured Data Update | gap | update_structure_data (data_history.rs:290-355) rejects non-Annotation values at line 306 (BadTypeMismatch); same restriction in sqlite backend.rs:1030 — no generic structured-data update |
| 2939 | Node Management Add Ref | implemented | add_references_impl (memory_mgr_impl.rs:414); tested memory_mgr_impl.rs:2453 (mismatch rejection). |
| 2940 | Base Info GetMonitoredItems Method | implemented | GetMonitoredItems method node_manager/memory/core.rs:1195-1207; test methods.rs:291-332 call_get_monitored_items |
| 2941 | Aggregate Subscription - MaximumActualTime2 | implemented | agg_maximum_actual_time2 engine.rs:950 (11306); test aggregates_tests.rs:952 (duplicate-extrema) |
| 2943 | Historical Access Delete Event | implemented | event_history.rs:226-250 delete_event; tests history_events_inmemory.rs:128 delete_event_by_id + sqlite history_events.rs:117 |
| 2946 | A & C Non-Exclusive RateOfChange | implemented | RateOfChangeAlarm (alarms/rate_of_change.rs) non-exclusive path via create_non_exclusive_in_address_space + LimitAlarmKind::RateOfChange; test alarms.rs::rate_of_change_alarm_reports_type_definition_and_activates_on_fast_change |
| 2947 | Historical Access Events | implemented | event_history.rs:68-138 read_events using ParsedEventFilter; test history_tests.rs:407 test_history_read_events_empty_result |
| 2948 | Aggregate - VariancePopulation | implemented | engine.rs dispatch AGG_VARIANCE_POPULATION=11429 (engine.rs:1520); test aggregates_tests.rs:369 phase_b_variance_and_stddev |
| 2950 | Historical Access ServerTimestamp | partial | both backends persist a distinct server_timestamp (sqlite backend.rs:105/417/854 dedicated column, query.rs:93 populates on read; in-memory stores full DataValue); config flag capabilities.rs:34 defaults false; no test asserts server_timestamp survives distinct from source_timestamp on read, and simple.rs history_read_raw_modified ignores timestamps_to_return (unused param) |
| 2951 | A & C Exclusive Deviation | implemented | DeviationAlarm (alarms/deviation.rs) exclusive path via create_exclusive_in_address_space + LimitAlarmKind::Deviation; test alarms.rs::deviation_alarm_reports_deviation_type_definition_and_activates_on_setpoint_deviation |
| 2952 | Aggregate Subscription - MinimumActualTime2 | implemented | agg_minimum_actual_time2 engine.rs:863 (11305); test phase_d_min_actual_time2_uses_bound_timestamp aggregates_tests.rs:603 |
| 2954 | Aggregate Subscription - DurationBad | implemented | agg_duration_bad engine.rs:1162 (2361); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694 |
| 2955 | Aggregate Subscription - StandardDeviationPopulation | implemented | agg_std_dev_population engine.rs:1033 (11427); test phase_b_variance_and_stddev aggregates_tests.rs:369-393 |
| 2957 | A & C Refresh | implemented | handle_condition_refresh methods.rs:359-367; tested alarms.rs:511 condition_refresh_delivers_retained_alarm_to_late_subscriber |
| 2958 | Aggregate Subscription - Count | implemented | agg_count engine.rs:1067 (2352); tests aggregates_tests.rs:320, count_boolean_source_counts:1003 |
| 2960 | Aggregate - VarianceSample | implemented | engine.rs dispatch AGG_VARIANCE_SAMPLE=11428 (engine.rs:1519); test aggregates_tests.rs:369 phase_b_variance_and_stddev |
| 2962 | Aggregate - Maximum | implemented | engine.rs dispatch AGG_MAXIMUM=2347 (engine.rs:1477); test aggregates_tests.rs:228 test_calculate_aggregate_min_max |
| 2963 | Monitor Basic | implemented | create/modify/delete_monitored_items + set_monitoring_mode (session/services/monitored_items.rs:170-573); tested subscriptions.rs. |
| 2965 | A & C Basic | implemented | ConditionStateMachine base creates EnabledState/Retain/etc for every condition (state_machine.rs:126-256); foundational to all A&C tests |
| 2969 | Base Info ValueAsText | implemented | base_info::create_enum_variable_with_value_as_text/update_enum_value attach a ValueAsText property kept in sync with an enumerated Variable's Value; test base_info.rs::value_as_text_tracks_enumerated_value_changes |
| 2974 | Aggregate Subscription - MinimumActualTime | implemented | agg_minimum_actual_time engine.rs:826 (2348); test phase_b_actual_time_returns_value_timestamp aggregates_tests.rs:345 |
| 2975 | Aggregate - PercentBad | implemented | engine.rs dispatch AGG_PERCENT_BAD=2363 (engine.rs:1502); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad |
| 2978 | Base Info SemanticChange | gap | SemanticChangeEventType only a generated type (events/generated.rs:699); never raised; no semantic-changed StatusCode bit usage |
| 2984 | Data Access DoubleComplex Number | gap | Searched 'DoubleComplexNumberType' - zero hits outside generated nodeset. |
| 2985 | Aggregate - NumberOfTransitions | implemented | engine.rs dispatch AGG_NUMBER_OF_TRANSITIONS=2355 (engine.rs:1495-1497); tests aggregates_tests.rs:1060 transitions_boolean_counts_each_flip, :1076 transitions_value_change_not_zero_crossing |
| 2988 | Data Access MultiState | implemented | create_multi_state_discrete_variable (data_access.rs) instantiates MultiStateDiscreteType with mandatory EnumStrings; test data_access.rs::multi_state_discrete_exposes_enum_strings_and_value |
| 2991 | Historical Access Structured Data Time Instance | gap | depends on ReadAtTimeDetails, which has zero server-side implementation for any backend (see CU 3020); a fortiori unsupported for structured data |
| 2993 | Aggregate - AnnotationCount | implemented | engine.rs dispatch AGG_ANNOTATION_COUNT=2351 (engine.rs:1493); test aggregates_tests.rs:1275 annotation_count_counts_annotations_in_interval; cross-backend parity via history_data_inmemory.rs:441 + sqlite history_update_data.rs:455 |
| 2996 | Aggregate - Average | implemented | engine.rs dispatch AGG_AVERAGE=2342 (engine.rs:1473); test aggregates_tests.rs:203 test_calculate_aggregate_average |
| 2998 | Aggregate Subscription - DurationInStateZero | implemented | agg_duration_in_state_zero engine.rs:1197 (11307); test duration_in_state_boolean_splits aggregates_tests.rs:1169 |
| 3000 | Documentation - Installation | implemented | docs/setup.md gives install/toolchain/feature-flag/cert-loading instructions. |
| 3001 | A & C Non-Exclusive Level | implemented | LimitAlarmKind::Level (limit.rs) parameterizes create_non_exclusive_in_address_space to report NonExclusiveLevelAlarmType via register_level_alarm; same evaluation path as CU 2746, no dedicated non-exclusive test yet |
| 3004 | A & C Discrete | implemented | discrete.rs covers OffNormalAlarmType+TripAlarmType, both DiscreteAlarmType subtypes (discrete.rs:1-2,182-186); tested alarms.rs:1176,2421 |
| 3006 | Aggregate - StandardDeviationSample | implemented | engine.rs dispatch AGG_STANDARD_DEVIATION_SAMPLE=11426 (engine.rs:1513-1515); test aggregates_tests.rs:173 test_calculate_std_dev_sample, :369 phase_b_variance_and_stddev |
| 3010 | Aggregate Subscription - PercentBad | implemented | agg_percent_bad engine.rs:1183 (2363); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694 |
| 3011 | Aggregate - Range | implemented | engine.rs dispatch AGG_RANGE=2350 (engine.rs:1480); test aggregates_tests.rs:320 phase_b_count_average_range_delta |
| 3015 | Historical Access Structured Data Replace | gap | update_structure_data Replace arm (data_history.rs:322-331) gated by is_annotation_data_value (line 305) — same annotation-only restriction, no generic structured data replace |
| 3018 | Aggregate - MaximumActualTime | implemented | engine.rs dispatch AGG_MAXIMUM_ACTUAL_TIME=2349 (engine.rs:1479); test aggregates_tests.rs:345 phase_b_actual_time_returns_value_timestamp_not_interval_start |
| 3020 | Historical Access Time Instance | gap | node_manager/mod.rs:433 declares history_read_at_time (default BadHistoryOperationUnsupported at memory_mgr_impl.rs:1759-1767); simple.rs has no override for it (raw_modified/processed/events/annotations all are overridden there, at_time is not) — ReadAtTimeDetails always fails server-side |
| 3026 | Address Space UserWriteMask Multilevel | gap | Same as 2163: UserWriteMask never varies by user/role anywhere (grep confirms zero dynamic computation); no multilevel test. |
| 3027 | Redundancy Server Transparent | gap | Same search as Redundancy Server (2258); no transparent-redundancy failover code found. |
| 3032 | Aggregate - Total | implemented | engine.rs dispatch AGG_TOTAL=2344 (engine.rs:1475); aggregates_tests.rs references 2344 in phase tests |
| 3043 | Aggregate Historical Configuration | gap | no helper analogous to attach_annotations_property instantiates a per-Variable HistoricalConfiguration+AggregateConfigurationType Object (searched async-opcua-server/src, no hits); middleware.rs read_processed_aggregates (lines 57-106) sources AggregateConfiguration only from the request parameter, never from an address-space node |
| 3047 | Aggregate Subscription - Range2 | implemented | agg_range2 engine.rs:757 (11288); test aggregates_tests.rs:580-598 (Range2=15) |
| 3048 | Aggregate Subscription - PercentGood | implemented | agg_percent_good engine.rs:1169 (2362); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694 |
| 3049 | A & C Confirm | implemented | handle_confirm_method methods.rs:225-278 + AcknowledgeableConditionType_Confirm registered methods.rs:668-671; tested alarms.rs:706,268 |
| 3053 | Historical Access Update Value | implemented | data_history.rs:253-268 update_data PerformUpdateType::Update; test history_data_inmemory.rs:93-105 update_data_matrix_matches_sqlite_semantics |
| 3055 | Aggregate - WorstQuality | implemented | engine.rs dispatch AGG_WORST_QUALITY=2364 (engine.rs:1503); test aggregates_tests.rs:1141 worst_quality_is_value_type_independent |
| 3060 | Documentation - Multiple Languages | gap | Searched docs/ for locale variants (*.fr.md etc.) and translated dirs — none; all docs English-only. |
| 3061 | Aggregate - End | gap | AggregateFunction_End (i=2358 per schemas/1.05/NodeIds.csv:992) is absent from engine.rs AGG_ constants and dispatch (searched, no hit); only the distinct EndBound(11506) aggregate is implemented |
| 3062 | Aggregate Subscription - Total | implemented | agg_total engine.rs:729 (2344); test phase_f_time_average_excludes_bad_regions aggregates_tests.rs:781-825 |
| 3064 | Address Space Notifier Hierarchy | gap | No has_notifier reference-builder method exists (only event_notifier attribute setter, object.rs:29); HasNotifier only appears in ref-type-hierarchy declaration (address_space/mod.rs:939), no instance hierarchy built/tested. |
| 3072 | Attribute Read | implemented | Read applies IndexRange via NumericRange::range_of node_manager/memory/core.rs:1079-1080; tests read.rs:1425,794 |
| 3073 | View RegisterNodes | implemented | RegisterNodes/UnregisterNodes handler session/services/view.rs:540, memory_mgr_impl.rs:1608; e2e test browse.rs:675 |
| 3075 | Aggregate Subscription - TimeAverage | implemented | agg_time_average engine.rs:710 (2343); tests aggregates_tests.rs:781,516 |
| 3080 | Security Default ApplicationInstance Certificate | implemented | CertificateStore::create_and_store_application_instance_cert certificate_store.rs:265, default builder.rs:119; test crypto.rs:46 |
| 3081 | Historical Access Delete Value | implemented | data_history.rs:357-466 delete_raw_modified/delete_at_time; test hda.rs:391-428 e2e_delete_at_time_via_client |
| 3083 | A & C Comment | implemented | handle_add_comment_method methods.rs:152-222 + ConditionType_AddComment registered methods.rs:660-663; tested alarms.rs:1574 |
| 3084 | Documentation - Users Guide | implemented | docs/server.md, docs/advanced_server.md, docs/advanced_features.md describe server functionality. |
| 3085 | Aggregate - DurationGood | implemented | engine.rs dispatch AGG_DURATION_GOOD=2360 (engine.rs:1499); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad |
| 3098 | A & C OffNormal | implemented | discrete.rs DiscreteAlarmKind::OffNormal; tested alarms.rs:1176 offnormal_alarm_activates_off_normal, 2421 auto_fires |
| 3099 | Aggregate Subscription - NumberOfTransitions | implemented | agg_number_of_transitions engine.rs:1205 (2355); tests aggregates_tests.rs:1060,1076 |
| 3101 | Aggregate - MaximumActualTime2 | implemented | engine.rs dispatch AGG_MAXIMUM_ACTUAL_TIME2=11306 (engine.rs:1490-1492); test aggregates_tests.rs:612 (MaximumActualTime2 case in phase_d_min_actual_time2_uses_bound_timestamp) |
| 3105 | Aggregate Subscription - DurationGood | implemented | agg_duration_good engine.rs:1155 (2360); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694 |
| 3107 | Documentation - Supported Profiles | implemented | docs/opcua-foundation-profile-roadmap.md + docs/ctt-conformance.md document supported profiles and certification-test evidence. |
| 3108 | Aggregate Subscription - Start | gap | AggregateFunction_Start=2357 (node_ids.rs:7364) NOT in SUPPORTED_AGGREGATE_IDS engine.rs:44-79; only StartBound(11505) implemented |
| 3112 | Data Access PercentDeadband | implemented | PercentDeadband tested vs EURange AnalogItemType (tests/integration/datachange_overflow.rs:151-245). |
| 3121 | Monitor Aggregate Filter | implemented | monitored_item.rs ParsedAggregateFilter:101,139; e2e test aggregate_filter_average subscriptions.rs:2276,2384 |
| 3125 | Security User X509 | implemented | X509 user cert validated incl. POP sig (info.rs:1291-1332); tests security_tests.rs:1565-1863 (untrusted/expired/revoked). |
| 3126 | Aggregate Subscription - TimeAverage2 | implemented | agg_time_average2 engine.rs:714 (11285); test phase_d_time_average2_total2 aggregates_tests.rs:621 |
| 3127 | Base Info OptionSet | implemented | base_info::create_option_set_variable instantiates OptionSetType with OptionSetValues/BitMask; test base_info.rs::option_set_exposes_per_bit_values_and_bitmask |
| 3130 | Aggregate Subscription - MaximumActualTime | implemented | agg_maximum_actual_time engine.rs:922 (2349); test phase_b_actual_time_returns_value_timestamp aggregates_tests.rs:345 |
| 3137 | Aggregate Subscription - Custom | gap | No custom aggregate extensibility; dispatch_aggregate engine.rs:1466 is a fixed closed match, unknown ids -> BadAggregateNotSupported |
| 3142 | Monitor Alternate Encoding | partial | sample() passes data_encoding through same pipeline as Read (subscriptions/mod.rs:1230), no monitored-item XML/JSON test found. |
| 3143 | Subscription PublishRequest Queue Overflow | implemented | enqueue_publish_request pops oldest on overflow, returns BadTooManyPublishRequests (session_subscriptions.rs:767); test :1581. |
| 3144 | Aggregate Subscription - DurationInStateNonZero | implemented | agg_duration_in_state_non_zero engine.rs:1201 (11308); test duration_in_state_boolean_splits aggregates_tests.rs:1169 |
| 3146 | Monitor Triggering | implemented | SetTriggering handler message_handler.rs:676, actor.rs:104/392/704; e2e tests triggering.rs:43,160 |
| 3147 | Attribute Write Index | implemented | Variant::set_range_of variant/mod.rs:1641 via Variable::set_value_range variable.rs:746; test write.rs:688,1008 |
| 3150 | Monitor Events | implemented | Full FilterOperator set incl Like/Between/InList/BitwiseAnd/OfType (async-opcua-nodes/src/events/evaluate.rs); tested event_filter_tests.rs |
| 3153 | Node Management Delete Ref | implemented | delete_references_impl (memory_mgr_impl.rs:704); tested node_management.rs / memory_mgr_impl.rs. |
| 3159 | Aggregate - Interpolative | implemented | engine.rs dispatch AGG_INTERPOLATIVE=2341 (engine.rs:1472); tests aggregates_tests.rs:505 phase_c_interpolative_at_interval_start, :534 phase_c_interpolative_before_data_is_bad_no_data |
| 3162 | Aggregate - StandardDeviationPopulation | implemented | engine.rs dispatch AGG_STANDARD_DEVIATION_POPULATION=11427 (engine.rs:1516-1518); test aggregates_tests.rs:369 phase_b_variance_and_stddev |
| 3165 | A & C Shelving | implemented | one_shot_shelve/timed_shelve/unshelve state_machine.rs:671-707 + methods registered methods.rs:674-693; tested alarms.rs:1255,1343 |
| 3170 | Discovery Register2 | implemented | Client::register_server2 (async-opcua-client/src/session/client.rs:879) + client-callable discovery-configuration support; tests discovery.rs::register_server2_mdns_config_result_matches_feature_support and :303 over secured_endpoint() (SignAndEncrypt) |
| 3171 | Discovery Server Announcement using mDNS  | implemented | mDNS responder discovery/mdns.rs:81 start_responder, wired at server.rs:516,525,827,1162; unit tests mdns.rs:521-673. |
| 3175 | Session Base | implemented | CreateSession/ActivateSession/CloseSession session/manager.rs; SecurityMode::None optional cert/nonce manager.rs:283-300; test :47,90 |
| 3182 | Authorization Service Configuration Server | gap | No AuthorizationServiceConfigurationType/AccessToken code found; searched "AuthorizationService","RequestAccessToken" - zero hits |
| 3184 | Base Info Core Structure 2 | implemented | Root/Objects/Server + ServerArray/NamespaceArray/ServiceLevel node_manager/memory/core.rs:986-1063; tests browse.rs:35, read.rs:42-43 |
| 3185 | Base Info Core Types Folders | implemented | Types/ObjectTypes/DataTypes/VariableTypes/ReferenceTypes folders exposed via default CoreNamespace import (core.rs:147). |
| 3186 | Base Info Core Views Folder | implemented | ViewsFolder entry point address_space/mod.rs:774-779; test at same location |
| 3188 | Base Info Base Types | implemented | Base built-in types present in schemas/1.05; imported via core.rs:147, exercised by address_space/mod.rs test suite. |
| 3189 | Base Info ServerType | implemented | ServerType is the root of the default AddressSpace; exercised across suite e.g. tests/integration/browse.rs. |
| 3192 | Base Info Diagnostics | implemented | EnabledFlag/ServerDiagnosticsSummary/SubscriptionDiagnosticsArray diagnostics/server.rs, core.rs:501-509; e2e read.rs:1604-1841 |
| 3194 | Base Info Events Capabilities | partial | MaxSelectClauseParameters/MaxWhereClauseParameters nodes exist (nodeset_28.rs:4158) but Value is DataValue::null(), not live-wired. |
| 3196 | Base Info Fixed SamplingInterval | implemented | CU is conditional on the Server using a fixed set of sampling intervals (OPC-10000-5 SS7.9/SS12.8); this server negotiates a continuously-variable client-requested interval per monitored item (sanitize_sampling_interval, subscriptions/monitored_item.rs:299-311), so the precondition never holds and non-exposure of SamplingIntervalDiagnosticsArray is spec-conformant, not a gap -- documented in docs/server-capacity-limits.md |
| 3197 | Base Info Security Role Capabilities | implemented | RoleSet on ServerCapabilities (role_management.rs:479-481); test rbac.rs:287-316 verifies i=15606 + 8 role nodes. |
| 3198 | Base Info Estimated Return Time | implemented | ServerStatusWrapper::schedule_shutdown/estimated_return_time (server_status.rs) + ServerHandle::shutdown_after_with_return_time (server_handle.rs) extend the existing shutdown mechanism; wired core.rs get_attribute; test base_info.rs::estimated_return_time_reflects_scheduled_shutdown_and_is_null_otherwise |
| 3199 | Base Info System Status | gap | SystemStatusChangeEventType has no server-side emission on shutdown; server_status.rs/server_handle.rs never calls notify_event/raise_event |
| 3201 | Base Info Custom Type System | partial | custom-codegen sample (samples/custom-codegen) demonstrates a full custom-type inheritance tree + generated Encoding Objects via async-opcua-codegen (types/encoding_ids.rs, types/gen.rs); no completeness e2e test proving all custom EventTypes are exposed alongside their encoding objects. Distinct from CU 5801 (which covers standard-nodeset type completeness, closed as a byproduct of the many typed-instantiation CUs) -- this one is specifically about CUSTOM (non-standard) types and remains open |
| 3203 | Base Info Model Change General | implemented | GeneralModelChangeEvent fired on add/delete_nodes/refs (model_change.rs, memory_mgr_impl.rs:325); e2e test node_management.rs:1437. |
| 3206 | Base Info EventQueueOverflow EventType | implemented | monitored_item.rs:1052-1085 notify_event inserts EventQueueOverflowEventType on overflow; tested subscriptions.rs:1697-1779 |
| 3207 | Base Info OptionSet DataType | implemented | OptionSet DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3210 | Base Info FileType Write | implemented | Feature 106: gds/file_node.rs's structurally-correct FileType Write Method now has real behavior. New fota/file_access.rs wires a real Write handler against the method's NodeId via SimpleNodeManagerImpl::add_method_callback_with_context (register_file_access_methods), performing a real std::fs::File write at the handle's tracked position (OPC-10000-20 section 4.2.5), bounded by MaxByteStringLength, empty/null data a documented no-op. e2e-proven via fota_file_access_integration.rs (real client -> Call service -> registered handler -> real bytes on disk, read back byte-for-byte through a separate Open/Read/Close sequence) plus 11 unit tests. |
| 3211 | Base Info FileDirectoryType Base | gap | FileDirectoryType only as generated NodeId consts/abstract type (node_ids.rs:10624, nodeset_16.rs); no instance created anywhere in server/samples. |
| 3213 | Base Info FileType Base | implemented | Feature 106: closes the base FileType surface (OPC-10000-20 section 4.2, grounded against the local Part 20 PDF -- FileType is defined there, not Part 5 as its name might suggest). New fota/file_access.rs implements real Open (mode-byte decode, exact spec open-conflict rules -- a write-open refused while open in any mode, a read-open refused only while open for write, verified via 2 dedicated unit tests), Close, Read (EOF-is-empty-not-error, MaxByteStringLength-capped), Write (empty-data-is-noop, MaxByteStringLength-rejected), GetPosition/SetPosition (EOF-clamping), all against a real std::fs::File per session-scoped handle (moka::sync::Cache, modeled on gds/trust_list's TrustListHandleRegistry but disk- not memory-backed, appropriate for large files). Status codes independently re-verified against the real spec text, not assumed (e.g. Bad_InvalidArgument for any bad/foreign-session handle, corrected from TrustList's own Bad_InvalidState convention which is Part-12-specific, not base FileType). OpenCount live-tracked. e2e-proven via fota_file_access_integration.rs plus 11 unit tests. FileDirectoryType (CU 3211) and TemporaryFileTransferType (CUs 3810-3813/5791) are explicit, separately-scoped follow-ups -- see TODO.md. |
| 3214 | Base Info Range DataType | implemented | Range in nodeset + generated types/range.rs; used as EURange in datachange_overflow.rs, alarms.rs. |
| 3224 | Auditing NodeManagement | partial | Fires for AddNodes/DeleteNodes/AddRef/DeleteRef memory_mgr_impl.rs:324,409,699,878 -> audit_events.rs:24-97; only AddNodes tested |
| 3226 | Auditing History Services | gap | HistoryUpdate handler attribute.rs:286-386 has no audit dispatch; AuditHistoryUpdateEventType only generated, never constructed, no test |
| 3228 | Auditing Write | implemented | dispatch_write_audit (audit.rs:818, message_handler.rs:899) emits AuditWriteUpdateEventType; e2e write.rs:1063. |
| 3230 | Auditing Method | implemented | dispatch_method_audit (audit.rs:799, method.rs:107) emits AuditUpdateMethodEventType; e2e methods.rs:608. |
| 3323 | Data Access YArrayItemType | implemented | create_y_array_item_variable (data_access.rs) instantiates YArrayItemType with mandatory EURange/EngineeringUnits/Title/AxisScaleType/XAxisDefinition; test data_access.rs::y_array_item_exposes_spectrum_and_x_axis_definition |
| 3324 | Data Access XYArrayItemType | implemented | create_xy_array_item_variable (data_access.rs) instantiates XYArrayItemType (XVType-valued) with mandatory base Properties + XAxisDefinition; test data_access.rs::xy_array_item_exposes_xv_type_peaks |
| 3325 | Data Access ImageItemType | implemented | create_image_item_variable (data_access.rs) instantiates ImageItemType (2-D) with mandatory base Properties + X/YAxisDefinition; test data_access.rs::image_item_exposes_2d_matrix_and_both_axis_definitions |
| 3326 | Data Access CubeItemType | implemented | create_cube_item_variable (data_access.rs) instantiates CubeItemType (3-D) with mandatory base Properties + X/Y/ZAxisDefinition; test data_access.rs::cube_item_exposes_3d_volume_and_all_three_axis_definitions |
| 3327 | Data Access NDimensionArrayItemType | implemented | create_nd_dimension_array_item_variable (data_access.rs) instantiates NDimensionArrayItemType with one AxisDefinition per dimension; test data_access.rs::nd_dimension_array_item_exposes_one_axis_definition_per_dimension |
| 3328 | Data Access AxisInformationType | implemented | AxisInformation in schemas/1.05 + generated types/axis_information.rs; type-level exposure via CoreNamespace import. |
| 3524 | Address Space Dictionary IRDI | gap | Searched 'IrdiDictionaryEntryType' - only a nodeset type node; no instance/dictionary wiring. |
| 3525 | Address Space Dictionary URI | gap | Searched 'UriDictionaryEntryType' - only a nodeset type node; no instance/dictionary wiring. |
| 3530 | View Basic 2 | implemented | Browse/BrowseNext w/ continuation points view.rs:213; tests browse.rs:252, :757 (Bad_ContinuationPointInvalid) |
| 3532 | Monitor Queueing | implemented | queue_size clamp monitored_item.rs:314-336, overflow:1067-1110; test datachange_overflow.rs:33-141 (size=2 discardOldest) |
| 3534 | Subscription Multiple | implemented | tests/integration/subscriptions.rs:476-509 creates >=2 subscriptions in one session, asserts BadTooManySubscriptions on next |
| 3535 | Subscription Retransmission Queue | implemented | RetransmissionQueue (retransmission_queue.rs, sized session_subscriptions.rs:1100) + Republish; test subscriptions.rs:1229 |
| 3536 | Security User Name Password 2 | implemented | Username/Password encrypted per policy (negotiate.rs:94-207 decrypt_identity_token_secret); tests negotiate.rs:259-330. |
| 3538 | Security Role Server Base 2 | implemented | RolePermissions/UserRolePermissions/AccessRestrictions enforced (decision.rs:168-195); nodeset types present; tests rbac.rs:106,146,176. |
| 3539 | Security Role Well Known | partial | SecurityAdmin perms tested (rbac.rs:991); ConfigureAdmin defined (preset.rs:66-76) but no test asserts its perm bits (only :308). |
| 3540 | Security Role Well Known Group 2 | partial | Anonymous perms tested (rbac.rs:977); AuthenticatedUser granted (resolver.rs:502) but perm bitset (preset.rs:34) never asserted. |
| 3541 | Security Role Well Known Group 3 | partial | Operator fully tested (rbac.rs:396-498,986); Observer/Engineer/Supervisor exist (preset.rs:39-64) but only node-existence tested. |
| 3542 | Security Role Server Base Eventing | partial | RoleMappingRuleChangedAuditEventType present in generated nodeset (nodeset_16.rs, i=17641); no code ever raises it, untested. |
| 3544 | Base Info ResendData Method | partial | ResendData method core.rs:1209-1220, wired subscription.rs:341-342,757; no test found (searched methods.rs, subscriptions.rs) |
| 3545 | Base Info Namespace Metadata | implemented | Dynamic per-namespace NamespaceMetaData objects diagnostics/node_manager.rs:583-650; e2e test browse.rs:942-967 |
| 3546 | Base Info LocalTime Events | partial | BaseEventType.local_time field (events/event.rs:52) read by get_value(128) but never assigned anywhere in async-opcua-server/src |
| 3547 | Base Info UaBinary File | implemented | UABinaryFileDataType + Description types present in schemas/1.05; type-level exposure via CoreNamespace import. |
| 3549 | Base Info OrderedList Change Notification | gap | Depends on OrderedListType (gap) and NodeVersion Property (gap); both searched, zero server-side hits. |
| 3550 | Base Info StatusResult DataType | implemented | StatusResult in nodeset + generated types/status_result.rs; exposed via CoreNamespace import. |
| 3551 | Base Info UriString | implemented | UriString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3554 | Address Space Base | implemented | Core AddressSpace all NodeClasses address_space/mod.rs (1454 LOC, unit tests) + opcua-nodes crate; e2e browse.rs:144-167 |
| 3560 | Address Space Interfaces | implemented | base_info::add_ordered_object attaches HasInterface from each OrderedListType child to IOrderedObjectType (a byproduct of closing CU 2512); test base_info.rs::ordered_list_children_are_ordered_and_interface_conformant |
| 3562 | Address Space Method Meta Data | gap | Searched 'HasArgumentDescription' usage - only nodeset ref-type node; MethodBuilder args lack description metadata. |
| 3565 | Data Access DataItems | implemented | Satisfied via subtype: AnalogItemType (subtype of DataItemType) tested datachange_overflow.rs:173; BadOutOfRange in write_validation.rs:280 |
| 3566 | Data Access BaseAnalogType | implemented | Satisfied via subtype: AnalogItemType instances w/ EURange tested (datachange_overflow.rs:173, alarms.rs:1494). |
| 3567 | Data Access AnalogItemType | implemented | AnalogItemType instances w/ EURange, exercised in PercentDeadband + A&C limit tests (datachange_overflow.rs:173, alarms.rs:1509). |
| 3568 | Data Access AnalogUnitType | gap | Searched 'AnalogUnitType' - zero instance usage, only a nodeset type node. |
| 3569 | Data Access AnalogUnitRangeType | gap | Searched 'AnalogUnitRangeType' - zero instance usage, only a nodeset type node. |
| 3571 | A & C Alarm Metrics | gap | grep "AlarmMetrics" finds zero hits in async-opcua-server/src or async-opcua-client/src (only in interop node_modules) |
| 3572 | A & E Wrapper Mapping | gap | No OPC-COM/DA/AE wrapper code anywhere (grep for COM/OPC-COM across server+client empty); native Rust stack, no COM interop layer |
| 3574 | Historical Access Aggregates | implemented | backend.rs:85-172 read_processed trait default + middleware.rs:57-106 read_processed_aggregates wiring ReadProcessedDetails end-to-end; test history_tests.rs:341 test_history_read_aggregates (client e2e) |
| 3576 | Aggregate Master Configuration | implemented | standard nodeset ships HistoryServerCapabilities/AggregateConfiguration Object (i=11203) with property children, generated at async-opcua-core-namespace/src/generated/nodeset_9.rs:483-499, imported by every server via core.rs:147 import_node_set(&CoreNamespace,...) |
| 3577 | Aggregate Subscription - Filter | implemented | supported_aggregates() engine.rs:85-89 returns 35 ids; ParsedAggregateFilter monitored_item.rs:139; e2e subscriptions.rs:2276 |
| 3581 | GDS Query Applications | gap | QueryApplications Method not implemented; method.rs:131-135 returns BadServiceUnsupported; no callback registered |
| 3582 | GDS Certificate Manager Pull Model | implemented | Feature 103 (Run 1): fixed the CertificateDirectoryType Pull-model implementation in gds/pull_methods.rs, which previously implemented the wrong (Push-model) methods (GetRejectedList/UpdateCertificate) against fabricated NodeIds -- the same defect class as pre-fix push_methods.rs. Made async-opcua-server::companion pub and wired its import_gds() into a new opt-in gds::register_gds_pull_methods_from_companion(), gated on the (pre-existing, previously dormant) companion-gds feature. Rewrote gds/pull_methods.rs entirely: StartSigningRequest/StartNewKeyPairRequest (new X509::issue_certificate_for_public_key CA-issuance primitive, non-self-signed, for a caller-supplied or freshly generated public key)/FinishRequest/GetCertificateGroups/GetTrustList/GetCertificateStatus, all against dynamically-resolved NodeIds (no hardcoded namespace index). e2e-proven via gds_pull_companion_integration.rs (real client -> Call service -> registered handler dispatch) plus unit tests. This investigation also found and fixed three independent, previously-undiscovered bugs in shared node-manager infrastructure that any companion-spec runtime import would have hit: import_companion_xml seeded a disconnected NamespaceMap::default() instead of the address space's real registered namespaces (risking namespace-index collisions); InMemoryNodeManager::owns_node checked a namespace-set snapshot frozen at construction time, never refreshed for namespaces added by a later runtime import (now RwLock-backed with a refresh_namespaces() call site); and CoreNodeManagerImpl::call_builtin_method, which under the default-on subscriptions-standard feature unconditionally short-circuited before consulting the generic method-callback registry for any method outside the core namespace-0 MethodId set -- silently swallowing any custom method callback registered for a companion-spec or other custom-namespace method. Feature 104 (correction): Run 1's research wrongly concluded CertificateDirectoryType ships no pre-instantiated singleton, and built ~250 lines of custom object-instantiation logic (gds/directory_instance.rs, ObjectBuilder/MethodBuilder) to hand-construct a parallel 'Directory' object with fabricated string NodeIds. The real NodeSet2.xml actually ships a fully pre-instantiated 'Directory' object (source ns=1;i=141, HasTypeDefinition -> CertificateDirectoryType) with real integer NodeIds for every Mandatory method and the CertificateGroups/DefaultApplicationGroup/TrustList subtree -- independently re-verified against the real XML. directory_instance.rs now simply resolves these real NodeIds (mirroring push_methods.rs's fixed-NodeId pattern) instead of constructing a duplicate object; a regression test (does_not_construct_a_duplicate_directory_object) guards against reintroducing the duplicate. RevokeCertificate/GetCertificates/CheckRevocationStatus (Optional) remain unregistered -- their real NodeIds now resolve too, but real semantics (an issuance ledger, real CRL mutation, a revocation-status lookup) are still separate, undone business-logic infrastructure (corrected reason; previously mis-stated as 'no real object to hang callbacks off of'). Feature 105 (Run 2, client-side): async-opcua-client/src/gds/ had the same fabricated-NodeId defect -- fixed via real dynamic discovery, GdsClient::discover(session) resolving the Directory object and RegisterApplication/StartSigningRequest/FinishRequest via the target server's namespace array plus TranslateBrowsePathsToNodeIds (Part 4 section 5.8.4), since every real external GDS deployment assigns its own namespace index. Also corrected GdsCsrClient::certificate_manager_id -> directory_object_id (no separate CertificateManager object exists) and removed a bogus 5th argument start_signing_request sent that isn't part of the real StartSigningRequest signature. Proven end-to-end against this SDK's own server with the GDS namespace at a non-default index; found and fixed two more server-side infrastructure bugs along the way (Server_NamespaceArray never reflecting a runtime-imported namespace, and AddressSpace/type_tree namespace-table divergence causing index collisions) -- see specs/105-gds-pull-client-fix/. |
| 3584 | GDS Key Credential Service Pull Model | gap | Zero non-generated source hits for "KeyCredential" anywhere in repo (grep across all *.rs excluding generated) |
| 3586 | GDS Authorization Service Server | gap | AuthorizationServiceType not implemented; same search as 3182, zero non-generated hits |
| 3605 | Base Info Method Capabilities | partial | MaxNodesPerMethodCall wired node_manager/memory/core.rs:888-892 (const->config->response) but no dedicated test found in tests/*.rs referencing it. |
| 3641 | Base Info Method Argument DataType | implemented | DataTypeId::Argument used building Method args async-opcua-nodes/src/method.rs:92; asserted in address_space/mod.rs:1320. |
| 3642 | Subscription Durable | gap | No "durable" references in async-opcua-server/src; SetSubscriptionDurable NodeId (12749) is a bare generated node, no callback registered |
| 3644 | Base Info SemanticVersionString | implemented | SemanticVersionString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3645 | Security User Token Unencrypted | implemented | SecurityPolicy::None UserTokenPolicy supported (authenticator.rs:397,415); tested authenticator.rs:492-518. |
| 3721 | Security ECC Policy | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3727 | Subscription Basic | implemented | CreateSubscription/Publish/Republish/SetPublishingMode etc implemented (subscriptions/session_subscriptions.rs); tested subscriptions.rs. |
| 3747 | Base Info IsExecutableOn | implemented | IsExecutableOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3748 | Base Info IsExecutingOn | implemented | IsExecutingOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3749 | Base Info Controls | implemented | Controls present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3750 | Base Info Utilizes | implemented | Utilizes present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3751 | Base Info Requires | implemented | Requires present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3752 | Base Info IsPhysicallyConnectedTo | implemented | IsPhysicallyConnectedTo present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3753 | Base Info RepresentsSameEntityAs | implemented | RepresentsSameEntityAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3754 | Base Info RepresentsSameHardwareAs | implemented | RepresentsSameHardwareAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3755 | Base Info RepresentsSameFunctionalityAs | implemented | RepresentsSameFunctionalityAs present schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3756 | Base Info IsHostedBy | implemented | IsHostedBy present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3757 | Base Info HasPhysicalComponent | implemented | HasPhysicalComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3758 | Base Info HasContainedComponent | implemented | HasContainedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3759 | Base Info HasAttachedComponent | implemented | HasAttachedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 3760 | A & C Suppression Group | gap | grep "SuppressionGroup" finds zero hits in async-opcua-server/src or async-opcua-client/src |
| 3761 | A & C InstrumentDiagnostic | gap | InstrumentDiagnosticAlarmType only structural (node_ids.rs:10685); no server instantiation |
| 3762 | A & C SystemDiagnostic | gap | SystemDiagnosticAlarmType only structural (node_ids.rs:10686); no server instantiation |
| 3763 | A & C Auditing | implemented | ConditionAuditEvent (methods.rs) emits AuditConditionCommentEventType on AddComment, closing the former methods.rs:201 TODO; test alarms.rs::add_comment_emits_audit_condition_comment_event |
| 3764 | A & C Dialog Auditing | gap | no AuditConditionEventType for dialog actions anywhere in async-opcua-server/src |
| 3765 | A & C Acknowledge Auditing | gap | no AuditConditionAcknowledgeEventType anywhere in async-opcua-server/src |
| 3766 | A & C Confirm Auditing | gap | no AuditConditionConfirmEventType anywhere in async-opcua-server/src |
| 3767 | A & C Shelving Auditing | gap | no AuditConditionShelvingEventType anywhere in async-opcua-server/src |
| 3768 | A & C Suppression Auditing | gap | no AuditConditionSuppressionEventType anywhere in async-opcua-server/src (suppression itself also unimplemented, 2897/2893) |
| 3770 | A & C Latching Auditing | gap | no latching implemented at all (see 3774), so no latching audit possible |
| 3771 | A & C OutOfService Auditing | implemented | AuditConditionOutOfServiceEventType emitted for RemoveFromService/PlaceInService (methods.rs notify_out_of_service_audit_event); test alarms.rs::remove_from_service_place_in_service_emit_audit_condition_out_of_service_event |
| 3772 | A & C Statemachine Trigger | gap | alarms manual set_suppressed/set_shelved (methods.rs) exist, but no external StateMachine auto-triggers Alarm transitions. |
| 3773 | A & C Statemachine Suppression Trigger | gap | set_suppressed is manual Method-driven (alarms/state_machine.rs:568); no linked-StateMachine auto-suppression trigger found. |
| 3774 | A & C Latched State | gap | no LatchedState variable and no Reset method anywhere in async-opcua-server/src/alarms (grep confirms) |
| 3775 | A & C Alarm Group | gap | grep "AlarmGroup" finds zero hits in async-opcua-server/src or async-opcua-client/src |
| 3776 | A & C GetGroupMemberships | gap | grep "GetGroupMemberships" finds zero hits in async-opcua-server/src or async-opcua-client/src |
| 3777 | A & C Limit BaseLimit | implemented | LimitConfig high/high_high/low/low_low + validate (limit.rs:58-195); test alarms.rs:891 limit_alarm_exclusive_drives_bands |
| 3778 | A & C Limit Severity | implemented | LimitDef.severity per level, severity selection (limit.rs:47-56,740-783); tested alarms.rs:923 (severity 400/700 assertions per band) |
| 3779 | A & C Limit Deadband | implemented | LimitDef.deadband + high/low_exceeded hysteresis (limit.rs:701-715); tested alarms.rs:2040 "deadband cleared" assertion |
| 3786 | Data Access ArrayItem2Type | gap | Searched 'ArrayItemType' subtype usage - zero instance usage (only nodeset type nodes). |
| 3802 | Time Sync - Configure Clock Skew | implemented | ServerConfig::max_acceptable_clock_skew_ns config/server.rs:669,998-1002; tests config/server.rs:375-432 |
| 3808 | Documentation - Core Capacities | implemented | docs/server-capacity-limits.md enumerates every Limits/SubscriptionLimits/OperationalLimits field with its default and configuration method, cross-checked against config/limits.rs's Default impls and the server_conf_limits_match_struct_field_names test |
| 3810 | Base Info TemporaryFileTransferType Sync Read | gap | Searched "GenerateFileForRead"/"TemporaryFileTransferType" across *.rs (excl. generated) — zero implementation hits. |
| 3811 | Base Info TemporaryFileTransferType Async Read | gap | Same search as 3810; no CompletionStateMachine/async read implementation found. |
| 3812 | Base Info TemporaryFileTransferType Sync Write | gap | Searched "GenerateFileForWrite"/"CloseAndCommit" — zero implementation hits outside generated NodeId constants. |
| 3813 | Base Info TemporaryFileTransferType Async Write | gap | Same search as 3812; no async-write CompletionStateMachine implementation found. |
| 3911 | Base Info Server Capabilities Subscriptions | implemented | core.rs get_attribute now wires MaxMonitoredItemsPerSubscription/MaxSubscriptionsPerSession to their SubscriptionLimits config fields, and MaxSubscriptions/MaxMonitoredItems (no server-wide cap exists) report spec-valid 0 per OPC-10000-5 SS6.3.2; tests read.rs::server_capabilities_max_monitored_items_per_subscription_and_max_subscriptions_per_session, ::server_capabilities_server_wide_max_subscriptions_and_max_monitored_items_are_zero |
| 3912 | Base Info Server Capabilities 2 | implemented | core.rs get_attribute wires MaxSessions to Limits.max_sessions (was the only unwired node in this CU per prior audit); test read.rs::server_capabilities_max_sessions_reports_configured_limit |
| 3913 | Subscription Publish Basic | implemented | max_publish_requests_per_subscription=4 (server/src/lib.rs:227); Publish exercised across tests/integration/subscriptions.rs. |
| 3922 | Base Info SemanticChange Bit | implemented | SemanticsChanged bit set monitored_item.rs:1012-1042 via EU-range writes session_subscriptions.rs:1238,1290; tests :1668 |
| 3923 | Session Multiple | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 3928 | Security User Anonymous Server | implemented | Anonymous gated by endpoint.user_token_ids (authenticator.rs:227-238,322-330); e2e session/manager.rs:2289, rbac.rs:238. |
| 3941 | Address Space DataTypeDefinition Attribute | implemented | DataTypeDefinition wired via DataTypeBuilder.data_type_definition; e2e-tested by custom_types.rs test_data_type_tree_builder. |
| 3965 | Address Space User Access Level Base | implemented | user_access_level() computed via RBAC (utils.rs:131-152); 2-role tests utils.rs:1020-1083 show differing AccessLevel per role. |
| 3968 | Auditing Services | partial | audit.rs dispatch_* covers Session/Channel/Cert/Cancel/Write/Method+AddNodes/DeleteNodes; no HistoryUpdate audit despite type existing |
| 3969 | Base Info Model Change | gap | Searched 'BaseModelChangeEventType'/'NodeVersion' - zero server-side hits (only GeneralModelChangeEventType is implemented). |
| 3979 | Auditing UpdateStates | gap | Searched 'AuditUpdateStateEventType' - only generated struct exists; no state machine emits it. |
| 3983 | Base Services Diagnostics | implemented | result.rs:17-58 filter_diagnostic_info masks diag bits; wired attribute.rs/node_management.rs; test per_op_diagnostics.rs |
| 3985 | Session General Service Behaviour | implemented | controller.rs:396 auth-token check, response.rs:207 requestHandle echo, deadline_queue:971-1016 BadTimeout; e2e read.rs:1400-1408 |
| 3994 | Session Sessionless Invocation | gap | SessionlessInvokeRequestType/ResponseType exist only as generated types (unused); rbac/decision.rs:147 TODO admits "sessionless: enforce SessionRequired... not done"; no dispatch path. |
| 3996 | Base Info ReferenceDescription | implemented | base_info::attach_reference_description instantiates ReferenceDescriptionVariableType via HasReferenceDescription, documenting a real Reference's SourceNode/ReferenceType/IsForward/TargetNode (OPC-10000-23 SS5, not Part 3/5); test base_info.rs::reference_description_documents_a_real_reference |
| 4030 | Monitor Complex Event Filter | implemented | OfType evaluated incl. supertypes (evaluate.rs:211-216, fn of_type:358); arity check validation.rs:345; unit test evaluate.rs:1030-1064. |
| 4052 | Base Info TrimmedString | implemented | TrimmedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 4053 | Base Info Locations Object | implemented | Locations object (i=31915, nodeset_16.rs:918-943) confirmed reachable via Browse from ObjectsFolder; test browse.rs::locations_object_is_reachable_from_objects_folder |
| 4054 | Base Info Handle DataType | implemented | Handle DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import. |
| 4055 | Base Info Server Capabilities MaxMonitoredItemsQueueSize | implemented | core.rs get_attribute wires MaxMonitoredItemsQueueSize to SubscriptionLimits.max_monitored_item_queue_size, the same limit already enforced at monitored_item.rs:314; test read.rs::server_capabilities_max_monitored_items_queue_size_reports_configured_limit |
| 4237 | Address Space NonVolatile and Constant | implemented | NonVolatile/Constant bits defined enums.rs:15-19, generic get/set variable.rs:826-838; test write.rs::access_level_ex_non_volatile_and_constant_round_trip |
| 4426 | Base Info Decimal DataType | implemented | Decimal in nodeset + generated types/decimal_data_type.rs; encoded generically as a Structure DataType. |
| 4427 | Base Info Client Events | gap | AuditClientEventType only a generated stub (node_ids.rs:10730, events/generated.rs:148); server-as-client code never raises it |
| 4428 | A & C Silencing Auditing | implemented | AuditConditionSilenceEventType emitted for Silence (methods.rs handle_condition_silence); test alarms.rs::silence_emits_audit_condition_silence_event |
| 4463 | A & C Suppression2 by Operator | implemented | AlarmConditionType_Suppress2/Unsuppress2 Methods registered (methods.rs), routed through the same handlers as Suppress/Unsuppress with apply_optional_comment; test alarms.rs::suppress2_and_place_in_service2_apply_optional_comment |
| 4464 | A & C OutOfService2 | implemented | AlarmConditionType_RemoveFromService2/PlaceInService2 Methods registered (methods.rs) with apply_optional_comment; test alarms.rs::suppress2_and_place_in_service2_apply_optional_comment |
| 4465 | A & C Shelving2 | gap | no TimedShelve2/OneShotShelve2/Unshelve2 MethodId anywhere; only non-"2" shelve methods exist (methods.rs:674-693) |
| 4466 | A & C Dialog2 | partial | Respond2 impl dialog.rs:200-209 + methods.rs:319-336,711-714 registered, but 0 test coverage (grep "Respond2" in test file: 0 hits) |
| 4467 | A & C OutOfService | implemented | OutOfServiceState var+get/set_out_of_service (state_machine.rs) exposed via AlarmConditionType_RemoveFromService/PlaceInService Methods (methods.rs); test alarms.rs::remove_from_service_place_in_service_toggle_out_of_service_state |
| 4500 | Scheduler Scheduling Base | gap | Searched "ScheduleType"/"CalendarEntryType"/"DailyScheduleType" across all *.rs — no matches; only unrelated Part-10 ProgramState (programs/state.rs) exists. |
| 4501 | Scheduler Calendar Base | gap | Searched "CalendarType"/"DateRangeType" — no matches anywhere in codebase. |
| 4502 | Scheduler Scheduling Configuration | gap | No ScheduleType/AddExceptionScheduleElements/RemoveExceptionScheduleElements methods found (Scheduler types entirely absent). |
| 4503 | Scheduler Calendar Configuration | gap | No CalendarType/AddDateListElements/RemoveDateListElements methods found (Scheduler types entirely absent). |
| 4505 | Security User Management Server | gap | Searched "UserManagement" — only unused generated UserManagementType defs (nodeset_18.rs:2083); no instantiated Object/Methods. |
| 4957 | Security User Identity Token Support | implemented | Per-endpoint user_token_ids admin-selects enabled token types (authenticator.rs:318-366, builder.rs:567); broad test coverage. |
| 5207 | Monitor Items 2 | implemented | No per-subscription item cap below 2 found (server/src/config/limits.rs); 2+ Double items trivially exercised in subscriptions.rs. |
| 5208 | Monitor Value Change V2 | partial | IndexRange applied to sample monitored_item.rs:931-940 (Variant::range_of); logic tested via read.rs:794-827, no MonitoredItem-level test |
| 5213 | Auditing Connections | implemented | audit.rs:736 AuditOpenSecureChannelEventType, :763 AuditChannelEventType, :928/:442 Create/ActivateSession; test session_audit.rs:18 |
| 5240 | Base Info Currency | implemented | base_info::create_currency_variable attaches a CurrencyUnit property (CurrencyUnitType) to a monetary DataVariable; test base_info.rs::currency_unit_property_reports_iso4217_fields |
| 5274 | Security Role Server IdentityManagement | implemented | AddIdentity/RemoveIdentity (role_management.rs:330-373), wired for 7 well-known roles; unit tests :682,721,913. |
| 5275 | Security Role Server EndpointManagement | implemented | AddEndpoint/RemoveEndpoint role_management.rs:282-328; unit tests role_management.rs:743,895 (filter add/remove + gating). |
| 5276 | Security Role Server ApplicationManagement | implemented | AddApplication/RemoveApplication role_management.rs:234-280; unit tests role_management.rs:743,810. |
| 5277 | Security Role TrustedApplication | gap | TrustedApplication absent from WellKnownRole enum (mod.rs:28-45); rules.rs:155 explicitly rejects it as unsupported. |
| 5292 | KeyCredential ProfileURI - UA transport with UserName | gap | No KeyCredential machinery exists at all (see 3584); generic UserName/Password auth exists but is unrelated to this CU |
| 5293 | KeyCredential Authentication Mechanism Support | gap | No KeyCredential authentication-mechanism support of any kind implemented (depends on 3584/5292/5301/5302, all gaps) |
| 5301 | KeyCredential ProfileURI - AMQP SASL Plain | gap | AMQP SASL PLAIN KeyCredential profile: no AMQP transport or KeyCredential code found in repo |
| 5302 | KeyCredential ProfileURI - MQTT UserName | gap | MQTT UserName KeyCredential profile: no KeyCredential code found (MQTT PubSub transport exists but unrelated) |
| 5303 | Push Model for KeyCredential Service | gap | Push Model for KeyCredential Service: zero KeyCredential code found anywhere (see 3584) |
| 5505 | Time Sync - UA based support | implemented | UaHeaderTimeSyncSource polls ResponseHeader.timestamp (time_sync_ua.rs:52-80), configurable builder.rs:258-262; test time_sync.rs:33 |
| 5510 | A & C Enabled TransitionTime | implemented | EnabledState.TransitionTime written by set_enabled (state_machine.rs); test alarms.rs::enabled_state_transition_time_updates_on_enable_disable |
| 5511 | A & C Enabled EffectiveTransitionTime  | gap | EnabledState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5512 | A & C Enabled EffectiveDisplayName  | gap | EnabledState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5513 | A & C Active TransitionTime  | implemented | ActiveState.TransitionTime written by set_active (state_machine.rs); test alarms.rs::active_state_transition_time_and_effective_display_name_update_on_activation |
| 5514 | A & C Active EffectiveTransitionTime  | implemented | ActiveState.EffectiveTransitionTime written by recompute_effective_state (state_machine.rs); same test as 5513, plus alarms.rs::shelving_updates_effective_transition_time_without_changing_active_state_transition_time |
| 5515 | A & C Active EffectiveDisplayName  | implemented | ActiveState.EffectiveDisplayName written by recompute_effective_state (state_machine.rs); same tests as 5514 |
| 5516 | A & C Acknowledge TransitionTime | implemented | AckedState.TransitionTime written by set_acked (state_machine.rs); test alarms.rs::acked_and_confirmed_state_transition_time_update_on_acknowledge_confirm |
| 5517 | A & C Acknowledge EffectiveTransitionTime  | gap | AckedState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5518 | A & C Acknowledge EffectiveDisplayName  | gap | AckedState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5519 | A & C Confirm TransitionTime  | implemented | ConfirmedState.TransitionTime written by set_confirmed (state_machine.rs); test alarms.rs::acked_and_confirmed_state_transition_time_update_on_acknowledge_confirm |
| 5520 | A & C Confirm EffectiveTransitionTime | gap | ConfirmedState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5521 | A & C Confirm EffectiveDisplayName | gap | ConfirmedState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5522 | A & C Suppression TransitionTime | implemented | SuppressedState.TransitionTime written by set_suppressed (state_machine.rs); exercised via alarms.rs::suppress_unsuppress_methods_toggle_suppressed_state (095 US2 Method), no dedicated TransitionTime-value assertion yet |
| 5523 | A & C Suppression EffectiveTransitionTime | gap | SuppressedState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5524 | A & C Suppression EffectiveDisplayName | gap | SuppressedState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5525 | A & C OutOfService TransitionTime | implemented | OutOfServiceState.TransitionTime written by set_out_of_service (state_machine.rs); exercised via alarms.rs::remove_from_service_place_in_service_toggle_out_of_service_state (095 US2 Method), no dedicated TransitionTime-value assertion yet |
| 5526 | A & C OutOfService EffectiveTransitionTime | gap | OutOfServiceState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5527 | A & C OutOfService EffectiveDisplayName | gap | OutOfServiceState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope) |
| 5528 | A & C Silence TransitionTime | implemented | SilenceState.TransitionTime written by set_silenced (state_machine.rs, 095 US2); exercised via alarms.rs::silence_method_toggles_silence_state_and_is_idempotent, no dedicated TransitionTime-value assertion yet |
| 5529 | A & C Silence EffectiveTransitionTime | gap | no SilenceState variable exists at all; no EffectiveTransitionTime property possible |
| 5530 | A & C Silence EffectiveDisplayName | gap | no SilenceState variable exists at all; no EffectiveDisplayName property possible |
| 5531 | A & C Latched TransitionTime | gap | no LatchedState variable exists at all (3774 also gap), so no TransitionTime property possible |
| 5532 | A & C Latched EffectiveTransitionTime | gap | no LatchedState variable exists at all; no EffectiveTransitionTime property possible |
| 5533 | A & C Latched EffectiveDisplayName | gap | no LatchedState variable exists at all; no EffectiveDisplayName property possible |
| 5534 | A & C Non-Exclusive HighHigh TransitionTime | implemented | HighHighState.TransitionTime written by write_non_exclusive_level, only on actual transition (limit.rs); test alarms.rs::limit_state_transition_time_updates_on_threshold_crossing covers the exclusive variant, non-exclusive covered by same write path |
| 5535 | A & C Non-Exclusive High TransitionTime | implemented | HighState.TransitionTime written by write_non_exclusive_level (limit.rs) |
| 5536 | A & C Non-Exclusive Low TransitionTime | implemented | LowState.TransitionTime written by write_non_exclusive_level (limit.rs) |
| 5537 | A & C Non-Exclusive LowLow TransitionTime | implemented | LowLowState.TransitionTime written by write_non_exclusive_level (limit.rs) |
| 5538 | A & C Non-Exclusive HighHigh EffectiveTransitionTime | gap | zero hits for "EffectiveTransitionTime" on HighHighState anywhere in repo src |
| 5539 | A & C Non-Exclusive High EffectiveTransitionTime | gap | zero hits for "EffectiveTransitionTime" on HighState anywhere in repo src |
| 5540 | A & C Non-Exclusive Low EffectiveTransitionTime | gap | zero hits for "EffectiveTransitionTime" on LowState anywhere in repo src |
| 5541 | A & C Non-Exclusive LowLow EffectiveTransitionTime | gap | zero hits for "EffectiveTransitionTime" on LowLowState anywhere in repo src |
| 5542 | A & C Non-Exclusive HighHigh EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on HighHighState anywhere in repo src |
| 5543 | A & C Non-Exclusive High EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on HighState anywhere in repo src |
| 5544 | A & C Non-Exclusive Low EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on LowState anywhere in repo src |
| 5545 | A & C Non-Exclusive LowLow EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on LowLowState anywhere in repo src |
| 5546 | A & C Dialog TransitionTime | gap | zero hits for "TransitionTime" on DialogState anywhere in dialog.rs or repo src |
| 5547 | A & C Dialog EffectiveTransitionTime | gap | zero hits for "EffectiveTransitionTime" on DialogState anywhere in repo src |
| 5548 | A & C Dialog EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on DialogState anywhere in repo src |
| 5549 | A & C Shelving LastTransition | implemented | ShelvingState.CurrentState.TransitionTime (the LastTransition equivalent) written by set_shelving_state (state_machine.rs); test alarms.rs::shelving_updates_effective_transition_time_without_changing_active_state_transition_time |
| 5550 | A & C Shelving UnshelvedToTimedShelved TransitionTime | gap | no per-transition TransitionTime tracking in ShelvingState machinery (state_machine.rs:589-624 stores only current state) |
| 5551 | A & C Shelving TimedShelvedToUnshelved TransitionTime | gap | no per-transition TransitionTime tracking in ShelvingState machinery |
| 5552 | A & C Shelving TimedShelvedToOneShotShelved TransitionTime | gap | no per-transition TransitionTime tracking in ShelvingState machinery |
| 5553 | A & C Shelving UnshelvedToOneShotShelved TransitionTime | gap | no per-transition TransitionTime tracking in ShelvingState machinery |
| 5554 | A & C Shelving OneShotShelvedToUnshelved TransitionTime | gap | no per-transition TransitionTime tracking in ShelvingState machinery |
| 5555 | A & C Shelving OneShotShelvedToTimedShelved TransitionTime | gap | no per-transition TransitionTime tracking in ShelvingState machinery |
| 5556 | A & C Shelving Unshelved EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on Unshelved sub-state anywhere in repo src |
| 5557 | A & C Shelving TimedShelved EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on TimedShelved sub-state anywhere in repo src |
| 5558 | A & C Shelving OneShotShelved EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on OneShotShelved sub-state anywhere in repo src |
| 5559 | A & C Exclusive Limit LastTransition | implemented | LimitState.CurrentState.TransitionTime (the LastTransition equivalent) written by write_exclusive_limit_state, only on actual level change (limit.rs); test alarms.rs::limit_state_transition_time_updates_on_threshold_crossing |
| 5560 | A & C Exclusive Limit LowToLowLow TransitionTime | gap | no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType (limit.rs has no such fields) |
| 5561 | A & C Exclusive Limit LowLowToLow TransitionTime | gap | no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType |
| 5562 | A & C Exclusive Limit HighToHighHigh TransitionTime | gap | no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType |
| 5563 | A & C Exclusive Limit HighHighToHigh TransitionTime | gap | no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType |
| 5564 | A & C Exclusive Limit LowLow EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on LowLow LimitState sub-state anywhere in limit.rs |
| 5565 | A & C Exclusive Limit Low EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on Low LimitState sub-state anywhere in limit.rs |
| 5566 | A & C Exclusive Limit High EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on High LimitState sub-state anywhere in limit.rs |
| 5567 | A & C Exclusive Limit HighHigh EffectiveDisplayName | gap | zero hits for "EffectiveDisplayName" on HighHigh LimitState sub-state anywhere in limit.rs |
| 5578 | Base Info Progress Events | gap | ProgressEventType only a generated struct generated.rs:651, used only as an arbitrary test fixture subscriptions.rs:1739; never raised |
| 5592 | Missing from normalized CU list | source-issue | Referenced by closure but absent from conformance_units. |
| 5791 | Base Info TemporaryFileTransferType Base | gap | No TemporaryFileTransferType or FileTransferStateMachineType instance/implementation found outside generated NodeId enum constants. |
| 5793 | Time Sync - Support | implemented | OsClockSource (time_sync.rs:112-124) + UA-based source satisfy facet; docs/time-synchronization.md:9-17; tests time_sync.rs:11-22 |
| 5795 | Documentation - Durable Subscription Capacity | gap | No durable-subscription capacity doc found; feature itself absent (CU 3642); only stale CU-COVERAGE.md:962 "needs-proof" placeholder |
| 5796 | Documentation - On-line | implemented | README.md docs.rs/crates.io badges (README.md:3,5,58) + docs/ folder shipped in repo, accessible from GitHub/docs.rs. |
| 5797 | Documentation - Trouble Shooting Guide | gap | Searched docs/ and root *.md for "troubleshoot"/"FAQ"/"common issue" — no troubleshooting or diagnostics guide found. |
| 5801 | Base Info Type Information | implemented | Not a standalone feature -- this server always imports the complete standard 1.05 nodeset (every ObjectType/VariableType/ReferenceType/DataType, their supertypes, and Encoding Objects for Structured DataTypes are generated nodeset nodes), so any instance referencing a standard TypeDefinition automatically satisfies this CU. Demonstrated cumulatively across every 'instantiate VariableType X' CU this project closes (e.g. feature 097's base_info.rs OrderedListType/SelectionListType/OptionSetType/ReferenceDescriptionVariableType, feature 100's data_access.rs TwoStateDiscreteType/MultiStateDiscreteType/MultiStateValueDiscreteType/ArrayItemType family) -- each instantiation test proves its referenced TypeDefinition node resolves in the AddressSpace, not just an isolated e2e check |
| 5806 | Historical Access Read Raw | implemented | data_history.rs:131-180 read_raw_modified; tests history_tests.rs:245 test_history_read_100k_page_reads, :299 test_history_read_reversed_intervals; hda.rs:320 e2e_inmemory_update_then_read_roundtrip |
| 5807 | A & C Non-Exclusive Limit | implemented | NonExclusiveLimitAlarmType via create_non_exclusive_in_address_space limit.rs:439-503; tested alarms.rs:971,2400 |
| 5808 | A & C Exclusive Limit | implemented | ExclusiveLimitAlarmType via create_exclusive_in_address_space limit.rs:357-436; tested alarms.rs:891,1033,1499 |
| 5809 | Security User JWT IssuedToken 2 | implemented | LocalOAuth2Validator does real RS256 sig verify (jwt_validator.rs:117-195); tested security_tests.rs:1894-2540. |
| 5812 | Attribute Historical Update  | implemented | history.rs:47-113 HistoryUpdateDetails dispatch (UpdateData/UpdateStructureData/UpdateEvent/Delete*); simple.rs:747-865 history_update; InsertData/ReplaceData/UpdateData/DeleteAtTime all independently tested (see CUs 2383/2264/3053/3081) |
| 5813 | Attribute Historical Read  | implemented | history.rs:26-45 HistoryReadDetails (RawModified/AtTime/Processed/Events/Annotations); attribute.rs:194-229 dispatch; ReadRaw/ReadProcessed/ReadEvents/ReadAnnotations all functional and tested (ReadAtTime is the one unsupported sub-mode, see CU 3020, but the "at least one" bar is met many times over) |
| 5814 | Security - No Application Authentication | implemented | Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence. |
| 5868 | Base Info Portable IDs | implemented | PortableQualifiedName/PortableNodeId present schemas/1.05 + generated types/portable_node_id.rs; exposed via CoreNamespace import. |
| 5875 | Base Info State Machine DescriptionNodeIdDataType | gap | 'ContinuousOptions'/'DescriptionNodeIdDataType' absent from schemas/1.05 and schemas/1.0.4 nodesets and the whole codebase. |

