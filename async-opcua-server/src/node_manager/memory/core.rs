use std::sync::Arc;
#[cfg(feature = "subscriptions")]
use std::time::Duration;
#[cfg(feature = "diagnostics")]
use std::time::Instant;

use async_trait::async_trait;
#[cfg(feature = "diagnostics")]
use chrono::Duration as ChronoDuration;
use chrono::Offset;
use hashbrown::HashMap;

#[cfg(all(feature = "method-call", feature = "subscriptions-standard"))]
use crate::load_method_args;
#[cfg(feature = "method-call")]
use crate::node_manager::MethodCall;
#[cfg(all(feature = "diagnostics", feature = "subscriptions"))]
use crate::subscriptions::SessionSubscriptionDiagnosticsSummary;
use crate::{
    address_space::{
        compute_user_role_permissions, read_node_value, AddressSpace, CoreNamespace, NodeType,
    },
    node_manager::{
        NamespaceMetadata, NodeManagersRef, ParsedReadValueId, ParsedWriteValue, RequestContext,
        ServerContext, WriteNode,
    },
    ServerCapabilities, ServerStatusWrapper,
};
#[cfg(not(feature = "history"))]
use crate::config::HistoryServerCapabilities;
#[cfg(feature = "diagnostics")]
use crate::{
    config::ANONYMOUS_USER_TOKEN_ID,
    identity_token::IdentityToken,
    session::{
        instance::Session,
        manager::{locale_ids_for_session, SessionManager},
    },
};
#[cfg(feature = "subscriptions")]
use crate::{
    node_manager::{MonitoredItemRef, MonitoredItemUpdateRef, SyncSampler},
    subscriptions::CreateMonitoredItem,
};
use opcua_core::sync::RwLock;
#[cfg(any(feature = "diagnostics", feature = "method-call"))]
use opcua_core::trace_read_lock;
#[cfg(feature = "method-call")]
use opcua_core::trace_write_lock;
#[cfg(feature = "subscriptions")]
use opcua_types::MonitoringMode;
#[cfg(feature = "diagnostics")]
use opcua_types::{
    profiles, AttributeId, ByteString, SessionDiagnosticsDataType,
    SessionSecurityDiagnosticsDataType, UAString,
};
use opcua_types::{
    DataValue, DateTime, ExtensionObject, IdType, Identifier, NodeId, NumericRange, ObjectId,
    ReferenceTypeId, RolePermissionType, StatusCode, TimeZoneDataType, TimestampsToReturn,
    VariableId, Variant,
};
#[cfg(all(feature = "method-call", feature = "subscriptions-standard"))]
use opcua_types::{MethodId, VariantScalarTypeId, VariantTypeId};

use super::{InMemoryNodeManager, InMemoryNodeManagerImpl, InMemoryNodeManagerImplBuilder};

/// Zeroed-out history capabilities used when the `history` gate is off (FR-004).
#[cfg(not(feature = "history"))]
const HISTORY_OFF: HistoryServerCapabilities = HistoryServerCapabilities {
    access_history_data: false,
    access_history_events: false,
    delete_at_time: false,
    delete_event: false,
    delete_raw: false,
    insert_annotation: false,
    insert_data: false,
    insert_event: false,
    max_return_data_values: 0,
    max_return_event_values: 0,
    replace_data: false,
    replace_event: false,
    server_timestamp_supported: false,
    update_data: false,
    update_event: false,
    aggregates: Vec::new(),
};

#[cfg(feature = "method-call")]
type MethodWithContextCB = Arc<
    dyn Fn(&RequestContext, &NodeId, &[Variant]) -> Result<Vec<Variant>, StatusCode>
        + Send
        + Sync
        + 'static,
>;

enum PreparedCoreReadValue {
    Value(DataValue),
    #[cfg(feature = "diagnostics")]
    DiagnosticsArray {
        kind: DiagnosticsArrayKind,
        index_range: NumericRange,
    },
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Copy)]
enum DiagnosticsArrayKind {
    Subscription,
    Session,
    SessionSecurity,
}

/// Node manager impl for the core namespace.
pub struct CoreNodeManagerImpl {
    #[cfg(feature = "subscriptions")]
    sampler: SyncSampler,
    node_managers: NodeManagersRef,
    #[cfg(feature = "diagnostics")]
    session_manager: Arc<RwLock<SessionManager>>,
    status: Arc<ServerStatusWrapper>,
    #[cfg(feature = "method-call")]
    method_with_context_cbs: Arc<RwLock<std::collections::HashMap<NodeId, MethodWithContextCB>>>,
}

#[cfg(all(feature = "diagnostics", feature = "subscriptions"))]
type CoreSessionSubscriptionDiagnosticsSummary = SessionSubscriptionDiagnosticsSummary;

#[cfg(all(feature = "diagnostics", not(feature = "subscriptions")))]
#[derive(Debug, Clone, Copy, Default)]
struct CoreSessionSubscriptionDiagnosticsSummary {
    subscription_count: u32,
    monitored_item_count: u32,
}

/// Node manager for the core namespace.
pub type CoreNodeManager = InMemoryNodeManager<CoreNodeManagerImpl>;

/// Builder for the [CoreNodeManager].
pub struct CoreNodeManagerBuilder;

impl InMemoryNodeManagerImplBuilder for CoreNodeManagerBuilder {
    type Impl = CoreNodeManagerImpl;

