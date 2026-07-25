//! Condition State Machine implementation.
//! Manages active alarms, EnabledState, ActiveState, AckedState, and ConfirmedState in the AddressSpace.

use crate::address_space::{AddressSpace, ObjectBuilder, VariableBuilder};
use opcua_nodes::NodeType;
use opcua_types::{
    DataTypeId, DateTime, LocalizedText, NodeId, ObjectTypeId, ReferenceTypeId, StatusCode,
    VariableTypeId, Variant, VariantScalarTypeId,
};
use std::sync::{Arc, Mutex};

/// Preserved prior state of an OPC UA Condition branch.
#[derive(Debug, Clone)]
pub struct Branch {
    /// Unique non-null BranchId for this preserved condition state.
    pub branch_id: NodeId,
    /// EventId used to acknowledge or confirm this branch.
    pub event_id: Vec<u8>,
    /// Preserved ActiveState value.
    pub active: bool,
    /// Preserved AckedState value.
    pub acked: bool,
    /// Preserved ConfirmedState value.
    pub confirmed: bool,
    /// Whether this branch is retained for refresh and operator action.
    pub retain: bool,
    /// Preserved Severity value.
    pub severity: u16,
    /// Preserved Message value.
    pub message: LocalizedText,
}

/// Current state of an AlarmCondition ShelvedStateMachineType instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelvingState {
    /// Alarm is not shelved.
    Unshelved,
    /// Alarm is shelved until it next becomes inactive.
    OneShotShelved,
    /// Alarm is shelved until its timer expires or it is explicitly unshelved.
    TimedShelved,
}

impl ShelvingState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unshelved => "Unshelved",
            Self::OneShotShelved => "OneShotShelved",
            Self::TimedShelved => "TimedShelved",
        }
    }
}

/// Manages an OPC-UA Alarm Condition instance and its state variables.
#[derive(Debug, Clone)]
pub struct ConditionStateMachine {
    /// NodeId of the condition instance.
    pub condition_id: NodeId,
    /// NodeId of the monitored source node.
    pub source_node_id: NodeId,
    /// Human-readable condition name.
    pub condition_name: String,
    /// NodeId of the EnabledState variable.
    pub enabled_state_id: NodeId,
    /// NodeId of the EnabledState.TransitionTime property.
    pub enabled_state_transition_time_id: NodeId,
    /// NodeId of the ActiveState variable.
    pub active_state_id: NodeId,
    /// NodeId of the ActiveState.TransitionTime property.
    pub active_state_transition_time_id: NodeId,
    /// NodeId of the ActiveState.EffectiveTransitionTime property.
    pub active_state_effective_transition_time_id: NodeId,
    /// NodeId of the ActiveState.EffectiveDisplayName property.
    pub active_state_effective_display_name_id: NodeId,
    /// NodeId of the AckedState variable.
    pub acked_state_id: NodeId,
    /// NodeId of the AckedState.TransitionTime property.
    pub acked_state_transition_time_id: NodeId,
    /// NodeId of the ConfirmedState variable.
    pub confirmed_state_id: NodeId,
    /// NodeId of the ConfirmedState.TransitionTime property.
    pub confirmed_state_transition_time_id: NodeId,
    /// NodeId of the Severity variable.
    pub severity_id: NodeId,
    /// NodeId of the Message variable.
    pub message_id: NodeId,
    /// NodeId of the Retain variable.
    pub retain_id: NodeId,
    /// NodeId of the BranchId property.
    pub branch_id_id: NodeId,
    /// NodeId of the SuppressedState variable.
    pub suppressed_state_id: NodeId,
    /// NodeId of the SuppressedState.TransitionTime property.
    pub suppressed_state_transition_time_id: NodeId,
    /// NodeId of the OutOfServiceState variable.
    pub out_of_service_state_id: NodeId,
    /// NodeId of the OutOfServiceState.TransitionTime property.
    pub out_of_service_state_transition_time_id: NodeId,
    /// NodeId of the SilenceState variable (OPC-10000-9 §5.8.2, optional on AlarmConditionType).
    pub silence_state_id: NodeId,
    /// NodeId of the SilenceState.TransitionTime property.
    pub silence_state_transition_time_id: NodeId,
    /// NodeId of the SuppressedOrShelved variable.
    pub suppressed_or_shelved_id: NodeId,
    /// NodeId of the ShelvingState object.
    pub shelving_state_id: NodeId,
    /// NodeId of the ShelvingState.CurrentState variable.
    pub shelving_current_state_id: NodeId,
    /// NodeId of the ShelvingState.CurrentState.TransitionTime property.
    pub shelving_current_state_transition_time_id: NodeId,
    /// NodeId of the ShelvingState.UnshelveTime property.
    pub unshelve_time_id: NodeId,
    /// NodeId of the AlarmConditionType OnDelay property (OPC-10000-9 §5.8.2, optional).
    pub on_delay_id: NodeId,
    /// NodeId of the AlarmConditionType OffDelay property (OPC-10000-9 §5.8.2, optional).
    pub off_delay_id: NodeId,
    /// NodeId of the AlarmConditionType ReAlarmTime property (OPC-10000-9 §5.8.2, optional).
    pub re_alarm_time_id: NodeId,
    /// NodeId of the AlarmConditionType ReAlarmRepeatCount variable (OPC-10000-9 §5.8.2,
    /// optional; server-maintained, not client-configured).
    pub re_alarm_repeat_count_id: NodeId,
    /// NodeId of the AlarmConditionType AudibleEnabled property (OPC-10000-9 §5.8.2, optional;
    /// server-computed from active/acked/silenced state, not client-configured).
    pub audible_enabled_id: NodeId,
    /// NodeId of the AlarmConditionType AudibleSound variable (OPC-10000-9 §5.8.2, optional).
    /// Modeled as a plain writable property here rather than the full `AudioVariableType`
    /// structure (`AudioDataType` has no generated Rust type in this codebase and its content is
    /// a client-side playback concern, not server-evaluated).
    pub audible_sound_id: NodeId,
    /// EventId of the condition's current (most recent) reportable state, shared across clones.
    /// Acknowledge/Confirm validate the client-supplied EventId against this (Part 9 §5.5.2).
    current_event_id: Arc<Mutex<Vec<u8>>>,
    /// Active condition branches, shared across clones.
    branches: Arc<opcua_core::sync::RwLock<Vec<Branch>>>,
    /// OnDelay/OffDelay hysteresis bookkeeping for `gate_active`, shared across clones.
    delay_gate: Arc<Mutex<DelayGateState>>,
    /// ReAlarmTime bookkeeping for `maybe_re_alarm`/`reset_re_alarm`, shared across clones.
    re_alarm: Arc<Mutex<ReAlarmState>>,
}

