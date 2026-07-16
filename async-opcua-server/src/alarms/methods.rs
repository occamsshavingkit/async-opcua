//! Client method acknowledgment callbacks and identity validation handlers for Alarms.

use crate::address_space::AddressSpace;
use crate::alarms::dialog::DialogRegistry;
use crate::alarms::dispatch::ServerAlarmEvent;
use crate::alarms::refresh_events::{RefreshEndEvent, RefreshStartEvent};
use crate::alarms::registry::ConditionRegistry;
use crate::alarms::state_machine::Branch;
use crate::alarms::state_machine::ConditionStateMachine;
use crate::alarms::transitions::{acknowledge_alarm, confirm_alarm};
use crate::node_manager::RequestContext;
use crate::MonitoredItemHandle;
use opcua_core::events::AlarmEvent;
use opcua_core::traits::ConditionMethodHandler;
use opcua_nodes::{BaseEventType, Event, EventField};
#[cfg(all(feature = "generated-address-space", feature = "method-call"))]
use opcua_types::MethodId;
use opcua_types::{
    AttributeId, ByteString, DateTime, LocalizedText, NodeId, NumericRange, ObjectId, ObjectTypeId,
    QualifiedName, StatusCode, TryFromVariant, UAString, Variant,
};
use std::sync::Arc;

/// Handler for Alarm Acknowledge/Confirm/AddComment method calls.
pub struct AlarmMethodHandler {
    /// Associated Alarm condition state machine
    pub state_machine: ConditionStateMachine,
    /// Thread-safe reference to the AddressSpace
    pub address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
}

impl ConditionMethodHandler for AlarmMethodHandler {
    fn handle_acknowledgment(
        &self,
        _session_id: &NodeId,
        condition_id: &NodeId,
        _event_id: &[u8],
        comment: &LocalizedText,
    ) -> Result<StatusCode, StatusCode> {
        if condition_id != &self.state_machine.condition_id {
            return Err(StatusCode::BadConditionDisabled);
        }

        let address_space = opcua_core::trace_write_lock!(self.address_space);
        match acknowledge_alarm(&address_space, &self.state_machine, comment.clone()) {
            Ok(_) => Ok(StatusCode::Good),
            Err(e) => Err(e),
        }
    }
}

impl AlarmMethodHandler {
    /// Creates a new `AlarmMethodHandler` instance.
    pub fn new(
        state_machine: ConditionStateMachine,
        address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
    ) -> Self {
        Self {
            state_machine,
            address_space,
        }
    }

    /// Callback executed when the Acknowledge method is called by a client.
    pub fn handle_ack_method(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        // FR-019: Trace user token securely (masking unencrypted token/key and logging SHA-256 hash)
        {
            let session = opcua_core::trace_read_lock!(context.session);
            if let crate::identity_token::IdentityToken::IssuedToken(ref token) =
                session.user_identity()
            {
                let token_str = String::from_utf8_lossy(token.token_data.as_ref());
                let hashed = opcua_core::logging::hash_jwt(&token_str);
                tracing::info!(
                    "Acknowledge called on alarm {} by user with token hash: {}",
                    self.state_machine.condition_name,
                    hashed
                );
            }
        }

        if args.len() < 2 {
            return Err(StatusCode::BadArgumentsMissing);
        }

        let event_id = match &args[0] {
            Variant::ByteString(ref b) => b.as_ref(),
            _ => return Err(StatusCode::BadTypeMismatch),
        };

        if self.state_machine.branch_by_event_id(event_id).is_some() {
            self.state_machine.ack_branch(event_id);
            return Ok(vec![]);
        }

        // Part 9 §5.5.2: the EventId must identify the condition's current reportable state.
        if !self.state_machine.current_event_id_matches(event_id) {
            return Err(StatusCode::BadEventIdUnknown);
        }

        let comment = match &args[1] {
            Variant::LocalizedText(ref t) => (**t).clone(),
            _ => return Err(StatusCode::BadTypeMismatch),
        };

        let session_id = opcua_core::trace_read_lock!(context.session)
            .session_id()
            .clone();

        // Perform transition
        let res = self.handle_acknowledgment(
            &session_id,
            &self.state_machine.condition_id,
            event_id,
            &comment,
        );
        res?;

        // Prepare and route the event notification
        let address_space = opcua_core::trace_read_lock!(self.address_space);
        let active = self.state_machine.get_active(&address_space);
        let confirmed = self.state_machine.get_confirmed(&address_space);
        let severity = self.state_machine.get_severity(&address_space);
        let retain = active || !confirmed;

        let event = opcua_core::events::AlarmEvent {
            event_id: event_id.to_vec(),
            event_type: NodeId::new(0, 2915),
            source_node: self.state_machine.source_node_id.clone(),
            source_name: self.state_machine.condition_name.clone(),
            time: DateTime::now(),
            message: comment,
            severity,
            condition_id: self.state_machine.condition_id.clone(),
            branch_id: NodeId::null(),
            condition_name: self.state_machine.condition_name.clone(),
            active_state: active,
            acked_state: true,
            confirmed_state: confirmed,
            retain,
        };

        notify_alarm_event(context, &event);

        Ok(vec![])
    }