    fn build(self, context: ServerContext, address_space: &mut AddressSpace) -> Self::Impl {
        {
            let mut type_tree = context.type_tree.write();
            address_space.import_node_set(&CoreNamespace, type_tree.namespaces_mut());
            context.info.publish_type_tree_snapshot(&type_tree);
        }

        CoreNodeManagerImpl::new(
            context.node_managers.clone(),
            #[cfg(feature = "diagnostics")]
            context.session_manager.clone(),
            context.status.clone(),
        )
    }
}

/*
The core node manager serves as an example for how you can create a simple
node manager based on the in-memory node manager.

In this case the data is largely static, so all we need to really
implement is Read, leaving the responsibility for notifying any subscriptions
of changes to these to the one doing the modifying.
*/

#[async_trait]
impl InMemoryNodeManagerImpl for CoreNodeManagerImpl {
    async fn init(&self, address_space: &mut AddressSpace, context: ServerContext) {
        self.set_core_namespace_metadata_defaults(address_space, &context);
        self.add_aggregates(address_space, &context.info.capabilities);
        #[cfg(feature = "subscriptions")]
        {
            let interval = context
                .info
                .config
                .limits
                .subscriptions
                .min_sampling_interval_ms
                .floor() as u64;
            let sampler_interval = if interval > 0 { interval } else { 100 };
            self.sampler.run(
                Duration::from_millis(sampler_interval),
                context.subscriptions.clone(),
            );
        }
    }

    fn namespaces(&self) -> Vec<NamespaceMetadata> {
        vec![NamespaceMetadata {
            // If necessary we could read this from the address space here,
            // but I don't think we need to, the diagnostics node manager
            // has an exception for the base namespace.
            is_namespace_subset: Some(false),
            namespace_publication_date: None,
            namespace_version: None,
            namespace_uri: "http://opcfoundation.org/UA/".to_owned(),
            static_node_id_types: Some(vec![IdType::Numeric]),
            namespace_index: 0,
            ..Default::default()
        }]
    }

    fn name(&self) -> &str {
        "core"
    }

    #[cfg(feature = "method-call")]
    fn accepts_method_without_object_component(&self, method_id: &NodeId) -> bool {
        trace_read_lock!(self.method_with_context_cbs).contains_key(method_id)
    }

    async fn read_values(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes: &[&ParsedReadValueId],
        max_age: f64,
        timestamps_to_return: TimestampsToReturn,
    ) -> Vec<DataValue> {
        let reads: Vec<_> = {
            let address_space = address_space.read();

            nodes
                .iter()
                .map(|n| {
                    self.prepare_read_node_value(
                        context,
                        &address_space,
                        n,
                        max_age,
                        timestamps_to_return,
                    )
                })
                .collect()
        };

        let mut values = Vec::with_capacity(reads.len());
        for read in reads {
            values.push(match read {
                PreparedCoreReadValue::Value(value) => value,
                #[cfg(feature = "diagnostics")]
                PreparedCoreReadValue::DiagnosticsArray { kind, index_range } => {
                    self.read_diagnostics_array(context, kind, &index_range)
                        .await
                }
            });
        }

        values
    }

    async fn write(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        nodes_to_write: &mut [&mut WriteNode],
    ) -> Result<(), StatusCode> {
        for write in nodes_to_write {
            let status = self.write_server_value(context, address_space, write.value());
            write.set_status(status);
        }

        Ok(())
    }

    #[cfg(feature = "subscriptions")]
    async fn create_value_monitored_items(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        items: &mut [&mut &mut CreateMonitoredItem],
    ) {
        let to_read: Vec<_> = items.iter().map(|r| r.item_to_monitor()).collect();
        let values = self
            .read_values(
                context,
                address_space,
                &to_read,
                0.0,
                TimestampsToReturn::Both,
            )
            .await;

        for (value, node) in values.into_iter().zip(items.iter_mut()) {
            if value.status() == StatusCode::BadUserAccessDenied {
                node.set_status(StatusCode::BadUserAccessDenied);
                continue;
            }
            if value.status() != StatusCode::BadAttributeIdInvalid {
                node.set_initial_value(value);
            }
            node.set_status(StatusCode::Good);

            if let Some(var_id) = self.status.get_managed_id(&node.item_to_monitor().node_id) {
                self.status.subscribe_to_component(
                    var_id,
                    node.monitoring_mode(),
                    node.handle(),
                    Duration::from_millis(node.sampling_interval() as u64),
                );
                continue;
            }
            #[cfg(feature = "diagnostics")]
            if self.is_internal_sampled(&node.item_to_monitor().node_id, context) {
                if let Err(e) = self.add_internal_sampler(node, context) {
                    node.set_status(e);
                }
            }
        }
    }

    #[cfg(feature = "method-call")]
    async fn call(
        &self,
        context: &RequestContext,
        _address_space: &RwLock<AddressSpace>,
        methods_to_call: &mut [&mut &mut MethodCall],
    ) -> Result<(), StatusCode> {
        for method in methods_to_call {
            if let Err(e) = self.call_builtin_method(method, context).await {
                method.set_status(e);
            }
        }
        Ok(())
    }