/// Bookkeeping for `ConditionStateMachine::gate_active`'s OnDelay/OffDelay hysteresis.
#[derive(Debug, Default)]
struct DelayGateState {
    committed: bool,
    pending: Option<(bool, DateTime)>,
}

/// Bookkeeping for `ConditionStateMachine::maybe_re_alarm`'s ReAlarmTime tracking (OPC-10000-9
/// §5.8.2): `None` while the alarm is not active; `Some(t)` records when it was last (re-)alarmed.
#[derive(Debug, Default)]
struct ReAlarmState {
    last_alarm_time: Option<DateTime>,
}

/// Creates a `PropertyType` child Variable (e.g. `TransitionTime`, `EffectiveTransitionTime`,
/// `EffectiveDisplayName`) attached to `parent_id` via a `HasProperty` reference, per the
/// pattern already used for `ShelvingState.UnshelveTime` (OPC-10000-9 §5.2).
fn insert_property_var(
    address_space: &AddressSpace,
    id: &NodeId,
    browse_name: &str,
    data_type: DataTypeId,
    value: Variant,
    parent_id: &NodeId,
) {
    let var = VariableBuilder::new(id, browse_name, browse_name)
        .data_type(data_type)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(value)
        .writable()
        .build();
    address_space.insert(
        var,
        Some(&[(
            parent_id,
            &NodeId::new(0, 46),
            opcua_nodes::ReferenceDirection::Inverse,
        )]),
    );
}

impl ConditionStateMachine {
    /// Creates and registers a new Alarm Condition state machine instance in the AddressSpace.
    pub fn create_in_address_space(
        address_space: &AddressSpace,
        device: &str,
        alarm_type: &str,
        source_node_id: NodeId,
        condition_name: &str,
    ) -> Self {
        let ns_idx = 2; // Dynamic namespace
        let base_s = format!("Alarm_{}_{}", device, alarm_type);

        let condition_id = NodeId::new(ns_idx, base_s.clone());
        let enabled_state_id = NodeId::new(ns_idx, format!("{}_EnabledState", base_s));
        let enabled_state_transition_time_id =
            NodeId::new(ns_idx, format!("{}_EnabledState_TransitionTime", base_s));
        let active_state_id = NodeId::new(ns_idx, format!("{}_ActiveState", base_s));
        let active_state_transition_time_id =
            NodeId::new(ns_idx, format!("{}_ActiveState_TransitionTime", base_s));
        let active_state_effective_transition_time_id = NodeId::new(
            ns_idx,
            format!("{}_ActiveState_EffectiveTransitionTime", base_s),
        );
        let active_state_effective_display_name_id = NodeId::new(
            ns_idx,
            format!("{}_ActiveState_EffectiveDisplayName", base_s),
        );
        let acked_state_id = NodeId::new(ns_idx, format!("{}_AckedState", base_s));
        let acked_state_transition_time_id =
            NodeId::new(ns_idx, format!("{}_AckedState_TransitionTime", base_s));
        let confirmed_state_id = NodeId::new(ns_idx, format!("{}_ConfirmedState", base_s));
        let confirmed_state_transition_time_id =
            NodeId::new(ns_idx, format!("{}_ConfirmedState_TransitionTime", base_s));
        let severity_id = NodeId::new(ns_idx, format!("{}_Severity", base_s));
        let message_id = NodeId::new(ns_idx, format!("{}_Message", base_s));
        let retain_id = NodeId::new(ns_idx, format!("{}_Retain", base_s));
        let branch_id_id = NodeId::new(ns_idx, format!("{}_BranchId", base_s));
        let suppressed_state_id = NodeId::new(ns_idx, format!("{}_SuppressedState", base_s));
        let suppressed_state_transition_time_id =
            NodeId::new(ns_idx, format!("{}_SuppressedState_TransitionTime", base_s));
        let out_of_service_state_id = NodeId::new(ns_idx, format!("{}_OutOfServiceState", base_s));
        let out_of_service_state_transition_time_id = NodeId::new(
            ns_idx,
            format!("{}_OutOfServiceState_TransitionTime", base_s),
        );
        let silence_state_id = NodeId::new(ns_idx, format!("{}_SilenceState", base_s));
        let silence_state_transition_time_id =
            NodeId::new(ns_idx, format!("{}_SilenceState_TransitionTime", base_s));
        let suppressed_or_shelved_id =
            NodeId::new(ns_idx, format!("{}_SuppressedOrShelved", base_s));
        let shelving_state_id = NodeId::new(ns_idx, format!("{}_ShelvingState", base_s));
        let shelving_current_state_id =
            NodeId::new(ns_idx, format!("{}_ShelvingState_CurrentState", base_s));
        let shelving_current_state_transition_time_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_CurrentState_TransitionTime", base_s),
        );
        let unshelve_time_id =
            NodeId::new(ns_idx, format!("{}_ShelvingState_UnshelveTime", base_s));
        let on_delay_id = NodeId::new(ns_idx, format!("{}_OnDelay", base_s));
        let off_delay_id = NodeId::new(ns_idx, format!("{}_OffDelay", base_s));
        let re_alarm_time_id = NodeId::new(ns_idx, format!("{}_ReAlarmTime", base_s));
        let re_alarm_repeat_count_id =
            NodeId::new(ns_idx, format!("{}_ReAlarmRepeatCount", base_s));
        let audible_enabled_id = NodeId::new(ns_idx, format!("{}_AudibleEnabled", base_s));
        let audible_sound_id = NodeId::new(ns_idx, format!("{}_AudibleSound", base_s));

        // 1. Create Condition Object (AlarmConditionType i=2915)
        let alarm_obj = ObjectBuilder::new(
            &condition_id,
            format!("Alarm_{}_{}", device, alarm_type),
            condition_name,
        )
        .has_type_definition(NodeId::new(0, 2915))
        .component_of(source_node_id.clone())
        .build();
        address_space.insert::<_, NodeId>(alarm_obj, None);