    /// Callback executed when the AddComment method is called by a client.
    pub fn handle_add_comment_method(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        // FR-019: Trace user token securely (masking unencrypted token/key and logging SHA-256 hash)
        {
            let session = opcua_core::trace_read_lock!(context.session);
            if let crate::identity_token::IdentityToken::IssuedToken(ref token) =
                session.user_identity()
            {
                let token_str = String::from_utf8_lossy(token.token_data.as_ref());
                let hashed = opcua_core::logging::hash_jwt(&token_str);
                tracing::info!(
                    "AddComment called on alarm {} by user with token hash: {}",
                    self.state_machine.condition_name,
                    hashed
                );
            }
        }

        if args.len() < 2 {
            return Err(StatusCode::BadArgumentsMissing);
        }

        let event_id = match &args[0] {
            Variant::ByteString(ref b) => b.as_ref(),
            _ => return Err(StatusCode::BadTypeMismatch),
        };

        if self.state_machine.branch_by_event_id(event_id).is_none()
            && !self.state_machine.current_event_id_matches(event_id)
        {
            return Err(StatusCode::BadEventIdUnknown);
        }

        let comment = match &args[1] {
            Variant::LocalizedText(ref t) => (**t).clone(),
            _ => return Err(StatusCode::BadTypeMismatch),
        };

        let address_space = opcua_core::trace_read_lock!(self.address_space);
        let active = self.state_machine.get_active(&address_space);
        let acked = self.state_machine.get_acked(&address_space);
        let confirmed = self.state_machine.get_confirmed(&address_space);
        let severity = self.state_machine.get_severity(&address_space);
        let retain = active || !confirmed;

        let event = opcua_core::events::AlarmEvent {
            event_id: event_id.to_vec(),
            event_type: NodeId::new(0, 2915),
            source_node: self.state_machine.source_node_id.clone(),
            source_name: self.state_machine.condition_name.clone(),
            time: DateTime::now(),
            message: comment.clone(),
            severity,
            condition_id: self.state_machine.condition_id.clone(),
            branch_id: NodeId::null(),
            condition_name: self.state_machine.condition_name.clone(),
            active_state: active,
            acked_state: acked,
            confirmed_state: confirmed,
            retain,
        };

        let audit_event = ConditionAuditEvent::new(
            ObjectTypeId::AuditConditionCommentEventType,
            "AddComment",
            &self.state_machine,
            StatusCode::Good,
            Some(comment),
        );
        notify_condition_audit_event(context, &audit_event);
        notify_alarm_event(context, &event);

        Ok(vec![])
    }

    /// Callback executed when the Confirm method is called by a client.
    pub fn handle_confirm_method(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        {
            let session = opcua_core::trace_read_lock!(context.session);
            if let crate::identity_token::IdentityToken::IssuedToken(ref token) =
                session.user_identity()
            {
                let token_str = String::from_utf8_lossy(token.token_data.as_ref());
                let hashed = opcua_core::logging::hash_jwt(&token_str);
                tracing::info!(
                    "Confirm called on alarm {} by user with token hash: {}",
                    self.state_machine.condition_name,
                    hashed
                );
            }
        }

        if args.len() < 2 {
            return Err(StatusCode::BadArgumentsMissing);
        }

        let event_id = match &args[0] {
            Variant::ByteString(ref b) => b.as_ref(),
            _ => return Err(StatusCode::BadTypeMismatch),
        };

        if self.state_machine.branch_by_event_id(event_id).is_some() {
            self.state_machine.confirm_branch(event_id);
            return Ok(vec![]);
        }

        // Part 9 §5.5.2: the EventId must identify the condition's current reportable state.
        if !self.state_machine.current_event_id_matches(event_id) {
            return Err(StatusCode::BadEventIdUnknown);
        }

        let comment = match &args[1] {
            Variant::LocalizedText(ref t) => (**t).clone(),
            _ => return Err(StatusCode::BadTypeMismatch),
        };

        let address_space = opcua_core::trace_write_lock!(self.address_space);
        match confirm_alarm(&address_space, &self.state_machine, comment) {
            Ok(Some(event)) => {
                notify_alarm_event(context, &event);
                Ok(vec![])
            }
            Ok(None) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }
}

/// Handler for OPC UA Part 9 dialog response method calls on standard namespace method ids.
pub struct DialogConditionMethodHandler {
    /// Shared registry of dialog conditions to receive responses.
    pub registry: DialogRegistry,
    /// Thread-safe reference to the AddressSpace.
    pub address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
}

impl DialogConditionMethodHandler {
    /// Creates a new `DialogConditionMethodHandler` instance.
    pub fn new(
        registry: DialogRegistry,
        address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
    ) -> Self {
        Self {
            registry,
            address_space,
        }
    }