    #[cfg(feature = "subscriptions")]
    async fn set_monitoring_mode(
        &self,
        _context: &RequestContext,
        mode: MonitoringMode,
        items: &[&MonitoredItemRef],
    ) {
        for item in items {
            if self.status.get_managed_id(item.node_id()).is_some() {
                self.status.sampler().set_sampler_mode(
                    item.node_id(),
                    item.attribute(),
                    item.handle(),
                    mode,
                );
            }
        }
    }

    #[cfg(feature = "subscriptions")]
    async fn modify_monitored_items(
        &self,
        _context: &RequestContext,
        items: &[&MonitoredItemUpdateRef],
    ) {
        for item in items {
            if self.status.get_managed_id(item.node_id()).is_some() {
                self.status.sampler().update_sampler(
                    item.node_id(),
                    item.attribute(),
                    item.handle(),
                    Duration::from_millis(item.update().revised_sampling_interval as u64),
                );
            }
        }
    }

    #[cfg(feature = "subscriptions")]
    async fn delete_monitored_items(&self, _context: &RequestContext, items: &[&MonitoredItemRef]) {
        for item in items {
            if self.status.get_managed_id(item.node_id()).is_some() {
                self.status.sampler().remove_sampler(
                    item.node_id(),
                    item.attribute(),
                    item.handle(),
                );
            }
        }
    }
}

impl CoreNodeManagerImpl {
    pub(super) fn new(
        node_managers: NodeManagersRef,
        #[cfg(feature = "diagnostics")] session_manager: Arc<RwLock<SessionManager>>,
        status: Arc<ServerStatusWrapper>,
    ) -> Self {
        Self {
            #[cfg(feature = "subscriptions")]
            sampler: SyncSampler::new(),
            status,
            node_managers,
            #[cfg(feature = "diagnostics")]
            session_manager,
            #[cfg(feature = "method-call")]
            method_with_context_cbs: Default::default(),
        }
    }

    fn set_core_namespace_metadata_defaults(
        &self,
        address_space: &mut AddressSpace,
        context: &ServerContext,
    ) {
        let defaults = &context.info.namespace_defaults;

        if let Some(role_permissions) = defaults.role_permissions(0) {
            Self::set_core_namespace_metadata_value(
                address_space,
                VariableId::OPCUANamespaceMetadata_DefaultRolePermissions,
                role_permissions_variant(role_permissions),
            );
            Self::set_core_namespace_metadata_value(
                address_space,
                VariableId::OPCUANamespaceMetadata_DefaultUserRolePermissions,
                role_permissions_variant(role_permissions),
            );
        }

        if let Some(access_restrictions) = defaults.access_restrictions(0) {
            Self::set_core_namespace_metadata_value(
                address_space,
                VariableId::OPCUANamespaceMetadata_DefaultAccessRestrictions,
                access_restrictions.bits().into(),
            );
        }
    }

    fn set_core_namespace_metadata_value(
        address_space: &mut AddressSpace,
        variable_id: VariableId,
        value: Variant,
    ) {
        let node_id: NodeId = variable_id.into();
        let Some(mut node) = address_space.find_mut(&node_id) else {
            // The generated core NamespaceMetadata hierarchy may be absent in custom builds; leave
            // it unset rather than fabricating the object and property nodes here.
            return;
        };

        if let NodeType::Variable(variable) = &mut *node {
            variable.set_data_value(DataValue::new_now(value));
        }
    }

    fn prepare_read_node_value(
        &self,
        context: &RequestContext,
        address_space: &AddressSpace,
        node_to_read: &ParsedReadValueId,
        max_age: f64,
        timestamps_to_return: TimestampsToReturn,
    ) -> PreparedCoreReadValue {
        let mut result_value = DataValue::null();
        // Check that the read is permitted.
        let node = match address_space.validate_node_read(context, node_to_read) {
            Ok(n) => n,
            Err(e) => {
                result_value.status = Some(e);
                return PreparedCoreReadValue::Value(result_value);
            }
        };

        #[cfg(feature = "diagnostics")]
        {
            if let Some(kind) = self.diagnostics_array_kind(&node_to_read.node_id) {
                return PreparedCoreReadValue::DiagnosticsArray {
                    kind,
                    index_range: node_to_read.index_range.clone(),
                };
            }
        }

        // Try to read a special value, that is obtained from somewhere else.
        // A custom node manager might read this from some device, or get them
        // in some other way.

        // In this case, the values are largely read from configuration.
        PreparedCoreReadValue::Value(
            if let Some(v) = self.read_server_value(context, node_to_read) {
                v
            } else {
                // If it can't be found, read it from the node hierarchy.
                read_node_value(&node, context, node_to_read, max_age, timestamps_to_return)
            },
        )
    }

    fn get_variable_id(&self, node: &NodeId) -> Option<VariableId> {
        if node.namespace != 0 {
            return None;
        }
        let Identifier::Numeric(identifier) = node.identifier else {
            return None;
        };
        VariableId::try_from(identifier).ok()
    }

    #[cfg(feature = "diagnostics")]
    fn diagnostics_array_kind(&self, node: &NodeId) -> Option<DiagnosticsArrayKind> {
        match self.get_variable_id(node)? {
            VariableId::Server_ServerDiagnostics_SubscriptionDiagnosticsArray => {
                Some(DiagnosticsArrayKind::Subscription)
            }
            VariableId::Server_ServerDiagnostics_SessionsDiagnosticsSummary_SessionDiagnosticsArray => {
                Some(DiagnosticsArrayKind::Session)
            }
            VariableId::Server_ServerDiagnostics_SessionsDiagnosticsSummary_SessionSecurityDiagnosticsArray => {
                Some(DiagnosticsArrayKind::SessionSecurity)
            }
            _ => None,
        }
    }