        // 2. Create EnabledState (TwoStateVariableType)
        let enabled_var = VariableBuilder::new(&enabled_state_id, "EnabledState", "EnabledState")
            .data_type(opcua_types::DataTypeId::Boolean)
            .value(true) // Enabled by default
            .writable()
            .build();
        address_space.insert(
            enabled_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &enabled_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &enabled_state_id,
        );

        // 3. Create ActiveState (TwoStateVariableType)
        let active_var = VariableBuilder::new(&active_state_id, "ActiveState", "ActiveState")
            .data_type(opcua_types::DataTypeId::Boolean)
            .value(false) // Inactive by default
            .writable()
            .build();
        address_space.insert(
            active_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &active_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &active_state_id,
        );
        insert_property_var(
            address_space,
            &active_state_effective_transition_time_id,
            "EffectiveTransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &active_state_id,
        );
        insert_property_var(
            address_space,
            &active_state_effective_display_name_id,
            "EffectiveDisplayName",
            DataTypeId::LocalizedText,
            Variant::from(LocalizedText::new("en", "Inactive")),
            &active_state_id,
        );

        // 4. Create AckedState (TwoStateVariableType)
        let acked_var = VariableBuilder::new(&acked_state_id, "AckedState", "AckedState")
            .data_type(opcua_types::DataTypeId::Boolean)
            .value(false) // Unacknowledged by default
            .writable()
            .build();
        address_space.insert(
            acked_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &acked_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &acked_state_id,
        );

        // 5. Create ConfirmedState (TwoStateVariableType)
        let confirmed_var =
            VariableBuilder::new(&confirmed_state_id, "ConfirmedState", "ConfirmedState")
                .data_type(opcua_types::DataTypeId::Boolean)
                .value(false) // Unconfirmed by default
                .writable()
                .build();
        address_space.insert(
            confirmed_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &confirmed_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &confirmed_state_id,
        );