    /// Callback executed when the standard Respond method is called by a client.
    pub fn handle_dialog_respond(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let selected_response = parse_i32_arg(args, 0)?;
        let dialog = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        let event = dialog.respond(&address_space, selected_response)?;
        notify_alarm_event(context, &event);
        Ok(vec![])
    }

    /// Callback executed when the standard Respond2 method is called by a client.
    pub fn handle_dialog_respond2(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let selected_response = parse_i32_arg(args, 0)?;
        let comment = parse_localized_text_arg(args, 1)?;
        let dialog = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        let event = dialog.respond2(&address_space, selected_response, comment)?;
        notify_alarm_event(context, &event);
        Ok(vec![])
    }
}

/// Handler for OPC UA Part 9 condition method calls on standard namespace method ids.
pub struct ConditionRefreshHandler {
    /// Shared registry of condition state machines to replay.
    pub registry: ConditionRegistry,
    /// Thread-safe reference to the AddressSpace.
    pub address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
}

impl ConditionRefreshHandler {
    /// Creates a new `ConditionRefreshHandler` instance.
    pub fn new(
        registry: ConditionRegistry,
        address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
    ) -> Self {
        Self {
            registry,
            address_space,
        }
    }

    /// Callback executed when the ConditionRefresh method is called by a client.
    pub fn handle_condition_refresh(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let subscription_id = parse_u32_arg(args, 0)?;
        self.refresh_events(context, subscription_id, None)
    }

    /// Callback executed when the ConditionRefresh2 method is called by a client.
    pub fn handle_condition_refresh2(
        &self,
        context: &RequestContext,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let subscription_id = parse_u32_arg(args, 0)?;
        let monitored_item_id = parse_u32_arg(args, 1)?;
        let monitored_item = MonitoredItemHandle {
            subscription_id,
            monitored_item_id,
        };
        self.refresh_events(context, subscription_id, Some(monitored_item))
    }

    /// Callback executed when the standard Acknowledge method is called by a client.
    pub fn handle_condition_acknowledge(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        AlarmMethodHandler::new(condition, self.address_space.clone())
            .handle_ack_method(context, args)
    }

    /// Callback executed when the standard AddComment method is called by a client.
    pub fn handle_condition_add_comment(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        AlarmMethodHandler::new(condition, self.address_space.clone())
            .handle_add_comment_method(context, args)
    }

    /// Callback executed when the standard Confirm method is called by a client.
    pub fn handle_condition_confirm(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        AlarmMethodHandler::new(condition, self.address_space.clone())
            .handle_confirm_method(context, args)
    }

    /// Callback executed when the standard Enable method (OPC-10000-9 §5.5.5) is called by a client.
    pub fn handle_condition_enable(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        _args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        {
            let address_space = opcua_core::trace_write_lock!(self.address_space);
            condition.set_enabled(&address_space, true);
        }
        notify_enable_audit_event(context, &condition, "Enable", StatusCode::Good);
        Ok(vec![])
    }

    /// Callback executed when the standard Disable method (OPC-10000-9 §5.5.4) is called by a client.
    pub fn handle_condition_disable(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        _args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        {
            let address_space = opcua_core::trace_write_lock!(self.address_space);
            condition.set_enabled(&address_space, false);
        }
        notify_enable_audit_event(context, &condition, "Disable", StatusCode::Good);
        Ok(vec![])
    }

    /// Callback executed when the standard Suppress/Suppress2 method (OPC-10000-9 §5.8.8/§5.8.9)
    /// is called by a client. `args[0]` (Suppress2 only) is an optional Comment.
    pub fn handle_condition_suppress(
        &self,
        _context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        apply_optional_comment(&condition, args, &address_space)?;
        condition.set_suppressed(&address_space, true);
        Ok(vec![])
    }