    #[cfg(feature = "diagnostics")]
    async fn read_diagnostics_array(
        &self,
        context: &RequestContext,
        kind: DiagnosticsArrayKind,
        index_range: &NumericRange,
    ) -> DataValue {
        match kind {
            DiagnosticsArrayKind::Subscription => {
                self.read_subscription_diagnostics_array(context, index_range)
                    .await
            }
            DiagnosticsArrayKind::Session => {
                self.read_session_diagnostics_array(context, index_range)
                    .await
            }
            DiagnosticsArrayKind::SessionSecurity => {
                self.read_session_security_diagnostics_array(context, index_range)
                    .await
            }
        }
    }

    #[cfg(feature = "diagnostics")]
    async fn read_subscription_diagnostics_array(
        &self,
        context: &RequestContext,
        index_range: &NumericRange,
    ) -> DataValue {
        let perms = context.info.authenticator.core_permissions(&context.token);
        if !perms.read_diagnostics {
            return DataValue::new_now_status(Variant::Empty, StatusCode::BadUserAccessDenied);
        }

        let rows = if context.info.diagnostics.enabled() {
            #[cfg(feature = "subscriptions")]
            {
                context
                    .subscriptions
                    .subscription_diagnostics()
                    .await
                    .into_iter()
                    .map(ExtensionObject::from_message)
                    .collect()
            }
            #[cfg(not(feature = "subscriptions"))]
            {
                Vec::<ExtensionObject>::new()
            }
        } else {
            Vec::new()
        };

        Self::extension_object_array_data_value(rows, index_range)
    }

    #[cfg(feature = "diagnostics")]
    async fn read_session_diagnostics_array(
        &self,
        context: &RequestContext,
        index_range: &NumericRange,
    ) -> DataValue {
        let perms = context.info.authenticator.core_permissions(&context.token);
        if !perms.read_diagnostics {
            return DataValue::new_now_status(Variant::Empty, StatusCode::BadUserAccessDenied);
        }

        let rows = if context.info.diagnostics.enabled() {
            self.session_diagnostics_rows(context)
                .await
                .into_iter()
                .map(ExtensionObject::from_message)
                .collect()
        } else {
            Vec::new()
        };

        Self::extension_object_array_data_value(rows, index_range)
    }

    #[cfg(feature = "diagnostics")]
    async fn read_session_security_diagnostics_array(
        &self,
        context: &RequestContext,
        index_range: &NumericRange,
    ) -> DataValue {
        let perms = context.info.authenticator.core_permissions(&context.token);
        if !perms.read_diagnostics || !perms.read_security_diagnostics {
            return DataValue::new_now_status(Variant::Empty, StatusCode::BadUserAccessDenied);
        }

        let rows = if context.info.diagnostics.enabled() {
            self.session_security_diagnostics_rows()
                .into_iter()
                .map(ExtensionObject::from_message)
                .collect()
        } else {
            Vec::new()
        };

        Self::extension_object_array_data_value(rows, index_range)
    }

    #[cfg(feature = "diagnostics")]
    fn extension_object_array_data_value(
        rows: Vec<ExtensionObject>,
        index_range: &NumericRange,
    ) -> DataValue {
        let mut value: Variant = rows.into();
        if !matches!(index_range, NumericRange::None) {
            match value.range_of(index_range) {
                Ok(ranged_value) => value = ranged_value,
                Err(e) => {
                    return DataValue {
                        value: None,
                        status: Some(e),
                        ..Default::default()
                    };
                }
            }
        }

        DataValue::new_now(value)
    }

    #[cfg(feature = "diagnostics")]
    async fn session_diagnostics_rows(
        &self,
        context: &RequestContext,
    ) -> Vec<SessionDiagnosticsDataType> {
        #[cfg(feature = "subscriptions")]
        let subscription_summaries = context.subscriptions.session_diagnostics_summaries().await;
        let sessions = self.snapshot_sessions();
        let mut rows = Vec::with_capacity(sessions.len());

        for session in sessions {
            let session = trace_read_lock!(session);
            #[cfg(feature = "subscriptions")]
            let subscription_summary = subscription_summaries
                .get(&session.session_id_numeric())
                .copied()
                .unwrap_or_default();
            #[cfg(not(feature = "subscriptions"))]
            let subscription_summary = CoreSessionSubscriptionDiagnosticsSummary::default();
            rows.push(Self::session_diagnostics_row(
                context,
                &session,
                subscription_summary,
            ));
        }

        rows
    }

    #[cfg(feature = "diagnostics")]
    fn session_security_diagnostics_rows(&self) -> Vec<SessionSecurityDiagnosticsDataType> {
        let sessions = self.snapshot_sessions();
        let mut rows = Vec::with_capacity(sessions.len());

        for session in sessions {
            let session = trace_read_lock!(session);
            rows.push(Self::session_security_diagnostics_row(&session));
        }

        rows
    }

    #[cfg(feature = "diagnostics")]
    fn snapshot_sessions(&self) -> Vec<Arc<RwLock<Session>>> {
        let session_manager = trace_read_lock!(self.session_manager);
        session_manager.snapshot_sessions()
    }