        // 6. Create Severity variable
        let severity_var = VariableBuilder::new(&severity_id, "Severity", "Severity")
            .data_type(opcua_types::DataTypeId::UInt16)
            .value(100u16)
            .writable()
            .build();
        address_space.insert(
            severity_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // 7. Create Message variable
        let message_var = VariableBuilder::new(&message_id, "Message", "Message")
            .data_type(opcua_types::DataTypeId::LocalizedText)
            .value(LocalizedText::new("en", "Normal operating state"))
            .writable()
            .build();
        address_space.insert(
            message_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // 8. Create Retain variable
        let retain_var = VariableBuilder::new(&retain_id, "Retain", "Retain")
            .data_type(opcua_types::DataTypeId::Boolean)
            .value(false)
            .writable()
            .build();
        address_space.insert(
            retain_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let branch_id_var = VariableBuilder::new(&branch_id_id, "BranchId", "BranchId")
            .data_type(DataTypeId::NodeId)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(NodeId::null())
            .writable()
            .build();
        address_space.insert(
            branch_id_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // 9. Create display-suppression state nodes.
        let suppressed_var =
            VariableBuilder::new(&suppressed_state_id, "SuppressedState", "SuppressedState")
                .data_type(DataTypeId::Boolean)
                .has_type_definition(VariableTypeId::TwoStateVariableType)
                .value(false)
                .writable()
                .build();
        address_space.insert(
            suppressed_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &suppressed_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &suppressed_state_id,
        );

        let out_of_service_var = VariableBuilder::new(
            &out_of_service_state_id,
            "OutOfServiceState",
            "OutOfServiceState",
        )
        .data_type(DataTypeId::Boolean)
        .has_type_definition(VariableTypeId::TwoStateVariableType)
        .value(false)
        .writable()
        .build();
        address_space.insert(
            out_of_service_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &out_of_service_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &out_of_service_state_id,
        );

        // SilenceState (OPC-10000-9 §5.8.2/§5.8.7, optional on AlarmConditionType) — created
        // unconditionally for every condition, matching this module's existing treatment of
        // SuppressedState/OutOfServiceState.
        let silence_var = VariableBuilder::new(&silence_state_id, "SilenceState", "SilenceState")
            .data_type(DataTypeId::Boolean)
            .has_type_definition(VariableTypeId::TwoStateVariableType)
            .value(false)
            .writable()
            .build();
        address_space.insert(
            silence_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &silence_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &silence_state_id,
        );

        let suppressed_or_shelved_var = VariableBuilder::new(
            &suppressed_or_shelved_id,
            "SuppressedOrShelved",
            "SuppressedOrShelved",
        )
        .data_type(DataTypeId::Boolean)
        .value(false)
        .writable()
        .build();
        address_space.insert(
            suppressed_or_shelved_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let shelving_obj = ObjectBuilder::new(&shelving_state_id, "ShelvingState", "ShelvingState")
            .has_type_definition(NodeId::new(0, 2929))
            .build();
        address_space.insert(
            shelving_obj,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        address_space.insert_reference(
            &shelving_state_id,
            &NodeId::from(ObjectTypeId::TransitionEventType),
            ReferenceTypeId::GeneratesEvent,
        );

        let shelving_current_state_var =
            VariableBuilder::new(&shelving_current_state_id, "CurrentState", "CurrentState")
                .data_type(DataTypeId::LocalizedText)
                .has_type_definition(VariableTypeId::StateVariableType)
                .value(LocalizedText::new("en", ShelvingState::Unshelved.as_str()))
                .writable()
                .build();
        address_space.insert(
            shelving_current_state_var,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );
        insert_property_var(
            address_space,
            &shelving_current_state_transition_time_id,
            "TransitionTime",
            DataTypeId::DateTime,
            Variant::from(DateTime::now()),
            &shelving_current_state_id,
        );

        let unshelve_time_var =
            VariableBuilder::new(&unshelve_time_id, "UnshelveTime", "UnshelveTime")
                .data_type(DataTypeId::Double)
                .has_type_definition(VariableTypeId::PropertyType)
                .value(0.0f64)
                .writable()
                .build();
        address_space.insert(
            unshelve_time_var,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // Shelving state sub-objects
        let unshelved_state_id = NodeId::new(ns_idx, format!("{}_ShelvingState_Unshelved", base_s));
        let timed_shelved_state_id =
            NodeId::new(ns_idx, format!("{}_ShelvingState_TimedShelved", base_s));
        let one_shot_shelved_state_id =
            NodeId::new(ns_idx, format!("{}_ShelvingState_OneShotShelved", base_s));

        let unshelved_obj = ObjectBuilder::new(&unshelved_state_id, "Unshelved", "Unshelved")
            .has_type_definition(NodeId::from(ObjectTypeId::StateType))
            .build();
        address_space.insert(
            unshelved_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let timed_shelved_obj =
            ObjectBuilder::new(&timed_shelved_state_id, "TimedShelved", "TimedShelved")
                .has_type_definition(NodeId::from(ObjectTypeId::StateType))
                .build();
        address_space.insert(
            timed_shelved_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let one_shot_shelved_obj = ObjectBuilder::new(
            &one_shot_shelved_state_id,
            "OneShotShelved",
            "OneShotShelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::StateType))
        .build();
        address_space.insert(
            one_shot_shelved_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // Shelving transition sub-objects
        let utots_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_UnshelvedToTimedShelved", base_s),
        );
        let utoos_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_UnshelvedToOneShotShelved", base_s),
        );
        let ttou_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_TimedShelvedToUnshelved", base_s),
        );
        let ttoos_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_TimedShelvedToOneShotShelved", base_s),
        );
        let ostou_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_OneShotShelvedToUnshelved", base_s),
        );
        let ostots_id = NodeId::new(
            ns_idx,
            format!("{}_ShelvingState_OneShotShelvedToTimedShelved", base_s),
        );

        let utots_obj = ObjectBuilder::new(
            &utots_id,
            "UnshelvedToTimedShelved",
            "UnshelvedToTimedShelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::TransitionType))
        .build();
        address_space.insert(
            utots_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let utoos_obj = ObjectBuilder::new(
            &utoos_id,
            "UnshelvedToOneShotShelved",
            "UnshelvedToOneShotShelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::TransitionType))
        .build();
        address_space.insert(
            utoos_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let ttou_obj = ObjectBuilder::new(
            &ttou_id,
            "TimedShelvedToUnshelved",
            "TimedShelvedToUnshelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::TransitionType))
        .build();
        address_space.insert(
            ttou_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let ttoos_obj = ObjectBuilder::new(
            &ttoos_id,
            "TimedShelvedToOneShotShelved",
            "TimedShelvedToOneShotShelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::TransitionType))
        .build();
        address_space.insert(
            ttoos_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let ostou_obj = ObjectBuilder::new(
            &ostou_id,
            "OneShotShelvedToUnshelved",
            "OneShotShelvedToUnshelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::TransitionType))
        .build();
        address_space.insert(
            ostou_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let ostots_obj = ObjectBuilder::new(
            &ostots_id,
            "OneShotShelvedToTimedShelved",
            "OneShotShelvedToTimedShelved",
        )
        .has_type_definition(NodeId::from(ObjectTypeId::TransitionType))
        .build();
        address_space.insert(
            ostots_obj,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // AvailableStates Property
        let shelving_available_states_id =
            NodeId::new(ns_idx, format!("{}_Shelving_AvailableStates", base_s));
        let shelving_state_ids: Vec<NodeId> = vec![
            unshelved_state_id,
            timed_shelved_state_id,
            one_shot_shelved_state_id,
        ];
        let shelving_as_var = VariableBuilder::new(
            &shelving_available_states_id,
            "AvailableStates",
            "AvailableStates",
        )
        .data_type(DataTypeId::NodeId)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(Variant::from((
            VariantScalarTypeId::NodeId,
            shelving_state_ids,
        )))
        .build();
        address_space.insert(
            shelving_as_var,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // AvailableTransitions Property
        let shelving_available_transitions_id =
            NodeId::new(ns_idx, format!("{}_Shelving_AvailableTransitions", base_s));
        let shelving_transition_ids: Vec<NodeId> =
            vec![utots_id, utoos_id, ttou_id, ttoos_id, ostou_id, ostots_id];
        let shelving_at_var = VariableBuilder::new(
            &shelving_available_transitions_id,
            "AvailableTransitions",
            "AvailableTransitions",
        )
        .data_type(DataTypeId::NodeId)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(Variant::from((
            VariantScalarTypeId::NodeId,
            shelving_transition_ids,
        )))
        .build();
        address_space.insert(
            shelving_at_var,
            Some(&[(
                &shelving_state_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // OnDelay/OffDelay (OPC-10000-9 §5.8.2, optional) -- created unconditionally for every
        // condition, matching this module's existing treatment of SuppressedState/SilenceState.
        // Their live values are read by `gate_active`'s callers, not re-read from these address
        // space nodes; the nodes exist for browsability/structural conformance.
        let on_delay_var = VariableBuilder::new(&on_delay_id, "OnDelay", "OnDelay")
            .data_type(DataTypeId::Duration)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(0.0f64)
            .writable()
            .build();
        address_space.insert(
            on_delay_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let off_delay_var = VariableBuilder::new(&off_delay_id, "OffDelay", "OffDelay")
            .data_type(DataTypeId::Duration)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(0.0f64)
            .writable()
            .build();
        address_space.insert(
            off_delay_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // ReAlarmTime/ReAlarmRepeatCount (OPC-10000-9 §5.8.2, optional) -- created
        // unconditionally, same rationale as OnDelay/OffDelay above. ReAlarmRepeatCount is
        // server-maintained (Int16, BaseDataVariableType), not client-configured.
        let re_alarm_time_var =
            VariableBuilder::new(&re_alarm_time_id, "ReAlarmTime", "ReAlarmTime")
                .data_type(DataTypeId::Duration)
                .has_type_definition(VariableTypeId::PropertyType)
                .value(0.0f64)
                .writable()
                .build();
        address_space.insert(
            re_alarm_time_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let re_alarm_repeat_count_var = VariableBuilder::new(
            &re_alarm_repeat_count_id,
            "ReAlarmRepeatCount",
            "ReAlarmRepeatCount",
        )
        .data_type(DataTypeId::Int16)
        .value(0i16)
        .writable()
        .build();
        address_space.insert(
            re_alarm_repeat_count_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        // AudibleEnabled/AudibleSound (OPC-10000-9 §5.8.2, optional) -- created unconditionally,
        // same rationale as OnDelay/OffDelay above. AudibleEnabled is server-computed (see
        // `recompute_audible_enabled`), not client-configured.
        let audible_enabled_var =
            VariableBuilder::new(&audible_enabled_id, "AudibleEnabled", "AudibleEnabled")
                .data_type(DataTypeId::Boolean)
                .has_type_definition(VariableTypeId::PropertyType)
                .value(false)
                .writable()
                .build();
        address_space.insert(
            audible_enabled_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 46),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        let audible_sound_var =
            VariableBuilder::new(&audible_sound_id, "AudibleSound", "AudibleSound")
                .data_type(DataTypeId::LocalizedText)
                .value(LocalizedText::null())
                .writable()
                .build();
        address_space.insert(
            audible_sound_var,
            Some(&[(
                &condition_id,
                &NodeId::new(0, 47),
                opcua_nodes::ReferenceDirection::Inverse,
            )]),
        );

        Self {
            condition_id,
            source_node_id,
            condition_name: condition_name.to_string(),
            enabled_state_id,
            enabled_state_transition_time_id,
            active_state_id,
            active_state_transition_time_id,
            active_state_effective_transition_time_id,
            active_state_effective_display_name_id,
            acked_state_id,
            acked_state_transition_time_id,
            confirmed_state_id,
            confirmed_state_transition_time_id,
            severity_id,
            message_id,
            retain_id,
            branch_id_id,
            suppressed_state_id,
            suppressed_state_transition_time_id,
            out_of_service_state_id,
            out_of_service_state_transition_time_id,
            silence_state_id,
            silence_state_transition_time_id,
            suppressed_or_shelved_id,
            shelving_state_id,
            shelving_current_state_id,
            shelving_current_state_transition_time_id,
            unshelve_time_id,
            on_delay_id,
            off_delay_id,
            re_alarm_time_id,
            re_alarm_repeat_count_id,
            audible_enabled_id,
            audible_sound_id,
            current_event_id: Arc::new(Mutex::new(Vec::new())),
            branches: Arc::new(opcua_core::sync::RwLock::new(Vec::new())),
            delay_gate: Arc::new(Mutex::new(DelayGateState::default())),
            re_alarm: Arc::new(Mutex::new(ReAlarmState::default())),
        }
    }

    /// Records the EventId of the most recent reportable state (set when an event is generated).
    pub fn set_current_event_id(&self, event_id: &[u8]) {
        *self.current_event_id.lock().unwrap() = event_id.to_vec();
    }

    /// Whether `event_id` matches the condition's current reportable EventId. A condition that has not
    /// yet emitted an event (empty) matches nothing — Acknowledge/Confirm then fail Bad_EventIdUnknown.
    pub fn current_event_id_matches(&self, event_id: &[u8]) -> bool {
        let current = self.current_event_id.lock().unwrap();
        !current.is_empty() && current.as_slice() == event_id
    }

    /// Returns the EventId of the condition's current reportable state.
    pub fn current_event_id(&self) -> Vec<u8> {
        self.current_event_id.lock().unwrap().clone()
    }

    /// Creates a branch by snapshotting the condition's current trunk state.
    pub fn create_branch(&self, address_space: &AddressSpace) -> NodeId {
        let branch_id = NodeId::new(
            2,
            format!(
                "{}_Branch_{}",
                self.condition_id,
                uuid::Uuid::new_v4().simple()
            ),
        );
        let branch = Branch {
            branch_id: branch_id.clone(),
            event_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
            active: self.get_active(address_space),
            acked: self.get_acked(address_space),
            confirmed: self.get_confirmed(address_space),
            retain: true,
            severity: self.get_severity(address_space),
            message: self.get_message(address_space),
        };
        self.branches.write().push(branch);
        branch_id
    }

    /// Returns a snapshot of all condition branches.
    pub fn branches(&self) -> Vec<Branch> {
        self.branches.read().clone()
    }

    /// Finds a branch by its EventId.
    pub fn branch_by_event_id(&self, event_id: &[u8]) -> Option<Branch> {
        self.branches
            .read()
            .iter()
            .find(|branch| branch.event_id.as_slice() == event_id)
            .cloned()
    }

    /// Acknowledges a branch by EventId.
    pub fn ack_branch(&self, event_id: &[u8]) -> bool {
        self.update_branch_ack_state(event_id, true, false)
    }

    /// Confirms a branch by EventId.
    pub fn confirm_branch(&self, event_id: &[u8]) -> bool {
        self.update_branch_ack_state(event_id, false, true)
    }

    /// Returns retained condition branches.
    pub fn retained_branches(&self) -> Vec<Branch> {
        self.branches
            .read()
            .iter()
            .filter(|branch| branch.retain)
            .cloned()
            .collect()
    }

    /// Gets whether the condition is enabled.
    pub fn get_enabled(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.enabled_state_id)
    }

    /// Sets whether the condition is enabled.
    pub fn set_enabled(&self, address_space: &AddressSpace, enabled: bool) {
        self.set_bool_value(address_space, &self.enabled_state_id, enabled);
        self.write_transition_time(address_space, &self.enabled_state_transition_time_id);
    }

    /// Gets whether the condition is active.
    pub fn get_active(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.active_state_id)
    }

    /// Sets whether the condition is active.
    pub fn set_active(&self, address_space: &AddressSpace, active: bool) {
        self.set_bool_value(address_space, &self.active_state_id, active);
        self.write_transition_time(address_space, &self.active_state_transition_time_id);
        self.recompute_effective_state(address_space);
        self.recompute_audible_enabled(address_space);
    }

    /// Applies OnDelay/OffDelay hysteresis (OPC-10000-9 §5.8.2) to a desired ActiveState
    /// transition evaluated at `now`, committing (writing) `ActiveState` via `set_active` only
    /// once the configured delay has elapsed since `desired` was first observed at a differing
    /// value from what's committed. Returns `Some(active)` when a transition is committed this
    /// call, `None` when `desired` already matches the committed state or the delay has not yet
    /// elapsed. Zero delays commit immediately, matching plain `set_active`.
    pub fn gate_active(
        &self,
        address_space: &AddressSpace,
        desired: bool,
        now: DateTime,
        on_delay_ms: f64,
        off_delay_ms: f64,
    ) -> Option<bool> {
        let commit = {
            let mut state = self.delay_gate.lock().unwrap();
            if desired == state.committed {
                state.pending = None;
                return None;
            }

            let started = match state.pending {
                Some((pending_desired, started)) if pending_desired == desired => started,
                _ => {
                    state.pending = Some((desired, now));
                    now
                }
            };

            let delay_ms = if desired { on_delay_ms } else { off_delay_ms };
            let elapsed_ms = (now.checked_ticks() - started.checked_ticks()) as f64 / 10_000.0;
            if elapsed_ms < delay_ms {
                return None;
            }

            state.committed = desired;
            state.pending = None;
            desired
        };

        self.set_active(address_space, commit);
        Some(commit)
    }

    /// (Re)initializes ReAlarmTime bookkeeping (OPC-10000-9 §5.8.2) after an ActiveState
    /// transition commits: a fresh activation starts the ReAlarm timer at `now` and resets
    /// `ReAlarmRepeatCount` to 0; a deactivation (return to normal) stops the timer and resets
    /// `ReAlarmRepeatCount` to 0 ("the count is reset when an Alarm returns to normal").
    pub fn reset_re_alarm(&self, address_space: &AddressSpace, active: bool, now: DateTime) {
        {
            let mut state = self.re_alarm.lock().unwrap();
            state.last_alarm_time = if active { Some(now) } else { None };
        }
        self.set_i16_value(address_space, &self.re_alarm_repeat_count_id, 0);
    }

    /// Checks whether `re_alarm_ms` has elapsed since this (still active, unacknowledged) alarm
    /// was last (re-)alarmed at `now`; if so, re-alarms it per OPC-10000-9 §5.8.2/Annex B.1.5:
    /// increments `ReAlarmRepeatCount`, returns `AckedState`/`SilenceState` to unacknowledged/
    /// un-silenced, stamps `ActiveState.TransitionTime` ("the Alarm active time is set to the
    /// time of the re-alarm"), and resets the timer to `now`. Returns whether a re-alarm
    /// occurred this call; a disabled timer (`re_alarm_ms <= 0.0`) or one not currently tracked
    /// as active never re-alarms.
    pub fn maybe_re_alarm(
        &self,
        address_space: &AddressSpace,
        now: DateTime,
        re_alarm_ms: f64,
    ) -> bool {
        if re_alarm_ms <= 0.0 {
            return false;
        }

        let started = match self.re_alarm.lock().unwrap().last_alarm_time {
            Some(t) => t,
            None => return false,
        };

        let elapsed_ms = (now.checked_ticks() - started.checked_ticks()) as f64 / 10_000.0;
        if elapsed_ms < re_alarm_ms {
            return false;
        }

        self.re_alarm.lock().unwrap().last_alarm_time = Some(now);
        let count = self.get_i16_value(address_space, &self.re_alarm_repeat_count_id);
        self.set_i16_value(
            address_space,
            &self.re_alarm_repeat_count_id,
            count.saturating_add(1),
        );
        // "The Server will generate a new Alarm for it (as if it just went into alarm)":
        // Acked/Confirmed both return to fresh-alarm (false), matching an initial activation.
        self.set_acked(address_space, false);
        self.set_confirmed(address_space, false);
        self.set_silenced(address_space, false);
        self.write_transition_time(address_space, &self.active_state_transition_time_id);
        true
    }

    /// Gets whether the condition is acknowledged.
    pub fn get_acked(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.acked_state_id)
    }

    /// Sets whether the condition is acknowledged.
    pub fn set_acked(&self, address_space: &AddressSpace, acked: bool) {
        self.set_bool_value(address_space, &self.acked_state_id, acked);
        self.write_transition_time(address_space, &self.acked_state_transition_time_id);
        self.recompute_effective_state(address_space);
        self.recompute_audible_enabled(address_space);
    }

    /// Gets whether the condition is confirmed.
    pub fn get_confirmed(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.confirmed_state_id)
    }

    /// Sets whether the condition is confirmed.
    pub fn set_confirmed(&self, address_space: &AddressSpace, confirmed: bool) {
        self.set_bool_value(address_space, &self.confirmed_state_id, confirmed);
        self.write_transition_time(address_space, &self.confirmed_state_transition_time_id);
    }

    /// Gets the current severity of the condition.
    pub fn get_severity(&self, address_space: &AddressSpace) -> u16 {
        if let Some(node) = address_space.find(&self.severity_id) {
            if let NodeType::Variable(ref var) = *node {
                if let Some(Variant::UInt16(v)) = var
                    .value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                {
                    return v;
                }
            }
        };
        0
    }

    /// Sets the current severity of the condition.
    pub fn set_severity(&self, address_space: &AddressSpace, severity: u16) {
        if let Some(mut node) = address_space.find_mut(&self.severity_id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(&opcua_types::NumericRange::None, Variant::from(severity));
            }
        };
    }

    /// Gets the current localized message of the condition.
    pub fn get_message(&self, address_space: &AddressSpace) -> LocalizedText {
        if let Some(node) = address_space.find(&self.message_id) {
            if let NodeType::Variable(ref var) = *node {
                if let Some(Variant::LocalizedText(ref t)) = var
                    .value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                {
                    return (**t).clone();
                }
            }
        };
        LocalizedText::null()
    }

    /// Sets the current localized message of the condition.
    pub fn set_message(&self, address_space: &AddressSpace, message: LocalizedText) {
        if let Some(mut node) = address_space.find_mut(&self.message_id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(&opcua_types::NumericRange::None, Variant::from(message));
            }
        };
    }

    /// Gets whether the condition is retained.
    pub fn get_retain(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.retain_id)
    }

    /// Sets whether the condition is retained.
    pub fn set_retain(&self, address_space: &AddressSpace, retain: bool) {
        self.set_bool_value(address_space, &self.retain_id, retain);
    }

    /// Gets whether the condition is system-suppressed.
    pub fn get_suppressed(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.suppressed_state_id)
    }

    /// Sets whether the condition is system-suppressed.
    pub fn set_suppressed(&self, address_space: &AddressSpace, suppressed: bool) {
        self.set_bool_value(address_space, &self.suppressed_state_id, suppressed);
        self.write_transition_time(address_space, &self.suppressed_state_transition_time_id);
        self.recompute_suppressed_or_shelved(address_space);
    }

    /// Gets whether the condition is maintenance-suppressed.
    pub fn get_out_of_service(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.out_of_service_state_id)
    }

    /// Sets whether the condition is maintenance-suppressed.
    pub fn set_out_of_service(&self, address_space: &AddressSpace, out_of_service: bool) {
        self.set_bool_value(address_space, &self.out_of_service_state_id, out_of_service);
        self.write_transition_time(address_space, &self.out_of_service_state_transition_time_id);
        self.recompute_suppressed_or_shelved(address_space);
    }

    /// Gets whether the alarm's audible/visible indicator is silenced (OPC-10000-9 §5.8.7).
    pub fn get_silenced(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.silence_state_id)
    }

    /// Sets whether the alarm's audible/visible indicator is silenced.
    pub fn set_silenced(&self, address_space: &AddressSpace, silenced: bool) {
        self.set_bool_value(address_space, &self.silence_state_id, silenced);
        self.write_transition_time(address_space, &self.silence_state_transition_time_id);
        self.recompute_audible_enabled(address_space);
    }

    /// Recomputes `AudibleEnabled` (OPC-10000-9 §5.8.2): true only while the alarm is active,
    /// unacknowledged, and not silenced ("this file would be play/generated as long as the Alarm
    /// is active and unacknowledged, unless the silence StateMachine ... silences it").
    fn recompute_audible_enabled(&self, address_space: &AddressSpace) {
        let audible = self.get_active(address_space)
            && !self.get_acked(address_space)
            && !self.get_silenced(address_space);
        self.set_bool_value(address_space, &self.audible_enabled_id, audible);
    }

    /// Gets whether the condition is suppressed or shelved.
    pub fn get_suppressed_or_shelved(&self, address_space: &AddressSpace) -> bool {
        self.get_bool_value(address_space, &self.suppressed_or_shelved_id)
    }

    /// Gets the current shelving state.
    pub fn get_shelving_state(&self, address_space: &AddressSpace) -> ShelvingState {
        if let Some(node) = address_space.find(&self.shelving_current_state_id) {
            if let NodeType::Variable(ref var) = *node {
                if let Some(Variant::LocalizedText(ref text)) = var
                    .value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                {
                    return match text.text.value().as_deref() {
                        Some("OneShotShelved") => ShelvingState::OneShotShelved,
                        Some("TimedShelved") => ShelvingState::TimedShelved,
                        _ => ShelvingState::Unshelved,
                    };
                }
            }
        };
        ShelvingState::Unshelved
    }

    /// Sets the current shelving state.
    pub fn set_shelving_state(&self, address_space: &AddressSpace, state: ShelvingState) {
        if let Some(mut node) = address_space.find_mut(&self.shelving_current_state_id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(
                    &opcua_types::NumericRange::None,
                    Variant::from(LocalizedText::new("en", state.as_str())),
                );
            }
        };
        self.write_transition_time(
            address_space,
            &self.shelving_current_state_transition_time_id,
        );
        self.recompute_suppressed_or_shelved(address_space);
    }

    /// Gets the remaining timed-shelve duration in milliseconds.
    pub fn get_unshelve_time(&self, address_space: &AddressSpace) -> f64 {
        if let Some(node) = address_space.find(&self.unshelve_time_id) {
            if let NodeType::Variable(ref var) = *node {
                if let Some(Variant::Double(v)) = var
                    .value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                {
                    return v;
                }
            }
        };
        0.0
    }

    /// Sets the remaining timed-shelve duration in milliseconds.
    pub fn set_unshelve_time(&self, address_space: &AddressSpace, unshelve_time_ms: f64) {
        if let Some(mut node) = address_space.find_mut(&self.unshelve_time_id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(
                    &opcua_types::NumericRange::None,
                    Variant::from(unshelve_time_ms),
                );
            }
        };
    }

    /// Recomputes SuppressedOrShelved from suppression and shelving state.
    pub fn recompute_suppressed_or_shelved(&self, address_space: &AddressSpace) {
        let suppressed_or_shelved = self.get_suppressed(address_space)
            || self.get_out_of_service(address_space)
            || self.get_shelving_state(address_space) != ShelvingState::Unshelved;
        self.set_bool_value(
            address_space,
            &self.suppressed_or_shelved_id,
            suppressed_or_shelved,
        );
        self.recompute_effective_state(address_space);
    }

    /// Shelves the condition until the alarm next goes inactive.
    pub fn one_shot_shelve(&self, address_space: &AddressSpace) -> StatusCode {
        if self.get_shelving_state(address_space) == ShelvingState::OneShotShelved {
            return StatusCode::BadConditionAlreadyShelved;
        }

        self.set_shelving_state(address_space, ShelvingState::OneShotShelved);
        self.set_unshelve_time(address_space, 0.0);
        self.recompute_suppressed_or_shelved(address_space);
        StatusCode::Good
    }

    /// Shelves the condition for the supplied duration in milliseconds.
    pub fn timed_shelve(&self, address_space: &AddressSpace, shelving_time_ms: f64) -> StatusCode {
        if shelving_time_ms <= 0.0 {
            return StatusCode::BadShelvingTimeOutOfRange;
        }
        if self.get_shelving_state(address_space) == ShelvingState::TimedShelved {
            return StatusCode::BadConditionAlreadyShelved;
        }

        self.set_shelving_state(address_space, ShelvingState::TimedShelved);
        self.set_unshelve_time(address_space, shelving_time_ms);
        self.recompute_suppressed_or_shelved(address_space);
        StatusCode::Good
    }

    /// Returns a shelved condition to Unshelved.
    pub fn unshelve(&self, address_space: &AddressSpace) -> StatusCode {
        if self.get_shelving_state(address_space) == ShelvingState::Unshelved {
            return StatusCode::BadConditionNotShelved;
        }

        self.set_shelving_state(address_space, ShelvingState::Unshelved);
        self.set_unshelve_time(address_space, 0.0);
        self.recompute_suppressed_or_shelved(address_space);
        StatusCode::Good
    }

    /// Writes `TransitionTime`/`EffectiveTransitionTime` (OPC-10000-9 §5.2) on the property node
    /// identified by `id`, stamped with the current time. `pub(crate)` so sibling alarm-kind
    /// modules (`limit.rs`, `discrete.rs`) can stamp `TransitionTime` on their own state-machine
    /// child nodes without duplicating this write pattern.
    pub(crate) fn write_transition_time(&self, address_space: &AddressSpace, id: &NodeId) {
        if let Some(mut node) = address_space.find_mut(id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(
                    &opcua_types::NumericRange::None,
                    Variant::from(DateTime::now()),
                );
            }
        };
    }

    /// Writes `EffectiveDisplayName` (OPC-10000-9 §5.2) with a computed, locale-aware description
    /// of the condition's current effective state.
    fn write_effective_display_name(&self, address_space: &AddressSpace, text: &str) {
        if let Some(mut node) = address_space.find_mut(&self.active_state_effective_display_name_id)
        {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(
                    &opcua_types::NumericRange::None,
                    Variant::from(LocalizedText::new("en", text)),
                );
            }
        };
    }

    /// Recomputes and writes `ActiveState.EffectiveTransitionTime` and `EffectiveDisplayName`
    /// (OPC-10000-9 §5.2) from the condition's current active/acked/shelving/suppression state.
    /// Called whenever any of those inputs change.
    pub fn recompute_effective_state(&self, address_space: &AddressSpace) {
        let active = self.get_active(address_space);
        let acked = self.get_acked(address_space);
        let shelved = self.get_shelving_state(address_space) != ShelvingState::Unshelved;
        let suppressed =
            self.get_suppressed(address_space) || self.get_out_of_service(address_space);

        let text = match (active, shelved, suppressed, acked) {
            (false, _, _, _) => "Inactive",
            (true, true, _, _) => "Shelved",
            (true, _, true, _) => "Suppressed",
            (true, false, false, false) => "Active | Unacknowledged",
            (true, false, false, true) => "Active | Acknowledged",
        };

        self.write_transition_time(
            address_space,
            &self.active_state_effective_transition_time_id,
        );
        self.write_effective_display_name(address_space, text);
    }

    fn get_bool_value(&self, address_space: &AddressSpace, id: &NodeId) -> bool {
        if let Some(node) = address_space.find(id) {
            if let NodeType::Variable(ref var) = *node {
                if let Some(Variant::Boolean(b)) = var
                    .value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                {
                    return b;
                }
            }
        };
        false
    }

    fn set_bool_value(&self, address_space: &AddressSpace, id: &NodeId, value: bool) {
        if let Some(mut node) = address_space.find_mut(id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(&opcua_types::NumericRange::None, Variant::from(value));
            }
        };
    }

    fn get_i16_value(&self, address_space: &AddressSpace, id: &NodeId) -> i16 {
        if let Some(node) = address_space.find(id) {
            if let NodeType::Variable(ref var) = *node {
                if let Some(Variant::Int16(v)) = var
                    .value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                {
                    return v;
                }
            }
        };
        0
    }

    fn set_i16_value(&self, address_space: &AddressSpace, id: &NodeId, value: i16) {
        if let Some(mut node) = address_space.find_mut(id) {
            if let NodeType::Variable(ref mut var) = &mut *node {
                let _ = var.set_value(&opcua_types::NumericRange::None, Variant::from(value));
            }
        };
    }

    fn update_branch_ack_state(&self, event_id: &[u8], acked: bool, confirmed: bool) -> bool {
        let mut branches = self.branches.write();
        let Some(index) = branches
            .iter()
            .position(|branch| branch.event_id.as_slice() == event_id)
        else {
            return false;
        };

        let branch = &mut branches[index];
        if acked {
            branch.acked = true;
        }
        if confirmed {
            branch.confirmed = true;
        }
        // A branch is a preserved PRIOR state: it is dropped once the operator has both
        // acknowledged and confirmed it. Its `active` is historical and (unlike the trunk, where an
        // active condition is always retained) does not keep the branch retained.
        branch.retain = !branch.acked || !branch.confirmed;

        if !branch.retain {
            branches.remove(index);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opcua_nodes::DefaultTypeTree;
    use opcua_types::{BrowseDirection, ObjectTypeId, ReferenceTypeId};

    #[test]
    fn shelving_state_machine_exposes_generates_event_reference_to_transition_event_type() {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 2);
        let _condition = ConditionStateMachine::create_in_address_space(
            &address_space,
            "Dev",
            "TestAlarm",
            NodeId::new(2, "Source"),
            "TestAlarm",
        );
        let shelving_state_id = NodeId::new(2, "Alarm_Dev_TestAlarm_ShelvingState");
        let tree = DefaultTypeTree::default();
        let refs = address_space.find_references(
            &shelving_state_id,
            Some((NodeId::from(ReferenceTypeId::GeneratesEvent), false)),
            &tree,
            BrowseDirection::Forward,
        );
        assert!(refs
            .iter()
            .any(|r| r.target_id == ObjectTypeId::TransitionEventType));
    }

    #[test]
    fn shelving_state_machine_available_states_and_transitions_are_populated() {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 2);
        let _condition = ConditionStateMachine::create_in_address_space(
            &address_space,
            "Dev",
            "TestAlarm",
            NodeId::new(2, "Source"),
            "TestAlarm",
        );
        let as_id = NodeId::new(2, "Alarm_Dev_TestAlarm_Shelving_AvailableStates");
        let at_id = NodeId::new(2, "Alarm_Dev_TestAlarm_Shelving_AvailableTransitions");
        let as_val = address_space
            .find(&as_id)
            .and_then(|n| {
                if let NodeType::Variable(ref v) = *n {
                    v.value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                    .clone()
                } else {
                    None
                }
            })
            .unwrap_or(Variant::Empty);
        let at_val = address_space
            .find(&at_id)
            .and_then(|n| {
                if let NodeType::Variable(ref v) = *n {
                    v.value(
                        opcua_types::TimestampsToReturn::Neither,
                        &opcua_types::NumericRange::None,
                        &opcua_types::DataEncoding::Binary,
                        0.0,
                    )
                    .value
                    .clone()
                } else {
                    None
                }
            })
            .unwrap_or(Variant::Empty);
        assert!(
            !matches!(as_val, Variant::Empty),
            "AvailableStates should be populated"
        );
        assert!(
            !matches!(at_val, Variant::Empty),
            "AvailableTransitions should be populated"
        );
    }
}