    /// Callback executed when the standard Unsuppress/Unsuppress2 method (OPC-10000-9
    /// §5.8.10/§5.8.11) is called by a client. `args[0]` (Unsuppress2 only) is an optional Comment.
    pub fn handle_condition_unsuppress(
        &self,
        _context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        apply_optional_comment(&condition, args, &address_space)?;
        condition.set_suppressed(&address_space, false);
        Ok(vec![])
    }

    /// Callback executed when the standard RemoveFromService/RemoveFromService2 method
    /// (OPC-10000-9 §5.8.12/§5.8.13) is called by a client. `args[0]` (RemoveFromService2 only)
    /// is an optional Comment.
    pub fn handle_condition_remove_from_service(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        {
            let address_space = opcua_core::trace_write_lock!(self.address_space);
            apply_optional_comment(&condition, args, &address_space)?;
            condition.set_out_of_service(&address_space, true);
        }
        notify_out_of_service_audit_event(
            context,
            &condition,
            "RemoveFromService",
            StatusCode::Good,
        );
        Ok(vec![])
    }

    /// Callback executed when the standard PlaceInService/PlaceInService2 method (OPC-10000-9
    /// §5.8.14/§5.8.15) is called by a client. `args[0]` (PlaceInService2 only) is an optional
    /// Comment.
    pub fn handle_condition_place_in_service(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        {
            let address_space = opcua_core::trace_write_lock!(self.address_space);
            apply_optional_comment(&condition, args, &address_space)?;
            condition.set_out_of_service(&address_space, false);
        }
        notify_out_of_service_audit_event(context, &condition, "PlaceInService", StatusCode::Good);
        Ok(vec![])
    }

    /// Callback executed when the standard Silence method (OPC-10000-9 §5.8.7) is called by a
    /// client. Idempotent: silencing an already-silenced alarm succeeds without error.
    pub fn handle_condition_silence(
        &self,
        context: &RequestContext,
        object_id: &NodeId,
        _args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        {
            let address_space = opcua_core::trace_write_lock!(self.address_space);
            condition.set_silenced(&address_space, true);
        }
        let audit_event = ConditionAuditEvent::new(
            ObjectTypeId::AuditConditionSilenceEventType,
            "Silence",
            &condition,
            StatusCode::Good,
            None,
        );
        notify_condition_audit_event(context, &audit_event);
        Ok(vec![])
    }