    #[cfg(feature = "diagnostics")]
    fn session_diagnostics_row(
        context: &RequestContext,
        session: &Session,
        subscription_summary: CoreSessionSubscriptionDiagnosticsSummary,
    ) -> SessionDiagnosticsDataType {
        SessionDiagnosticsDataType {
            session_id: session.session_id().clone(),
            session_name: UAString::from(session.session_name()),
            client_description: session.application_description().clone(),
            server_uri: context.info.application_uri.clone(),
            endpoint_url: session.endpoint_url().clone(),
            locale_ids: locale_ids_for_session(&context.info, session.session_id_numeric()),
            actual_session_timeout: Self::actual_session_timeout_ms(session),
            max_response_message_size: session.max_response_message_size(),
            client_connection_time: Self::instant_to_date_time(session.created_at()),
            client_last_contact_time: DateTime::now(),
            current_subscriptions_count: subscription_summary.subscription_count,
            current_monitored_items_count: subscription_summary.monitored_item_count,
            ..Default::default()
        }
    }

    #[cfg(feature = "diagnostics")]
    fn session_security_diagnostics_row(session: &Session) -> SessionSecurityDiagnosticsDataType {
        let (client_user_id_of_session, authentication_mechanism) =
            Self::session_user_and_authentication_mechanism(session);

        SessionSecurityDiagnosticsDataType {
            session_id: session.session_id().clone(),
            client_user_id_of_session,
            authentication_mechanism,
            encoding: UAString::from("UA Binary"),
            transport_protocol: UAString::from(profiles::TRANSPORT_PROFILE_URI_BINARY),
            security_mode: session.message_security_mode(),
            security_policy_uri: UAString::from(session.security_policy_uri()),
            client_certificate: session
                .client_certificate()
                .and_then(|certificate| certificate.to_der().ok())
                .map(ByteString::from)
                .unwrap_or_else(ByteString::null),
            ..Default::default()
        }
    }

    #[cfg(feature = "diagnostics")]
    fn session_user_and_authentication_mechanism(session: &Session) -> (UAString, UAString) {
        match session.user_identity() {
            IdentityToken::None => (UAString::null(), UAString::null()),
            IdentityToken::Anonymous(identity) => (
                UAString::from(ANONYMOUS_USER_TOKEN_ID),
                identity.policy_id.clone(),
            ),
            IdentityToken::UserName(identity) => {
                (identity.user_name.clone(), identity.policy_id.clone())
            }
            IdentityToken::X509(identity) => (UAString::from("x509"), identity.policy_id.clone()),
            IdentityToken::IssuedToken(identity) => {
                (UAString::from("issued"), identity.policy_id.clone())
            }
            IdentityToken::Invalid(_) => (UAString::null(), UAString::from("Invalid")),
        }
    }

    #[cfg(feature = "diagnostics")]
    fn actual_session_timeout_ms(session: &Session) -> f64 {
        session
            .deadline()
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
            * 1000.0
    }

    #[cfg(feature = "diagnostics")]
    fn instant_to_date_time(instant: Instant) -> DateTime {
        let now_instant = Instant::now();
        let now = chrono::Utc::now();

        if instant <= now_instant {
            let Ok(offset) = ChronoDuration::from_std(now_instant.duration_since(instant)) else {
                return DateTime::now();
            };
            DateTime::from(now - offset)
        } else {
            let Ok(offset) = ChronoDuration::from_std(instant.duration_since(now_instant)) else {
                return DateTime::now();
            };
            DateTime::from(now + offset)
        }
    }

    #[cfg(all(feature = "diagnostics", feature = "subscriptions"))]
    fn is_internal_sampled(&self, node: &NodeId, context: &RequestContext) -> bool {
        let Some(variable_id) = self.get_variable_id(node) else {
            return false;
        };

        context.info.diagnostics.is_mapped(variable_id)
    }

    #[cfg(all(feature = "diagnostics", feature = "subscriptions"))]
    fn add_internal_sampler(
        &self,
        monitored_item: &mut CreateMonitoredItem,
        context: &RequestContext,
    ) -> Result<(), StatusCode> {
        let Some(var_id) = self.get_variable_id(&monitored_item.item_to_monitor().node_id) else {
            return Err(StatusCode::BadNodeIdUnknown);
        };

        if context.info.diagnostics.is_mapped(var_id) {
            let info = context.info.clone();
            self.sampler.add_sampler(
                monitored_item.item_to_monitor().node_id.clone(),
                monitored_item.item_to_monitor().attribute_id,
                move || info.diagnostics.get(var_id),
                monitored_item.monitoring_mode(),
                monitored_item.handle(),
                Duration::from_millis(monitored_item.sampling_interval() as u64),
            );
            Ok(())
        } else {
            Err(StatusCode::BadNodeIdUnknown)
        }
    }

    fn read_server_value(
        &self,
        context: &RequestContext,
        node: &ParsedReadValueId,
    ) -> Option<DataValue> {
        let var_id = self.get_variable_id(&node.node_id)?;

        let limits = &context.info.config.limits;

        // History capabilities: advertise false/0 when the history gate is off (FR-004).
        #[cfg(feature = "history")]
        let hist_cap = &context.info.capabilities.history;
        #[cfg(not(feature = "history"))]
        let hist_cap: &HistoryServerCapabilities = &HISTORY_OFF;

        let v: Variant = match var_id {
            VariableId::Server_ServerCapabilities_MaxArrayLength => {
                (limits.max_array_length as u32).into()
            }
            VariableId::Server_ServerCapabilities_MaxBrowseContinuationPoints => {
                (limits.max_browse_continuation_points as u16).into()
            }
            VariableId::Server_ServerCapabilities_MaxByteStringLength => {
                (limits.max_byte_string_length as u32).into()
            }
            VariableId::Server_ServerCapabilities_MaxHistoryContinuationPoints => {
                #[cfg(feature = "history")]
                { (limits.max_history_continuation_points as u16).into() }
                #[cfg(not(feature = "history"))]
                { 0u16.into() }
            }
            VariableId::Server_ServerCapabilities_MaxQueryContinuationPoints => {
                #[cfg(feature = "query")]
                { (limits.max_query_continuation_points as u16).into() }
                #[cfg(not(feature = "query"))]
                { 0u16.into() }
            }
            VariableId::Server_ServerCapabilities_MaxStringLength => {
                (limits.max_string_length as u32).into()
            }
            VariableId::Server_ServerCapabilities_MinSupportedSampleRate => {
                // Part 5 §6.3.2: MinSupportedSampleRate is a Duration (Double), not a UInt32 — the
                // value is already an f64, so expose it directly (the old `as u32` truncated it).
                limits.subscriptions.min_sampling_interval_ms.into()
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxMonitoredItemsPerCall => {
                (limits.operational.max_monitored_items_per_call as u32).into()
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerBrowse => {
                (limits.operational.max_nodes_per_browse as u32).into()
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerHistoryReadData => {
                #[cfg(feature = "history")]
                { (limits.operational.max_nodes_per_history_read_data as u32).into() }
                #[cfg(not(feature = "history"))]
                { 0u32.into() }
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerHistoryReadEvents => {
                #[cfg(feature = "history")]
                { (limits.operational.max_nodes_per_history_read_events as u32).into() }
                #[cfg(not(feature = "history"))]
                { 0u32.into() }
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerHistoryUpdateData => {
                #[cfg(feature = "history")]
                { (limits.operational.max_nodes_per_history_update as u32).into() }
                #[cfg(not(feature = "history"))]
                { 0u32.into() }
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerHistoryUpdateEvents => {
                #[cfg(feature = "history")]
                { (limits.operational.max_nodes_per_history_update as u32).into() }
                #[cfg(not(feature = "history"))]
                { 0u32.into() }
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerMethodCall => {
                #[cfg(feature = "method-call")]
                { (limits.operational.max_nodes_per_method_call as u32).into() }
                #[cfg(not(feature = "method-call"))]
                { 0u32.into() }
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerNodeManagement => {
                #[cfg(feature = "node-management")]
                { (limits.operational.max_nodes_per_node_management as u32).into() }
                #[cfg(not(feature = "node-management"))]
                { 0u32.into() }
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerRead => {
                (limits.operational.max_nodes_per_read as u32).into()
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerRegisterNodes => {
                (limits.operational.max_nodes_per_register_nodes as u32).into()
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerTranslateBrowsePathsToNodeIds => {
                (limits.operational.max_nodes_per_translate_browse_paths_to_node_ids as u32).into()
            }
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerWrite => {
                (limits.operational.max_nodes_per_write as u32).into()
            }
            VariableId::Server_ServerCapabilities_ServerProfileArray => {
                context.info.capabilities.profiles.clone().into()
            }
            // Part 5 §6.3.2: LocaleIdArray is a mandatory ServerCapabilities property; expose the
            // server's configured locales rather than the static (empty) node value.
            VariableId::Server_ServerCapabilities_LocaleIdArray => {
                context.info.config.locale_ids.clone().into()
            }

            VariableId::OPCUANamespaceMetadata_DefaultRolePermissions => {
                role_permissions_variant(context.info.namespace_defaults.role_permissions(0)?)
            }
            VariableId::OPCUANamespaceMetadata_DefaultUserRolePermissions => {
                let role_permissions = context.info.namespace_defaults.role_permissions(0)?;
                role_permissions_variant(&compute_user_role_permissions(
                    role_permissions,
                    context.user_roles(),
                ))
            }
            VariableId::OPCUANamespaceMetadata_DefaultAccessRestrictions => context
                .info
                .namespace_defaults
                .access_restrictions(0)?
                .bits()
                .into(),

            // History capabilities
            VariableId::HistoryServerCapabilities_AccessHistoryDataCapability => {
                hist_cap.access_history_data.into()
            }
            VariableId::HistoryServerCapabilities_AccessHistoryEventsCapability => {
                hist_cap.access_history_events.into()
            }
            VariableId::HistoryServerCapabilities_DeleteAtTimeCapability => {
                hist_cap.delete_at_time.into()
            }
            VariableId::HistoryServerCapabilities_DeleteEventCapability => {
                hist_cap.delete_event.into()
            }
            VariableId::HistoryServerCapabilities_DeleteRawCapability => {
                hist_cap.delete_raw.into()
            }
            VariableId::HistoryServerCapabilities_InsertAnnotationCapability => {
                hist_cap.insert_annotation.into()
            }
            VariableId::HistoryServerCapabilities_InsertDataCapability => {
                hist_cap.insert_data.into()
            }
            VariableId::HistoryServerCapabilities_InsertEventCapability => {
                hist_cap.insert_event.into()
            }
            VariableId::HistoryServerCapabilities_MaxReturnDataValues => {
                hist_cap.max_return_data_values.into()
            }
            VariableId::HistoryServerCapabilities_MaxReturnEventValues => {
                hist_cap.max_return_event_values.into()
            }
            VariableId::HistoryServerCapabilities_ReplaceDataCapability => {
                hist_cap.replace_data.into()
            }
            VariableId::HistoryServerCapabilities_ReplaceEventCapability => {
                hist_cap.replace_event.into()
            }
            VariableId::HistoryServerCapabilities_ServerTimestampSupported => {
                hist_cap.server_timestamp_supported.into()
            }
            VariableId::HistoryServerCapabilities_UpdateDataCapability => {
                hist_cap.update_data.into()
            }
            VariableId::HistoryServerCapabilities_UpdateEventCapability => {
                hist_cap.update_event.into()
            }

            // Misc server status
            VariableId::Server_ServiceLevel => {
                context.info.service_level.load(std::sync::atomic::Ordering::Relaxed).into()
            }
            VariableId::Server_LocalTime => {
                let offset = chrono::Local::now().offset().fix().local_minus_utc() / 60;
                ExtensionObject::from_message(TimeZoneDataType {
                    offset: offset.try_into().ok()?,
                    // TODO: Figure out how to set this. Chrono does not provide a way to
                    // tell whether daylight savings is in effect for the local time zone.
                    daylight_saving_in_offset: false,
                }).into()
            }

            // ServerStatus
            VariableId::Server_ServerStatus => {
                self.status.full_status_obj().into()
            }
            VariableId::Server_ServerStatus_BuildInfo => {
                ExtensionObject::from_message(self.status.build_info()).into()
            }
            VariableId::Server_ServerStatus_BuildInfo_BuildDate => {
                self.status.build_info().build_date.into()
            }
            VariableId::Server_ServerStatus_BuildInfo_BuildNumber => {
                self.status.build_info().build_number.into()
            }
            VariableId::Server_ServerStatus_BuildInfo_ManufacturerName => {
                self.status.build_info().manufacturer_name.into()
            }
            VariableId::Server_ServerStatus_BuildInfo_ProductName => {
                self.status.build_info().product_name.into()
            }
            VariableId::Server_ServerStatus_BuildInfo_ProductUri => {
                self.status.build_info().product_uri.into()
            }
            VariableId::Server_ServerStatus_BuildInfo_SoftwareVersion => {
                self.status.build_info().software_version.into()
            }
            VariableId::Server_ServerStatus_CurrentTime => {
                DateTime::now().into()
            }
            VariableId::Server_ServerStatus_SecondsTillShutdown => {
                match self.status.seconds_till_shutdown() {
                    Some(x) => x.into(),
                    None => Variant::Empty
                }
            }
            VariableId::Server_ServerStatus_ShutdownReason => {
                self.status.shutdown_reason().into()
            }
            VariableId::Server_ServerStatus_StartTime => {
                self.status.start_time().into()
            }
            VariableId::Server_ServerStatus_State => {
                (self.status.state() as i32).into()
            }

            VariableId::Server_NamespaceArray => {
                // This actually calls into other node managers to obtain the value, in fact
                // it calls into _this_ node manager as well.
                // Be careful to avoid holding exclusive locks in a way that causes a deadlock
                // when doing this. Here we hold a read lock on the address space,
                // but in this case it doesn't matter.
                let nss: HashMap<_, _> = self.node_managers.iter().flat_map(|n| n.namespaces_for_user(context)).map(|ns| (ns.namespace_index, ns.namespace_uri)).collect();
                // Make sure that holes are filled with empty strings, so that the
                // namespace array actually has correct indices.
                let &max = nss.keys().max()?;
                let namespaces: Vec<_> = (0..(max + 1)).map(|idx| nss.get(&idx).cloned().unwrap_or_default()).collect();
                namespaces.into()
            }

            VariableId::Server_ServerArray => {
                // Part 5 §5.3.2 (ServerArray): the list of Server URIs in this Server, as String[].
                // For a single server that is just its own application URI. Must be a (non-null)
                // array — strict clients (e.g. the OPC UA .NET Standard reference stack) validate
                // the DataType during session setup and reject a null/scalar value.
                vec![context.info.application_uri.clone()].into()
            }

            #[cfg(feature = "diagnostics")]
            r if context.info.diagnostics.is_mapped(r) => {
                let perms = context.info.authenticator.core_permissions(&context.token);
                if !perms.read_diagnostics {
                    return Some(DataValue::new_now_status(Variant::Empty, StatusCode::BadUserAccessDenied));
                } else {
                    return Some(context.info.diagnostics.get(r).unwrap_or_default())
                }
            }

            _ => return None,

        };

        let v = if !matches!(node.index_range, NumericRange::None) {
            match v.range_of(&node.index_range) {
                Ok(v) => v,
                Err(e) => {
                    return Some(DataValue {
                        value: None,
                        status: Some(e),
                        ..Default::default()
                    });
                }
            }
        } else {
            v
        };

        Some(DataValue {
            value: Some(v),
            status: Some(StatusCode::Good),
            source_timestamp: Some(**context.info.start_time.load()),
            server_timestamp: Some(**context.info.start_time.load()),
            ..Default::default()
        })
    }

    #[cfg_attr(not(feature = "diagnostics"), allow(unused_variables))]
    fn write_server_value(
        &self,
        context: &RequestContext,
        address_space: &RwLock<AddressSpace>,
        node: &ParsedWriteValue,
    ) -> StatusCode {
        #[cfg(feature = "diagnostics")]
        {
            let Some(var_id) = self.get_variable_id(&node.node_id) else {
                return StatusCode::BadServiceUnsupported;
            };

            if var_id != VariableId::Server_ServerDiagnostics_EnabledFlag
                || node.attribute_id != AttributeId::Value
            {
                return StatusCode::BadServiceUnsupported;
            }

            if trace_read_lock!(address_space)
                .find(&node.node_id)
                .is_none()
            {
                return StatusCode::BadNodeIdUnknown;
            }

            let perms = context.info.authenticator.core_permissions(&context.token);
            if !perms.write_diagnostics {
                return StatusCode::BadUserAccessDenied;
            }

            let Some(Variant::Boolean(enabled)) = node.value.value.as_ref() else {
                return StatusCode::BadTypeMismatch;
            };

            if node.index_range.has_range() {
                return StatusCode::BadWriteNotSupported;
            }

            context.info.diagnostics.set_enabled(*enabled);
            #[cfg(feature = "subscriptions")]
            if let Some(value) = context.info.diagnostics.get(var_id) {
                context
                    .subscriptions
                    .notify_data_change([(value, &node.node_id, node.attribute_id)].into_iter());
            }

            StatusCode::Good
        }
        #[cfg(not(feature = "diagnostics"))]
        {
            StatusCode::BadServiceUnsupported
        }
    }

    fn add_aggregates(&self, address_space: &mut AddressSpace, capabilities: &ServerCapabilities) {
        for aggregate in &capabilities.history.aggregates {
            address_space.insert_reference(
                &ObjectId::HistoryServerCapabilities_AggregateFunctions.into(),
                aggregate,
                ReferenceTypeId::Organizes,
            )
        }
    }

    /// Add a callback for `Call` on the method given by `id` that has access to the RequestContext.
    #[cfg(feature = "method-call")]
    pub fn add_method_callback_with_context(
        &self,
        id: NodeId,
        cb: impl Fn(&RequestContext, &NodeId, &[Variant]) -> Result<Vec<Variant>, StatusCode>
            + Send
            + Sync
            + 'static,
    ) {
        let mut cbs = trace_write_lock!(self.method_with_context_cbs);
        cbs.insert(id, Arc::new(cb));
    }

    #[cfg(feature = "method-call")]
    async fn call_builtin_method(
        &self,
        call: &mut MethodCall,
        context: &RequestContext,
    ) -> Result<(), StatusCode> {
        #[cfg(feature = "subscriptions-standard")]
        {
            let Ok(id) = call.method_id().as_method_id() else {
                return Ok(());
            };

            match id {
                MethodId::Server_GetMonitoredItems => {
                    let id = load_method_args!(call, UInt32)?;
                    let subs = context
                        .subscriptions
                        .get_session_subscriptions(context.session_id)
                        .ok_or(StatusCode::BadSessionIdInvalid)?;
                    let (ids, handles) = subs
                        .legacy(move |subs| {
                            let sub = subs.get(id).ok_or(StatusCode::BadSubscriptionIdInvalid)?;
                            let (ids, handles): (Vec<_>, Vec<_>) =
                                sub.items().map(|i| (i.id(), i.client_handle())).unzip();
                            Ok::<_, StatusCode>((ids, handles))
                        })
                        .await
                        .map_err(|_| StatusCode::BadSessionIdInvalid)??;
                    call.set_outputs(vec![ids.into(), handles.into()]);
                    call.set_status(StatusCode::Good);
                    return Ok(());
                }
                MethodId::Server_ResendData => {
                    let id = load_method_args!(call, UInt32)?;
                    let subs = context
                        .subscriptions
                        .get_session_subscriptions(context.session_id)
                        .ok_or(StatusCode::BadSessionIdInvalid)?;
                    subs.legacy(move |subs| {
                        let sub = subs
                            .get_mut(id)
                            .ok_or(StatusCode::BadSubscriptionIdInvalid)?;
                        sub.set_resend_data();
                        Ok::<_, StatusCode>(())
                    })
                    .await
                    .map_err(|_| StatusCode::BadSessionIdInvalid)??;
                    call.set_status(StatusCode::Good);
                    return Ok(());
                }
                _ => {}
            }
        }

        let cb = {
            let cbs = trace_read_lock!(self.method_with_context_cbs);
            cbs.get(call.method_id()).cloned()
        };

        if let Some(cb) = cb {
            match cb(context, call.object_id(), call.arguments()) {
                Ok(r) => {
                    call.set_outputs(r);
                    call.set_status(StatusCode::Good);
                }
                Err(e) => call.set_status(e),
            }
            return Ok(());
        }

        Err(StatusCode::BadNotSupported)
    }
}

fn role_permissions_variant(role_permissions: &[RolePermissionType]) -> Variant {
    role_permissions
        .iter()
        .cloned()
        .map(ExtensionObject::from_message)
        .collect::<Vec<_>>()
        .into()
}