    /// Callback executed when the standard OneShotShelve method is called by a client.
    pub fn handle_condition_one_shot_shelve(
        &self,
        _context: &RequestContext,
        object_id: &NodeId,
        _args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get_by_shelving_state(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        let status_code = condition.one_shot_shelve(&address_space);
        if status_code.is_good() {
            Ok(vec![])
        } else {
            Err(status_code)
        }
    }

    /// Callback executed when the standard TimedShelve method is called by a client.
    pub fn handle_condition_timed_shelve(
        &self,
        _context: &RequestContext,
        object_id: &NodeId,
        args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let shelving_time_ms = parse_f64_arg(args, 0)?;
        let condition = self
            .registry
            .get_by_shelving_state(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        let status_code = condition.timed_shelve(&address_space, shelving_time_ms);
        if status_code.is_good() {
            Ok(vec![])
        } else {
            Err(status_code)
        }
    }

    /// Callback executed when the standard Unshelve method is called by a client.
    pub fn handle_condition_unshelve(
        &self,
        _context: &RequestContext,
        object_id: &NodeId,
        _args: &[Variant],
    ) -> Result<Vec<Variant>, StatusCode> {
        let condition = self
            .registry
            .get_by_shelving_state(object_id)
            .ok_or(StatusCode::BadNodeIdUnknown)?;
        let address_space = opcua_core::trace_write_lock!(self.address_space);
        let status_code = condition.unshelve(&address_space);
        if status_code.is_good() {
            Ok(vec![])
        } else {
            Err(status_code)
        }
    }

    fn refresh_events(
        &self,
        context: &RequestContext,
        subscription_id: u32,
        monitored_item: Option<MonitoredItemHandle>,
    ) -> Result<Vec<Variant>, StatusCode> {
        let address_space = opcua_core::trace_read_lock!(self.address_space);
        let conditions = self.registry.iter_retained(&address_space);
        let mut alarm_events = Vec::with_capacity(conditions.len());
        for condition in conditions {
            alarm_events.push(build_current_alarm_event(&condition, &address_space));
            for branch in condition.retained_branches() {
                alarm_events.push(build_branch_alarm_event(
                    &condition,
                    &branch,
                    &address_space,
                ));
            }
        }
        drop(address_space);

        let mut events: Vec<Box<dyn Event + Send>> = Vec::with_capacity(alarm_events.len() + 2);
        events.push(Box::new(RefreshStartEvent::new()));
        events.extend(
            alarm_events
                .into_iter()
                .map(|event| Box::new(OwnedAlarmEvent::new(event)) as Box<dyn Event + Send>),
        );
        events.push(Box::new(RefreshEndEvent::new()));

        context.subscriptions.refresh_subscription_events(
            context.session_id,
            subscription_id,
            monitored_item,
            events,
        )?;

        Ok(vec![])
    }
}

#[derive(Clone)]
pub(crate) struct OwnedAlarmEvent {
    event: AlarmEvent,
}

impl OwnedAlarmEvent {
    pub(crate) fn new(event: AlarmEvent) -> Self {
        Self { event }
    }
}

impl Event for OwnedAlarmEvent {
    fn clone_box(&self) -> Box<dyn Event + Send> {
        Box::new(self.clone())
    }

    fn time(&self) -> &DateTime {
        &self.event.time
    }

    fn event_type_id(&self) -> &NodeId {
        &self.event.event_type
    }

    fn get_field(
        &self,
        _type_definition_id: &NodeId,
        attribute_id: AttributeId,
        index_range: &NumericRange,
        browse_path: &[QualifiedName],
    ) -> Variant {
        self.get_value(attribute_id, index_range, browse_path)
    }
}

impl EventField for OwnedAlarmEvent {
    fn get_value(
        &self,
        attribute_id: AttributeId,
        _index_range: &NumericRange,
        remaining_path: &[QualifiedName],
    ) -> Variant {
        if attribute_id != AttributeId::Value {
            return Variant::Empty;
        }
        if remaining_path.is_empty() {
            return Variant::Empty;
        }

        let first_name = remaining_path[0].name.as_ref();

        if remaining_path.len() == 2 {
            let second_name = remaining_path[1].name.as_ref();
            if second_name == "Id" {
                match first_name {
                    "ActiveState" => return Variant::from(self.event.active_state),
                    "AckedState" => return Variant::from(self.event.acked_state),
                    "ConfirmedState" => return Variant::from(self.event.confirmed_state),
                    "DialogState" => return Variant::from(self.event.active_state),
                    "EnabledState" => return Variant::from(true),
                    _ => {}
                }
            }
        }

        if remaining_path.len() == 1 {
            match first_name {
                "EventId" => {
                    return Variant::from(ByteString::from(self.event.event_id.clone()));
                }
                "EventType" => return Variant::from(self.event.event_type.clone()),
                "SourceNode" => return Variant::from(self.event.source_node.clone()),
                "SourceName" => {
                    return Variant::from(UAString::from(self.event.source_name.clone()))
                }
                "Time" => return Variant::from(self.event.time),
                "ReceiveTime" => return Variant::from(self.event.time),
                "Message" => return Variant::from(self.event.message.clone()),
                "Severity" => return Variant::from(self.event.severity),
                "ConditionId" => return Variant::from(self.event.condition_id.clone()),
                "BranchId" => {
                    return Variant::NodeId(Box::new(self.event.branch_id.clone()));
                }
                "ConditionName" => {
                    return Variant::from(UAString::from(self.event.condition_name.clone()));
                }
                "Retain" => return Variant::from(self.event.retain),
                "ActiveState" => return Variant::from(self.event.active_state),
                "AckedState" => return Variant::from(self.event.acked_state),
                "ConfirmedState" => return Variant::from(self.event.confirmed_state),
                "DialogState" => return Variant::from(self.event.active_state),
                "EnabledState" => return Variant::from(true),
                _ => {}
            }
        }

        Variant::Empty
    }
}

/// A&C audit event (OPC-10000-9 §5.10.2 `AuditConditionEventType` family): Enable/Disable,
/// Silence, RemoveFromService/PlaceInService, and AddComment actions.
///
/// Deliberately lighter-weight than `session/audit.rs`'s `ServerAuditEvent`: this call path
/// (a Method callback registered via `add_method_callback_with_context`, invoked from the
/// generic Call service dispatch) has a `RequestContext`, not the `RequestHeader`-derived
/// `AuditEventContext` (client audit entry id, secure channel id, ...) that session-service-layer
/// audit events use — building one here would require threading request-header state through a
/// call path that doesn't carry it today. This event still carries what OPC-10000-9 §5.10.2-5.10.4
/// actually require: `ConditionId`, `ConditionEventId`, and (for AddComment) `Comment`, plus the
/// generic `BaseEventType` fields (`Time`/`Message`/`Severity`/`SourceNode`).
#[derive(Clone)]
struct ConditionAuditEvent {
    base: BaseEventType,
    status: bool,
    /// The Node that should receive this notification. Audit events are raised with
    /// `SourceNode = Server` (matching every other audit event in this codebase,
    /// `session/audit.rs`'s `ServerAuditEvent`), not the alarm's own source device — using the
    /// alarm's source would deliver audit notifications to every plain alarm-event subscription
    /// on that device too, corrupting the existing `AlarmEvent` notification stream those
    /// subscriptions expect.
    notify_target: NodeId,
    condition_event_id: ByteString,
    comment: Option<LocalizedText>,
}

impl ConditionAuditEvent {
    fn new(
        event_type: ObjectTypeId,
        action: &str,
        condition: &ConditionStateMachine,
        status_code: StatusCode,
        comment: Option<LocalizedText>,
    ) -> Self {
        let now = DateTime::now();
        let status = status_code.is_good();
        let severity = if status { 100 } else { 900 };
        let message = if status {
            format!("{action} succeeded")
        } else {
            format!("{action} failed: {status_code}")
        };
        let server_node: NodeId = ObjectId::Server.into();
        let base = BaseEventType::new(
            event_type,
            ByteString::from(uuid::Uuid::new_v4().as_bytes().as_slice()),
            message,
            now,
        )
        .set_source_node(server_node.clone())
        .set_source_name(UAString::from("Server"))
        .set_severity(severity);

        Self {
            base,
            status,
            notify_target: server_node,
            condition_event_id: ByteString::from(condition.current_event_id()),
            comment,
        }
    }
}

impl Event for ConditionAuditEvent {
    fn clone_box(&self) -> Box<dyn Event + Send> {
        Box::new(self.clone())
    }

    fn get_field(
        &self,
        _type_definition_id: &NodeId,
        attribute_id: AttributeId,
        index_range: &NumericRange,
        browse_path: &[QualifiedName],
    ) -> Variant {
        self.get_value(attribute_id, index_range, browse_path)
    }

    fn time(&self) -> &DateTime {
        &self.base.time
    }

    fn event_type_id(&self) -> &NodeId {
        &self.base.event_type
    }
}

impl EventField for ConditionAuditEvent {
    fn get_value(
        &self,
        attribute_id: AttributeId,
        index_range: &NumericRange,
        remaining_path: &[QualifiedName],
    ) -> Variant {
        if attribute_id != AttributeId::Value || remaining_path.len() != 1 {
            return self
                .base
                .get_value(attribute_id, index_range, remaining_path);
        }

        match remaining_path[0].name.as_ref() {
            "Status" => Variant::from(self.status),
            // Not "ConditionId": OPC-10000-9 §5.10.2 declares `ConditionEventId` on
            // AuditConditionEventType, inherited by every subtype — `ConditionId` is an
            // `AlarmEvent`-only field from a different type hierarchy (ConditionType).
            "ConditionEventId" => Variant::from(self.condition_event_id.clone()),
            "Comment" => match &self.comment {
                Some(text) => Variant::from(text.clone()),
                None => Variant::from(LocalizedText::null()),
            },
            _ => self
                .base
                .get_value(attribute_id, index_range, remaining_path),
        }
    }
}

fn notify_condition_audit_event(context: &RequestContext, event: &ConditionAuditEvent) {
    let items = std::iter::once((event as &dyn Event, &event.notify_target));
    context.subscriptions.notify_events(items);
}

/// Emits `AuditConditionEnableEventType` (OPC-10000-9 §5.10.2/§5.10.3) for Enable/Disable.
fn notify_enable_audit_event(
    context: &RequestContext,
    condition: &ConditionStateMachine,
    action: &str,
    status_code: StatusCode,
) {
    let event = ConditionAuditEvent::new(
        ObjectTypeId::AuditConditionEnableEventType,
        action,
        condition,
        status_code,
        None,
    );
    notify_condition_audit_event(context, &event);
}

/// Emits `AuditConditionOutOfServiceEventType` (OPC-10000-9 §5.10.12) for RemoveFromService/
/// PlaceInService.
fn notify_out_of_service_audit_event(
    context: &RequestContext,
    condition: &ConditionStateMachine,
    action: &str,
    status_code: StatusCode,
) {
    let event = ConditionAuditEvent::new(
        ObjectTypeId::AuditConditionOutOfServiceEventType,
        action,
        condition,
        status_code,
        None,
    );
    notify_condition_audit_event(context, &event);
}

/// Registers standard ConditionRefresh, ConditionRefresh2, AddComment, Acknowledge, and Confirm method callbacks.
#[cfg(all(feature = "generated-address-space", feature = "method-call"))]
pub fn register_condition_methods(
    core_node_manager: &crate::node_manager::memory::CoreNodeManager,
    registry: ConditionRegistry,
    address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
) {
    let handler = Arc::new(ConditionRefreshHandler::new(registry, address_space));

    let refresh_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ConditionType_ConditionRefresh.into(),
        move |ctx, _object_id, args| refresh_handler.handle_condition_refresh(ctx, args),
    );

    let refresh2_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ConditionType_ConditionRefresh2.into(),
        move |ctx, _object_id, args| refresh2_handler.handle_condition_refresh2(ctx, args),
    );

    let acknowledge_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AcknowledgeableConditionType_Acknowledge.into(),
        move |ctx, object_id, args| {
            acknowledge_handler.handle_condition_acknowledge(ctx, object_id, args)
        },
    );

    let add_comment_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ConditionType_AddComment.into(),
        move |ctx, object_id, args| {
            add_comment_handler.handle_condition_add_comment(ctx, object_id, args)
        },
    );

    let confirm_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AcknowledgeableConditionType_Confirm.into(),
        move |ctx, object_id, args| confirm_handler.handle_condition_confirm(ctx, object_id, args),
    );

    let enable_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ConditionType_Enable.into(),
        move |ctx, object_id, args| enable_handler.handle_condition_enable(ctx, object_id, args),
    );

    let disable_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ConditionType_Disable.into(),
        move |ctx, object_id, args| disable_handler.handle_condition_disable(ctx, object_id, args),
    );

    let suppress_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_Suppress.into(),
        move |ctx, object_id, args| {
            suppress_handler.handle_condition_suppress(ctx, object_id, args)
        },
    );

    let suppress2_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_Suppress2.into(),
        move |ctx, object_id, args| {
            suppress2_handler.handle_condition_suppress(ctx, object_id, args)
        },
    );

    let unsuppress_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_Unsuppress.into(),
        move |ctx, object_id, args| {
            unsuppress_handler.handle_condition_unsuppress(ctx, object_id, args)
        },
    );

    let unsuppress2_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_Unsuppress2.into(),
        move |ctx, object_id, args| {
            unsuppress2_handler.handle_condition_unsuppress(ctx, object_id, args)
        },
    );

    let remove_from_service_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_RemoveFromService.into(),
        move |ctx, object_id, args| {
            remove_from_service_handler.handle_condition_remove_from_service(ctx, object_id, args)
        },
    );

    let remove_from_service2_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_RemoveFromService2.into(),
        move |ctx, object_id, args| {
            remove_from_service2_handler.handle_condition_remove_from_service(ctx, object_id, args)
        },
    );

    let place_in_service_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_PlaceInService.into(),
        move |ctx, object_id, args| {
            place_in_service_handler.handle_condition_place_in_service(ctx, object_id, args)
        },
    );

    let place_in_service2_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_PlaceInService2.into(),
        move |ctx, object_id, args| {
            place_in_service2_handler.handle_condition_place_in_service(ctx, object_id, args)
        },
    );

    let silence_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::AlarmConditionType_Silence.into(),
        move |ctx, object_id, args| silence_handler.handle_condition_silence(ctx, object_id, args),
    );

    let one_shot_shelve_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ShelvedStateMachineType_OneShotShelve.into(),
        move |ctx, object_id, args| {
            one_shot_shelve_handler.handle_condition_one_shot_shelve(ctx, object_id, args)
        },
    );

    let timed_shelve_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ShelvedStateMachineType_TimedShelve.into(),
        move |ctx, object_id, args| {
            timed_shelve_handler.handle_condition_timed_shelve(ctx, object_id, args)
        },
    );

    core_node_manager.inner().add_method_callback_with_context(
        MethodId::ShelvedStateMachineType_Unshelve.into(),
        move |ctx, object_id, args| handler.handle_condition_unshelve(ctx, object_id, args),
    );
}

/// Registers standard DialogConditionType Respond and Respond2 method callbacks.
#[cfg(all(feature = "generated-address-space", feature = "method-call"))]
pub fn register_dialog_condition_methods(
    core_node_manager: &crate::node_manager::memory::CoreNodeManager,
    registry: DialogRegistry,
    address_space: Arc<opcua_core::sync::RwLock<AddressSpace>>,
) {
    let handler = Arc::new(DialogConditionMethodHandler::new(registry, address_space));

    let respond_handler = handler.clone();
    core_node_manager.inner().add_method_callback_with_context(
        MethodId::DialogConditionType_Respond.into(),
        move |ctx, object_id, args| respond_handler.handle_dialog_respond(ctx, object_id, args),
    );

    core_node_manager.inner().add_method_callback_with_context(
        MethodId::DialogConditionType_Respond2.into(),
        move |ctx, object_id, args| handler.handle_dialog_respond2(ctx, object_id, args),
    );
}

fn notify_alarm_event(context: &RequestContext, event: &AlarmEvent) {
    let wrapper = ServerAlarmEvent { event };
    let items = std::iter::once((&wrapper as &dyn Event, &event.source_node));
    context.subscriptions.notify_events(items);
}

/// Applies the optional `Comment` argument used by the "2" variants of the standard AlarmConditionType
/// lifecycle Methods (Suppress2, Unsuppress2, RemoveFromService2, PlaceInService2 — OPC-10000-9
/// §5.8.9/§5.8.11/§5.8.13/§5.8.15). A missing argument (the base, non-"2" Method) or a NULL
/// LocalizedText is a no-op, not an error; a present, non-LocalizedText argument is rejected.
fn apply_optional_comment(
    condition: &ConditionStateMachine,
    args: &[Variant],
    address_space: &AddressSpace,
) -> Result<(), StatusCode> {
    let Some(arg) = args.first() else {
        return Ok(());
    };
    let Variant::LocalizedText(ref text) = arg else {
        return Err(StatusCode::BadTypeMismatch);
    };
    if text.text.value().is_some() {
        condition.set_message(address_space, (**text).clone());
    }
    Ok(())
}

fn parse_u32_arg(args: &[Variant], index: usize) -> Result<u32, StatusCode> {
    let Some(arg) = args.get(index) else {
        return Err(StatusCode::BadInvalidArgument);
    };
    u32::try_from_variant(arg.clone()).map_err(|_| StatusCode::BadInvalidArgument)
}

fn parse_f64_arg(args: &[Variant], index: usize) -> Result<f64, StatusCode> {
    let Some(arg) = args.get(index) else {
        return Err(StatusCode::BadInvalidArgument);
    };
    f64::try_from_variant(arg.clone()).map_err(|_| StatusCode::BadInvalidArgument)
}

fn parse_i32_arg(args: &[Variant], index: usize) -> Result<i32, StatusCode> {
    match args.get(index) {
        Some(Variant::Int32(value)) => Ok(*value),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn parse_localized_text_arg(args: &[Variant], index: usize) -> Result<LocalizedText, StatusCode> {
    match args.get(index) {
        Some(Variant::LocalizedText(value)) => Ok((**value).clone()),
        Some(_) => Err(StatusCode::BadTypeMismatch),
        None => Err(StatusCode::BadArgumentsMissing),
    }
}

fn build_current_alarm_event(
    state_machine: &ConditionStateMachine,
    address_space: &AddressSpace,
) -> AlarmEvent {
    let mut event_id = state_machine.current_event_id();
    if event_id.is_empty() {
        event_id = uuid::Uuid::new_v4().as_bytes().to_vec();
    }

    AlarmEvent {
        event_id,
        event_type: NodeId::new(0, 2915),
        source_node: state_machine.source_node_id.clone(),
        source_name: state_machine.condition_name.clone(),
        time: DateTime::now(),
        message: state_machine.get_message(address_space),
        severity: state_machine.get_severity(address_space),
        condition_id: state_machine.condition_id.clone(),
        branch_id: NodeId::null(),
        condition_name: state_machine.condition_name.clone(),
        active_state: state_machine.get_active(address_space),
        acked_state: state_machine.get_acked(address_space),
        confirmed_state: state_machine.get_confirmed(address_space),
        retain: state_machine.get_retain(address_space),
    }
}

fn build_branch_alarm_event(
    state_machine: &ConditionStateMachine,
    branch: &Branch,
    _address_space: &AddressSpace,
) -> AlarmEvent {
    AlarmEvent {
        event_id: branch.event_id.clone(),
        event_type: NodeId::new(0, 2915),
        source_node: state_machine.source_node_id.clone(),
        source_name: state_machine.condition_name.clone(),
        time: DateTime::now(),
        message: branch.message.clone(),
        severity: branch.severity,
        condition_id: state_machine.condition_id.clone(),
        branch_id: branch.branch_id.clone(),
        condition_name: state_machine.condition_name.clone(),
        active_state: branch.active,
        acked_state: branch.acked,
        confirmed_state: branch.confirmed,
        retain: branch.retain,
    }
}
