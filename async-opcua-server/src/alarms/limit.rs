//! Limit-alarm evaluation and address-space wiring for ExclusiveLimitAlarmType and
//! NonExclusiveLimitAlarmType.

use crate::address_space::{AddressSpace, ObjectBuilder, VariableBuilder};
use crate::alarms::replace_condition_type_definition;
use crate::alarms::source_monitor::{source_value_as_f64, SourceMonitoredAlarm};
use crate::alarms::state_machine::ConditionStateMachine;
use opcua_core::events::AlarmEvent;
use opcua_nodes::{DefaultTypeTree, NodeType};
use opcua_types::{
    AttributeId, BrowseDirection, DataEncoding, DataTypeId, DataValue, DateTime, Identifier,
    LocalizedText, NodeId, NumericRange, ObjectId, ObjectTypeId, QualifiedName, Range,
    ReferenceTypeId, StatusCode, TimestampsToReturn, VariableTypeId, Variant,
};
use std::sync::Mutex;

const EXCLUSIVE_STATE_HIGH_HIGH_ID: u32 = 9329;
const EXCLUSIVE_STATE_HIGH_ID: u32 = 9331;
const EXCLUSIVE_STATE_LOW_ID: u32 = 9333;
const EXCLUSIVE_STATE_LOW_LOW_ID: u32 = 9335;
const INPUT_NODE_PROPERTY_NAME: &str = "InputNode";

/// Selects whether limit state is mutually exclusive or independently tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitMode {
    /// Only one currently exceeded limit can be active.
    Exclusive,
    /// Each configured limit is evaluated independently.
    NonExclusive,
}

/// Selects the concrete limit-family alarm ObjectType (OPC-10000-9 §5.8.21). `Limit`, `Level`,
/// `Deviation`, and `RateOfChange` share identical threshold/deadband evaluation
/// (`LimitEvaluator`); they differ only in which `TypeDefinition` the resulting condition
/// instance reports and in what value is fed to the evaluator (raw process value, deviation
/// from a setpoint, or rate of change).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitAlarmKind {
    /// Generic process-limit alarm (`ExclusiveLimitAlarmType`/`NonExclusiveLimitAlarmType`).
    Limit,
    /// Level alarm, a `LimitAlarmType` subtype for level-monitoring use cases
    /// (`ExclusiveLevelAlarmType`/`NonExclusiveLevelAlarmType`, OPC-10000-9 §5.8.21.2/.3).
    Level,
    /// Deviation alarm, evaluated against `processValue - setpointValue` rather than the raw
    /// process value (`ExclusiveDeviationAlarmType`/`NonExclusiveDeviationAlarmType`, OPC-10000-9
    /// §5.8.22).
    Deviation,
    /// Rate-of-change alarm, evaluated against a computed rate rather than the raw process value
    /// (`ExclusiveRateOfChangeAlarmType`/`NonExclusiveRateOfChangeAlarmType`, OPC-10000-9 §5.8.23).
    RateOfChange,
}

impl LimitAlarmKind {
    pub(crate) fn type_id(self, mode: LimitMode) -> NodeId {
        match (self, mode) {
            (Self::Limit, LimitMode::Exclusive) => {
                NodeId::from(ObjectTypeId::ExclusiveLimitAlarmType)
            }
            (Self::Limit, LimitMode::NonExclusive) => {
                NodeId::from(ObjectTypeId::NonExclusiveLimitAlarmType)
            }
            (Self::Level, LimitMode::Exclusive) => {
                NodeId::from(ObjectTypeId::ExclusiveLevelAlarmType)
            }
            (Self::Level, LimitMode::NonExclusive) => {
                NodeId::from(ObjectTypeId::NonExclusiveLevelAlarmType)
            }
            (Self::Deviation, LimitMode::Exclusive) => {
                NodeId::from(ObjectTypeId::ExclusiveDeviationAlarmType)
            }
            (Self::Deviation, LimitMode::NonExclusive) => {
                NodeId::from(ObjectTypeId::NonExclusiveDeviationAlarmType)
            }
            (Self::RateOfChange, LimitMode::Exclusive) => {
                NodeId::from(ObjectTypeId::ExclusiveRateOfChangeAlarmType)
            }
            (Self::RateOfChange, LimitMode::NonExclusive) => {
                NodeId::from(ObjectTypeId::NonExclusiveRateOfChangeAlarmType)
            }
        }
    }
}

/// One of the four process alarm limit bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitLevel {
    /// High-high process limit.
    HighHigh,
    /// High process limit.
    High,
    /// Low process limit.
    Low,
    /// Low-low process limit.
    LowLow,
}

/// Threshold, hysteresis deadband, and severity for one process limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimitDef {
    /// Threshold value that trips the limit.
    pub value: f64,
    /// Hysteresis distance used when clearing an exceeded limit.
    pub deadband: f64,
    /// OPC UA condition severity to report while this limit is active.
    pub severity: u16,
}

/// Configured process limits for an exclusive or non-exclusive limit alarm.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitConfig {
    /// Evaluation mode for this limit alarm.
    pub mode: LimitMode,
    /// Optional high-high limit definition.
    pub high_high: Option<LimitDef>,
    /// Optional high limit definition.
    pub high: Option<LimitDef>,
    /// Optional low limit definition.
    pub low: Option<LimitDef>,
    /// Optional low-low limit definition.
    pub low_low: Option<LimitDef>,
}

impl LimitConfig {
    /// Starts building a limit configuration for the selected mode.
    #[must_use]
    pub fn new(mode: LimitMode) -> Self {
        Self {
            mode,
            high_high: None,
            high: None,
            low: None,
            low_low: None,
        }
    }

    /// Sets the high-high limit definition.
    #[must_use]
    pub fn with_high_high(mut self, limit: LimitDef) -> Self {
        self.high_high = Some(limit);
        self
    }

    /// Sets the high limit definition.
    #[must_use]
    pub fn with_high(mut self, limit: LimitDef) -> Self {
        self.high = Some(limit);
        self
    }

    /// Sets the low limit definition.
    #[must_use]
    pub fn with_low(mut self, limit: LimitDef) -> Self {
        self.low = Some(limit);
        self
    }

    /// Sets the low-low limit definition.
    #[must_use]
    pub fn with_low_low(mut self, limit: LimitDef) -> Self {
        self.low_low = Some(limit);
        self
    }

    /// Validates and returns the completed configuration.
    pub fn build(self) -> Result<Self, StatusCode> {
        self.validate()?;
        Ok(self)
    }

    /// Validates ordering, finite values, and deadband ranges.
    pub fn validate(&self) -> Result<(), StatusCode> {
        let limits = self.configured_limits();

        for (_, limit) in &limits {
            if !limit.value.is_finite()
                || !limit.deadband.is_finite()
                || limit.deadband.is_sign_negative()
            {
                return Err(StatusCode::BadOutOfRange);
            }
        }

        for pair in limits.windows(2) {
            let upper = pair[0].1;
            let lower = pair[1].1;
            if upper.value < lower.value {
                return Err(StatusCode::BadOutOfRange);
            }
        }

        for (index, (_, limit)) in limits.iter().enumerate() {
            let mut nearest_gap = f64::INFINITY;

            if let Some((_, upper)) = index
                .checked_sub(1)
                .and_then(|previous| limits.get(previous))
            {
                nearest_gap = nearest_gap.min(upper.value - limit.value);
            }

            if let Some((_, lower)) = limits.get(index + 1) {
                nearest_gap = nearest_gap.min(limit.value - lower.value);
            }

            if nearest_gap.is_finite() && limit.deadband >= nearest_gap {
                return Err(StatusCode::BadOutOfRange);
            }
        }

        Ok(())
    }

    /// Validates that all configured limit values fit inside the source variable EURange.
    pub fn validate_against_eurange(&self, low: f64, high: f64) -> Result<(), StatusCode> {
        for (_, limit) in self.configured_limits() {
            if limit.value < low || limit.value > high {
                return Err(StatusCode::BadOutOfRange);
            }
        }

        Ok(())
    }

    fn configured_limits(&self) -> Vec<(LimitLevel, LimitDef)> {
        let mut limits = Vec::with_capacity(4);

        if let Some(limit) = self.high_high {
            limits.push((LimitLevel::HighHigh, limit));
        }

        if let Some(limit) = self.high {
            limits.push((LimitLevel::High, limit));
        }

        if let Some(limit) = self.low {
            limits.push((LimitLevel::Low, limit));
        }

        if let Some(limit) = self.low_low {
            limits.push((LimitLevel::LowLow, limit));
        }

        limits
    }
}

/// Reads the EURange property of an AnalogItem source variable.
#[must_use]
pub fn read_eurange(address_space: &AddressSpace, source_node_id: &NodeId) -> Option<(f64, f64)> {
    let type_tree = DefaultTypeTree::new();
    let eurange_node = address_space.find_node_by_browse_name(
        source_node_id,
        Some((ReferenceTypeId::HasProperty, false)),
        &type_tree,
        BrowseDirection::Forward,
        QualifiedName::from("EURange"),
    )?;

    let value = eurange_node
        .as_node()
        .get_attribute(
            TimestampsToReturn::Neither,
            AttributeId::Value,
            &NumericRange::None,
            &DataEncoding::Binary,
        )
        .and_then(|data_value| data_value.value)?;

    match value {
        Variant::ExtensionObject(eurange) => eurange
            .inner_as::<Range>()
            .map(|range| (range.low, range.high)),
        _ => None,
    }
}

/// Active flags for a non-exclusive limit alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NonExclusiveState {
    /// Whether the high-high limit is active.
    pub high_high: bool,
    /// Whether the high limit is active.
    pub high: bool,
    /// Whether the low limit is active.
    pub low: bool,
    /// Whether the low-low limit is active.
    pub low_low: bool,
}

/// Current active limit state for either alarm mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveLimits {
    /// Exclusive mode state; `None` means inactive.
    Exclusive(Option<LimitLevel>),
    /// Non-exclusive mode state.
    NonExclusive(NonExclusiveState),
}

/// Result of evaluating a process value against limit alarm configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitOutcome {
    /// Active limit state after evaluation.
    pub limits: ActiveLimits,
    /// Whether any limit is active.
    pub active: bool,
    /// Effective severity for the active limit state.
    pub severity: u16,
    /// Human-readable condition message.
    pub message: String,
}

/// Id properties for configured NonExclusiveLimitAlarmType state variables.
#[derive(Debug, Clone, Default)]
pub struct NonExclusiveLimitStateIds {
    /// HighHighState.Id node, when HighHigh is configured.
    pub high_high: Option<NodeId>,
    /// HighHighState.TransitionTime node, when HighHigh is configured.
    pub high_high_transition_time: Option<NodeId>,
    /// HighState.Id node, when High is configured.
    pub high: Option<NodeId>,
    /// HighState.TransitionTime node, when High is configured.
    pub high_transition_time: Option<NodeId>,
    /// LowState.Id node, when Low is configured.
    pub low: Option<NodeId>,
    /// LowState.TransitionTime node, when Low is configured.
    pub low_transition_time: Option<NodeId>,
    /// LowLowState.Id node, when LowLow is configured.
    pub low_low: Option<NodeId>,
    /// LowLowState.TransitionTime node, when LowLow is configured.
    pub low_low_transition_time: Option<NodeId>,
}

impl NonExclusiveLimitStateIds {
    fn set(&mut self, level: LimitLevel, id: NodeId, transition_time_id: NodeId) {
        match level {
            LimitLevel::HighHigh => {
                self.high_high = Some(id);
                self.high_high_transition_time = Some(transition_time_id);
            }
            LimitLevel::High => {
                self.high = Some(id);
                self.high_transition_time = Some(transition_time_id);
            }
            LimitLevel::Low => {
                self.low = Some(id);
                self.low_transition_time = Some(transition_time_id);
            }
            LimitLevel::LowLow => {
                self.low_low = Some(id);
                self.low_low_transition_time = Some(transition_time_id);
            }
        }
    }

    fn transition_time_for(&self, level: LimitLevel) -> Option<&NodeId> {
        match level {
            LimitLevel::HighHigh => self.high_high_transition_time.as_ref(),
            LimitLevel::High => self.high_transition_time.as_ref(),
            LimitLevel::Low => self.low_transition_time.as_ref(),
            LimitLevel::LowLow => self.low_low_transition_time.as_ref(),
        }
    }
}

/// Pure limit alarm evaluator with deadband hysteresis.
pub struct LimitEvaluator;

impl LimitEvaluator {
    /// Evaluates a new process value against the configuration and previous state.
    #[must_use]
    pub fn evaluate(value: f64, cfg: &LimitConfig, prev: &ActiveLimits) -> LimitOutcome {
        if !value.is_finite() {
            return outcome_for_existing_state(previous_for_mode(cfg.mode, prev), cfg);
        }

        match cfg.mode {
            LimitMode::Exclusive => evaluate_exclusive(value, cfg, prev),
            LimitMode::NonExclusive => evaluate_non_exclusive(value, cfg, prev),
        }
    }
}

/// Address-space nodes and runtime state for a process limit alarm.
#[derive(Debug)]
pub struct LimitAlarm {
    /// Base A&C lifecycle state machine.
    pub condition: ConditionStateMachine,
    /// Bound AlarmConditionType InputNode source variable, or null when not bound.
    pub source_node: NodeId,
    /// Limit thresholds, deadbands, severities, and evaluation mode.
    pub config: LimitConfig,
    /// Exclusive LimitState.CurrentState variable node.
    pub limit_current_state_id: NodeId,
    /// Exclusive LimitState.CurrentState.Id property node.
    pub limit_current_state_id_id: NodeId,
    /// Exclusive LimitState.CurrentState.TransitionTime property node.
    pub limit_current_state_transition_time_id: NodeId,
    /// Non-exclusive state Id property nodes.
    pub non_exclusive_state_ids: NonExclusiveLimitStateIds,
    /// Previous evaluator state used for deadband hysteresis.
    pub prev: Mutex<ActiveLimits>,
    /// Which limit-family ObjectType this instance was created as (OPC-10000-9 §5.8.21).
    kind: LimitAlarmKind,
    /// OnDelay hysteresis in milliseconds (OPC-10000-9 §5.8.2, optional); 0.0 (default) commits
    /// activation immediately.
    on_delay_ms: f64,
    /// OffDelay hysteresis in milliseconds (OPC-10000-9 §5.8.2, optional); 0.0 (default) commits
    /// deactivation immediately.
    off_delay_ms: f64,
    /// ReAlarmTime in milliseconds (OPC-10000-9 §5.8.2, optional); 0.0 (default) disables
    /// re-alarming.
    re_alarm_ms: f64,
}

impl LimitAlarm {
    /// Configures OnDelay/OffDelay hysteresis (OPC-10000-9 §5.8.2, optional): activation/
    /// deactivation is only committed once the desired state has persisted for the respective
    /// delay, in milliseconds. Defaults to 0.0/0.0 (immediate) when never called.
    #[must_use]
    pub fn with_delays(mut self, on_delay_ms: f64, off_delay_ms: f64) -> Self {
        self.on_delay_ms = on_delay_ms;
        self.off_delay_ms = off_delay_ms;
        self
    }

    /// Configures ReAlarmTime (OPC-10000-9 §5.8.2, optional), in milliseconds: while the alarm
    /// remains active and unacknowledged for this long since it was last (re-)alarmed, it is
    /// re-alarmed (see `ConditionStateMachine::maybe_re_alarm`). Defaults to 0.0 (disabled) when
    /// never called.
    #[must_use]
    pub fn with_re_alarm(mut self, re_alarm_ms: f64) -> Self {
        self.re_alarm_ms = re_alarm_ms;
        self
    }

    /// Returns the base condition state machine for registry integration.
    #[must_use]
    pub fn condition_state_machine(&self) -> ConditionStateMachine {
        self.condition.clone()
    }

    /// Returns the bound AlarmConditionType InputNode source variable.
    #[must_use]
    pub fn source_node(&self) -> &NodeId {
        &self.source_node
    }

    /// Sets the bound AlarmConditionType InputNode source variable.
    pub fn set_source_node(&mut self, source: NodeId) {
        self.source_node = source;
    }

    /// Ensures the AlarmConditionType InputNode property exists and writes the source NodeId.
    pub fn write_input_node_property(&mut self, address_space: &AddressSpace, source: &NodeId) {
        let input_node_id = ensure_input_node_property(address_space, &self.condition.condition_id);
        set_variable_value(address_space, &input_node_id, Variant::from(source.clone()));
        self.source_node = source.clone();
    }

    /// Ensures the bound source has a forward HasCondition reference to this condition.
    pub fn write_has_condition_reference(&self, address_space: &AddressSpace, source: &NodeId) {
        let condition_id = &self.condition.condition_id;
        if !address_space.has_reference(source, condition_id, ReferenceTypeId::HasCondition) {
            address_space.insert_reference(source, condition_id, ReferenceTypeId::HasCondition);
        }
        let server_id = NodeId::from(ObjectId::Server);
        if !address_space.has_reference(&server_id, source, ReferenceTypeId::HasEventSource) {
            address_space.insert_reference(&server_id, source, ReferenceTypeId::HasEventSource);
        }
    }

    /// Creates an ExclusiveLimitAlarmType (or ExclusiveLevelAlarmType, per `kind`) instance and
    /// its LimitState nodes in the address space.
    pub fn create_exclusive_in_address_space(
        address_space: &AddressSpace,
        ns: u16,
        device: &str,
        alarm_name: &str,
        source_node_id: NodeId,
        cfg: LimitConfig,
        kind: LimitAlarmKind,
    ) -> Self {
        let condition = ConditionStateMachine::create_in_address_space(
            address_space,
            device,
            alarm_name,
            source_node_id,
            alarm_name,
        );

        replace_condition_type_definition(
            address_space,
            &condition.condition_id,
            kind.type_id(LimitMode::Exclusive),
        );

        let base_s = format!("Alarm_{}_{}", device, alarm_name);
        for (level, limit) in cfg.configured_limits() {
            add_limit_property(
                address_space,
                ns,
                &condition.condition_id,
                &base_s,
                level,
                limit,
            );
            add_deadband_property(
                address_space,
                ns,
                &condition.condition_id,
                &base_s,
                level,
                limit,
            );
        }

        let limit_state_id = NodeId::new(ns, format!("{}_LimitState", base_s));
        let limit_current_state_id = NodeId::new(ns, format!("{}_LimitState_CurrentState", base_s));
        let limit_current_state_id_id =
            NodeId::new(ns, format!("{}_LimitState_CurrentState_Id", base_s));
        let limit_current_state_transition_time_id = NodeId::new(
            ns,
            format!("{}_LimitState_CurrentState_TransitionTime", base_s),
        );

        ObjectBuilder::new(&limit_state_id, "LimitState", "LimitState")
            .has_type_definition(ObjectTypeId::ExclusiveLimitStateMachineType)
            .component_of(condition.condition_id.clone())
            .insert(address_space);

        VariableBuilder::new(&limit_current_state_id, "CurrentState", "CurrentState")
            .data_type(DataTypeId::LocalizedText)
            .has_type_definition(VariableTypeId::StateVariableType)
            .value(LocalizedText::null())
            .writable()
            .component_of(limit_state_id.clone())
            .insert(address_space);

        VariableBuilder::new(&limit_current_state_id_id, "Id", "Id")
            .data_type(DataTypeId::NodeId)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(NodeId::null())
            .writable()
            .property_of(limit_current_state_id.clone())
            .insert(address_space);

        VariableBuilder::new(
            &limit_current_state_transition_time_id,
            "TransitionTime",
            "TransitionTime",
        )
        .data_type(DataTypeId::DateTime)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(DateTime::now())
        .writable()
        .property_of(limit_current_state_id.clone())
        .insert(address_space);

        let initial_prev = inactive_limits_for_mode(cfg.mode);

        Self {
            condition,
            source_node: NodeId::null(),
            config: cfg,
            limit_current_state_id,
            limit_current_state_id_id,
            limit_current_state_transition_time_id,
            non_exclusive_state_ids: NonExclusiveLimitStateIds::default(),
            prev: Mutex::new(initial_prev),
            kind,
            on_delay_ms: 0.0,
            off_delay_ms: 0.0,
            re_alarm_ms: 0.0,
        }
    }

    /// Creates a NonExclusiveLimitAlarmType (or NonExclusiveLevelAlarmType, per `kind`) instance
    /// and its limit state nodes in the address space.
    pub fn create_non_exclusive_in_address_space(
        address_space: &AddressSpace,
        ns: u16,
        device: &str,
        alarm_name: &str,
        source_node_id: NodeId,
        cfg: LimitConfig,
        kind: LimitAlarmKind,
    ) -> Self {
        let condition = ConditionStateMachine::create_in_address_space(
            address_space,
            device,
            alarm_name,
            source_node_id,
            alarm_name,
        );

        replace_condition_type_definition(
            address_space,
            &condition.condition_id,
            kind.type_id(LimitMode::NonExclusive),
        );

        let base_s = format!("Alarm_{}_{}", device, alarm_name);
        let mut non_exclusive_state_ids = NonExclusiveLimitStateIds::default();

        for (level, limit) in cfg.configured_limits() {
            add_limit_property(
                address_space,
                ns,
                &condition.condition_id,
                &base_s,
                level,
                limit,
            );
            add_deadband_property(
                address_space,
                ns,
                &condition.condition_id,
                &base_s,
                level,
                limit,
            );

            let (state_id, transition_time_id) = add_non_exclusive_limit_state(
                address_space,
                ns,
                &condition.condition_id,
                &base_s,
                level,
            );
            non_exclusive_state_ids.set(level, state_id, transition_time_id);
        }

        let initial_prev = inactive_limits_for_mode(cfg.mode);

        Self {
            condition,
            source_node: NodeId::null(),
            config: cfg,
            limit_current_state_id: NodeId::null(),
            limit_current_state_id_id: NodeId::null(),
            limit_current_state_transition_time_id: NodeId::null(),
            non_exclusive_state_ids,
            prev: Mutex::new(initial_prev),
            kind,
            on_delay_ms: 0.0,
            off_delay_ms: 0.0,
            re_alarm_ms: 0.0,
        }
    }

    /// Evaluates and writes a new process value, returning an alarm event when the limit state changes.
    pub fn update_value(&self, address_space: &AddressSpace, value: f64) -> Option<AlarmEvent> {
        self.update_value_at(address_space, value, DateTime::now())
    }

    /// Same as `update_value`, but evaluated at an explicit time `now`: lets OnDelay/OffDelay
    /// hysteresis (OPC-10000-9 §5.8.2) be tested deterministically, and lets a periodic
    /// re-sample resolve a pending delay even when the raw process value hasn't changed.
    pub fn update_value_at(
        &self,
        address_space: &AddressSpace,
        value: f64,
        now: DateTime,
    ) -> Option<AlarmEvent> {
        if !self.condition.get_enabled(address_space) {
            return None;
        }

        let (previous, outcome) = {
            let mut prev = self.prev.lock().unwrap();
            let previous = *prev;
            let outcome = LimitEvaluator::evaluate(value, &self.config, &previous);
            *prev = outcome.limits;
            (previous, outcome)
        };

        let was_active = self.condition.get_active(address_space);
        let was_acked = self.condition.get_acked(address_space);
        let nothing_changed = previous == outcome.limits && outcome.active == was_active;
        // ReAlarmTime (OPC-10000-9 §5.8.2) can come due even with no new raw value change, via a
        // periodic re-sample of an alarm that hasn't yet returned to normal (Annex B.1.5's
        // example explicitly re-alarms an already-Acknowledged alarm that's still active).
        let re_alarm_possible = self.re_alarm_ms > 0.0 && was_active;

        if nothing_changed && !re_alarm_possible {
            return None;
        }

        if previous != outcome.limits {
            self.write_limit_state(address_space, previous, outcome.limits);
        }

        let mut re_alarmed = false;
        // OnDelay/OffDelay (OPC-10000-9 §5.8.2) only gates the false<->true ActiveState
        // transition itself; a severity/message change while already active (e.g. High
        // escalating to HighHigh) is reported immediately regardless of delay configuration.
        let reported_active = if outcome.active != was_active {
            let committed = self.condition.gate_active(
                address_space,
                outcome.active,
                now,
                self.on_delay_ms,
                self.off_delay_ms,
            )?;
            self.condition.reset_re_alarm(address_space, committed, now);
            committed
        } else if nothing_changed {
            // Only reachable when re_alarm_possible: re-check whether ReAlarmTime has elapsed.
            re_alarmed = self
                .condition
                .maybe_re_alarm(address_space, now, self.re_alarm_ms);
            if !re_alarmed {
                return None;
            }
            was_active
        } else {
            was_active
        };

        let message = LocalizedText::new("en", &outcome.message);
        if was_active && !was_acked && !reported_active {
            self.condition.create_branch(address_space);
        }
        self.condition.set_severity(address_space, outcome.severity);
        self.condition.set_message(address_space, message.clone());

        if reported_active && !re_alarmed {
            self.condition.set_acked(address_space, false);
            self.condition.set_confirmed(address_space, false);
        }

        let acked = self.condition.get_acked(address_space);
        let confirmed = self.condition.get_confirmed(address_space);
        let retain = reported_active || !acked || !confirmed;
        self.condition.set_retain(address_space, retain);

        let event_id = uuid::Uuid::new_v4().as_bytes().to_vec();
        self.condition.set_current_event_id(&event_id);

        Some(AlarmEvent {
            event_id,
            event_type: self.kind.type_id(self.config.mode),
            source_node: self.condition.source_node_id.clone(),
            source_name: self.condition.condition_name.clone(),
            time: DateTime::now(),
            message,
            severity: outcome.severity,
            condition_id: self.condition.condition_id.clone(),
            branch_id: NodeId::null(),
            condition_name: self.condition.condition_name.clone(),
            active_state: reported_active,
            acked_state: acked,
            confirmed_state: confirmed,
            retain,
        })
    }

    fn write_limit_state(
        &self,
        address_space: &AddressSpace,
        previous: ActiveLimits,
        limits: ActiveLimits,
    ) {
        match self.config.mode {
            LimitMode::Exclusive => {
                self.write_exclusive_limit_state(address_space, previous, limits)
            }
            LimitMode::NonExclusive => {
                self.write_non_exclusive_limit_state(address_space, previous, limits)
            }
        }
    }

    fn write_exclusive_limit_state(
        &self,
        address_space: &AddressSpace,
        previous: ActiveLimits,
        limits: ActiveLimits,
    ) {
        let level = match limits {
            ActiveLimits::Exclusive(level) => level,
            ActiveLimits::NonExclusive(_) => None,
        };
        let prev_level = match previous {
            ActiveLimits::Exclusive(level) => level,
            ActiveLimits::NonExclusive(_) => None,
        };

        let (text, id) = level.map_or_else(
            || (LocalizedText::null(), NodeId::null()),
            |level| {
                (
                    LocalizedText::new("en", level_name(level)),
                    exclusive_state_id(level),
                )
            },
        );

        set_variable_value(
            address_space,
            &self.limit_current_state_id,
            Variant::from(text),
        );
        set_variable_value(
            address_space,
            &self.limit_current_state_id_id,
            Variant::from(id),
        );
        // TransitionTime (OPC-10000-9 §5.2) records when this specific level was last entered —
        // only stamp it when the exceeded level actually changed, not on every re-evaluation.
        if level != prev_level {
            self.condition
                .write_transition_time(address_space, &self.limit_current_state_transition_time_id);
        }
    }

    fn write_non_exclusive_limit_state(
        &self,
        address_space: &AddressSpace,
        previous: ActiveLimits,
        limits: ActiveLimits,
    ) {
        let state = match limits {
            ActiveLimits::NonExclusive(state) => state,
            ActiveLimits::Exclusive(_) => NonExclusiveState::default(),
        };
        let prev_state = match previous {
            ActiveLimits::NonExclusive(state) => state,
            ActiveLimits::Exclusive(_) => NonExclusiveState::default(),
        };

        self.write_non_exclusive_level(
            address_space,
            LimitLevel::HighHigh,
            state.high_high,
            prev_state.high_high,
        );
        self.write_non_exclusive_level(
            address_space,
            LimitLevel::High,
            state.high,
            prev_state.high,
        );
        self.write_non_exclusive_level(address_space, LimitLevel::Low, state.low, prev_state.low);
        self.write_non_exclusive_level(
            address_space,
            LimitLevel::LowLow,
            state.low_low,
            prev_state.low_low,
        );
    }

    fn write_non_exclusive_level(
        &self,
        address_space: &AddressSpace,
        level: LimitLevel,
        active: bool,
        previous_active: bool,
    ) {
        let id = match level {
            LimitLevel::HighHigh => self.non_exclusive_state_ids.high_high.as_ref(),
            LimitLevel::High => self.non_exclusive_state_ids.high.as_ref(),
            LimitLevel::Low => self.non_exclusive_state_ids.low.as_ref(),
            LimitLevel::LowLow => self.non_exclusive_state_ids.low_low.as_ref(),
        };
        let Some(id) = id else {
            return;
        };
        set_variable_value(address_space, id, Variant::from(active));
        if active != previous_active {
            if let Some(transition_time_id) =
                self.non_exclusive_state_ids.transition_time_for(level)
            {
                self.condition
                    .write_transition_time(address_space, transition_time_id);
            }
        }
    }
}

impl SourceMonitoredAlarm for LimitAlarm {
    fn source_node(&self) -> &NodeId {
        &self.source_node
    }

    fn condition_id(&self) -> &NodeId {
        &self.condition.condition_id
    }

    fn re_evaluate(&self, address_space: &AddressSpace, value: &DataValue) -> Option<AlarmEvent> {
        source_value_as_f64(value).and_then(|value| self.update_value(address_space, value))
    }
}

fn evaluate_exclusive(value: f64, cfg: &LimitConfig, prev: &ActiveLimits) -> LimitOutcome {
    let previous = match prev {
        ActiveLimits::Exclusive(level) => *level,
        ActiveLimits::NonExclusive(_) => None,
    };

    let high_high = cfg
        .high_high
        .map(|limit| high_exceeded(value, limit, previous == Some(LimitLevel::HighHigh)))
        .unwrap_or(false);
    let high = cfg
        .high
        .map(|limit| high_exceeded(value, limit, previous == Some(LimitLevel::High)))
        .unwrap_or(false);
    let low = cfg
        .low
        .map(|limit| low_exceeded(value, limit, previous == Some(LimitLevel::Low)))
        .unwrap_or(false);
    let low_low = cfg
        .low_low
        .map(|limit| low_exceeded(value, limit, previous == Some(LimitLevel::LowLow)))
        .unwrap_or(false);

    let level = if high_high {
        Some(LimitLevel::HighHigh)
    } else if high {
        Some(LimitLevel::High)
    } else if low_low {
        Some(LimitLevel::LowLow)
    } else if low {
        Some(LimitLevel::Low)
    } else {
        None
    };

    outcome_for_existing_state(ActiveLimits::Exclusive(level), cfg)
}

fn evaluate_non_exclusive(value: f64, cfg: &LimitConfig, prev: &ActiveLimits) -> LimitOutcome {
    let previous = match prev {
        ActiveLimits::NonExclusive(state) => *state,
        ActiveLimits::Exclusive(_) => NonExclusiveState::default(),
    };

    let state = NonExclusiveState {
        high_high: cfg
            .high_high
            .map(|limit| high_exceeded(value, limit, previous.high_high))
            .unwrap_or(false),
        high: cfg
            .high
            .map(|limit| high_exceeded(value, limit, previous.high))
            .unwrap_or(false),
        low: cfg
            .low
            .map(|limit| low_exceeded(value, limit, previous.low))
            .unwrap_or(false),
        low_low: cfg
            .low_low
            .map(|limit| low_exceeded(value, limit, previous.low_low))
            .unwrap_or(false),
    };

    outcome_for_existing_state(ActiveLimits::NonExclusive(state), cfg)
}

fn high_exceeded(value: f64, limit: LimitDef, was_exceeded: bool) -> bool {
    if value > limit.value {
        return true;
    }

    limit.deadband > 0.0 && was_exceeded && value >= limit.value - limit.deadband
}

fn low_exceeded(value: f64, limit: LimitDef, was_exceeded: bool) -> bool {
    if value < limit.value {
        return true;
    }

    limit.deadband > 0.0 && was_exceeded && value <= limit.value + limit.deadband
}

fn previous_for_mode(mode: LimitMode, prev: &ActiveLimits) -> ActiveLimits {
    match (mode, prev) {
        (LimitMode::Exclusive, ActiveLimits::Exclusive(level)) => ActiveLimits::Exclusive(*level),
        (LimitMode::Exclusive, ActiveLimits::NonExclusive(_)) => ActiveLimits::Exclusive(None),
        (LimitMode::NonExclusive, ActiveLimits::NonExclusive(state)) => {
            ActiveLimits::NonExclusive(*state)
        }
        (LimitMode::NonExclusive, ActiveLimits::Exclusive(_)) => {
            ActiveLimits::NonExclusive(NonExclusiveState::default())
        }
    }
}

fn inactive_limits_for_mode(mode: LimitMode) -> ActiveLimits {
    match mode {
        LimitMode::Exclusive => ActiveLimits::Exclusive(None),
        LimitMode::NonExclusive => ActiveLimits::NonExclusive(NonExclusiveState::default()),
    }
}

fn outcome_for_existing_state(limits: ActiveLimits, cfg: &LimitConfig) -> LimitOutcome {
    match limits {
        ActiveLimits::Exclusive(level) => {
            let severity = level
                .and_then(|level| cfg.limit_def(level))
                .map_or(0, |limit| limit.severity);
            let active = level.is_some();
            let message = level.map_or_else(|| "Normal".to_string(), limit_message);

            LimitOutcome {
                limits: ActiveLimits::Exclusive(level),
                active,
                severity,
                message,
            }
        }
        ActiveLimits::NonExclusive(state) => {
            let mut active_levels = Vec::with_capacity(4);
            let mut severity = 0;

            for (is_active, level) in [
                (state.high_high, LimitLevel::HighHigh),
                (state.high, LimitLevel::High),
                (state.low, LimitLevel::Low),
                (state.low_low, LimitLevel::LowLow),
            ] {
                if is_active {
                    active_levels.push(level);
                    if let Some(limit) = cfg.limit_def(level) {
                        severity = severity.max(limit.severity);
                    }
                }
            }

            let active = !active_levels.is_empty();
            let message = if active {
                active_set_message(&active_levels)
            } else {
                "Normal".to_string()
            };

            LimitOutcome {
                limits: ActiveLimits::NonExclusive(state),
                active,
                severity,
                message,
            }
        }
    }
}

impl LimitConfig {
    fn limit_def(&self, level: LimitLevel) -> Option<LimitDef> {
        match level {
            LimitLevel::HighHigh => self.high_high,
            LimitLevel::High => self.high,
            LimitLevel::Low => self.low,
            LimitLevel::LowLow => self.low_low,
        }
    }
}

fn limit_message(level: LimitLevel) -> String {
    format!("{} limit exceeded", level_name(level))
}

fn active_set_message(levels: &[LimitLevel]) -> String {
    let names = levels
        .iter()
        .map(|level| level_name(*level))
        .collect::<Vec<_>>()
        .join(", ");

    if levels.len() == 1 {
        format!("{names} limit exceeded")
    } else {
        format!("{names} limits exceeded")
    }
}

fn level_name(level: LimitLevel) -> &'static str {
    match level {
        LimitLevel::HighHigh => "HighHigh",
        LimitLevel::High => "High",
        LimitLevel::Low => "Low",
        LimitLevel::LowLow => "LowLow",
    }
}

fn add_limit_property(
    address_space: &AddressSpace,
    ns: u16,
    condition_id: &NodeId,
    base_s: &str,
    level: LimitLevel,
    limit: LimitDef,
) {
    let (name, _) = limit_property_names(level);
    add_double_property(
        address_space,
        &NodeId::new(ns, format!("{}_{}", base_s, name)),
        condition_id,
        name,
        limit.value,
    );
}

fn add_deadband_property(
    address_space: &AddressSpace,
    ns: u16,
    condition_id: &NodeId,
    base_s: &str,
    level: LimitLevel,
    limit: LimitDef,
) {
    let (_, name) = limit_property_names(level);
    add_double_property(
        address_space,
        &NodeId::new(ns, format!("{}_{}", base_s, name)),
        condition_id,
        name,
        limit.deadband,
    );
}

fn add_double_property(
    address_space: &AddressSpace,
    node_id: &NodeId,
    parent_id: &NodeId,
    name: &str,
    value: f64,
) {
    VariableBuilder::new(node_id, name, name)
        .data_type(DataTypeId::Double)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(value)
        .writable()
        .property_of(parent_id.clone())
        .insert(address_space);
}

fn add_non_exclusive_limit_state(
    address_space: &AddressSpace,
    ns: u16,
    condition_id: &NodeId,
    base_s: &str,
    level: LimitLevel,
) -> (NodeId, NodeId) {
    let browse_name = non_exclusive_state_browse_name(level);
    let state_id = NodeId::new(ns, format!("{}_{}", base_s, browse_name));
    let id_id = NodeId::new(ns, format!("{}_{}_Id", base_s, browse_name));
    let true_state_id = NodeId::new(ns, format!("{}_{}_TrueState", base_s, browse_name));
    let false_state_id = NodeId::new(ns, format!("{}_{}_FalseState", base_s, browse_name));
    let transition_time_id = NodeId::new(ns, format!("{}_{}_TransitionTime", base_s, browse_name));

    VariableBuilder::new(&state_id, browse_name, browse_name)
        .data_type(DataTypeId::Boolean)
        .has_type_definition(VariableTypeId::TwoStateVariableType)
        .value(false)
        .writable()
        .component_of(condition_id.clone())
        .insert(address_space);

    VariableBuilder::new(&id_id, "Id", "Id")
        .data_type(DataTypeId::Boolean)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(false)
        .writable()
        .property_of(state_id.clone())
        .insert(address_space);

    add_localized_text_property(
        address_space,
        &true_state_id,
        &state_id,
        "TrueState",
        LocalizedText::new("en", non_exclusive_state_text(level, true)),
    );
    add_localized_text_property(
        address_space,
        &false_state_id,
        &state_id,
        "FalseState",
        LocalizedText::new("en", non_exclusive_state_text(level, false)),
    );

    VariableBuilder::new(&transition_time_id, "TransitionTime", "TransitionTime")
        .data_type(DataTypeId::DateTime)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(DateTime::now())
        .writable()
        .property_of(state_id.clone())
        .insert(address_space);

    (id_id, transition_time_id)
}

fn add_localized_text_property(
    address_space: &AddressSpace,
    node_id: &NodeId,
    parent_id: &NodeId,
    name: &str,
    value: LocalizedText,
) {
    VariableBuilder::new(node_id, name, name)
        .data_type(DataTypeId::LocalizedText)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(value)
        .writable()
        .property_of(parent_id.clone())
        .insert(address_space);
}

fn limit_property_names(level: LimitLevel) -> (&'static str, &'static str) {
    match level {
        LimitLevel::HighHigh => ("HighHighLimit", "HighHighDeadband"),
        LimitLevel::High => ("HighLimit", "HighDeadband"),
        LimitLevel::Low => ("LowLimit", "LowDeadband"),
        LimitLevel::LowLow => ("LowLowLimit", "LowLowDeadband"),
    }
}

fn exclusive_state_id(level: LimitLevel) -> NodeId {
    match level {
        LimitLevel::HighHigh => NodeId::new(0, EXCLUSIVE_STATE_HIGH_HIGH_ID),
        LimitLevel::High => NodeId::new(0, EXCLUSIVE_STATE_HIGH_ID),
        LimitLevel::Low => NodeId::new(0, EXCLUSIVE_STATE_LOW_ID),
        LimitLevel::LowLow => NodeId::new(0, EXCLUSIVE_STATE_LOW_LOW_ID),
    }
}

fn non_exclusive_state_browse_name(level: LimitLevel) -> &'static str {
    match level {
        LimitLevel::HighHigh => "HighHighState",
        LimitLevel::High => "HighState",
        LimitLevel::Low => "LowState",
        LimitLevel::LowLow => "LowLowState",
    }
}

fn non_exclusive_state_text(level: LimitLevel, active: bool) -> &'static str {
    match (level, active) {
        (LimitLevel::HighHigh, true) => "High High active",
        (LimitLevel::HighHigh, false) => "High High inactive",
        (LimitLevel::High, true) => "High active",
        (LimitLevel::High, false) => "High inactive",
        (LimitLevel::Low, true) => "Low active",
        (LimitLevel::Low, false) => "Low inactive",
        (LimitLevel::LowLow, true) => "Low Low active",
        (LimitLevel::LowLow, false) => "Low Low inactive",
    }
}

fn set_variable_value(address_space: &AddressSpace, node_id: &NodeId, value: Variant) {
    if let Some(mut node) = address_space.find_mut(node_id) {
        if let NodeType::Variable(ref mut var) = &mut *node {
            let _ = var.set_value(&opcua_types::NumericRange::None, value);
        }
    }
}

fn ensure_input_node_property(address_space: &AddressSpace, condition_id: &NodeId) -> NodeId {
    ensure_node_ref_property(address_space, condition_id, INPUT_NODE_PROPERTY_NAME)
}

/// Ensures a `NodeId`-valued reference property (e.g. `InputNode`, `SetpointNode`,
/// `TargetValueNode`) exists on `condition_id` and returns its NodeId, creating it on first use.
/// Shared by every alarm kind that binds to a second Variable beyond its primary source.
pub(crate) fn ensure_node_ref_property(
    address_space: &AddressSpace,
    condition_id: &NodeId,
    property_name: &str,
) -> NodeId {
    if let Some(node_id) = find_node_ref_property(address_space, condition_id, property_name) {
        return node_id;
    }

    let node_id = node_ref_property_node_id(condition_id, property_name);
    if !address_space.node_exists(&node_id) {
        VariableBuilder::new(
            &node_id,
            QualifiedName::new(0, property_name),
            property_name,
        )
        .data_type(DataTypeId::NodeId)
        .has_type_definition(VariableTypeId::PropertyType)
        .value(NodeId::null())
        .writable()
        .property_of(condition_id.clone())
        .insert(address_space);
    } else if !address_space.has_reference(condition_id, &node_id, ReferenceTypeId::HasProperty) {
        address_space.insert_reference(condition_id, &node_id, ReferenceTypeId::HasProperty);
    }

    node_id
}

fn find_node_ref_property(
    address_space: &AddressSpace,
    condition_id: &NodeId,
    property_name: &str,
) -> Option<NodeId> {
    let type_tree = DefaultTypeTree::new();
    address_space
        .find_node_by_browse_name(
            condition_id,
            Some((ReferenceTypeId::HasProperty, false)),
            &type_tree,
            BrowseDirection::Forward,
            QualifiedName::new(0, property_name),
        )
        .map(|node| node.as_node().node_id().clone())
}

fn node_ref_property_node_id(condition_id: &NodeId, property_name: &str) -> NodeId {
    let base = match &condition_id.identifier {
        Identifier::String(value) => value
            .value()
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| condition_id.to_string()),
        _ => condition_id.to_string(),
    };

    NodeId::new(condition_id.namespace, format!("{base}_{property_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_address_space() -> AddressSpace {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 2);
        address_space
    }

    fn input_node_property_id(address_space: &AddressSpace, condition_id: &NodeId) -> NodeId {
        let type_tree = DefaultTypeTree::new();
        address_space
            .find_node_by_browse_name(
                condition_id,
                Some((ReferenceTypeId::HasProperty, false)),
                &type_tree,
                BrowseDirection::Forward,
                QualifiedName::new(0, "InputNode"),
            )
            .map(|node| node.as_node().node_id().clone())
            .expect("InputNode property should exist")
    }

    fn node_value(address_space: &AddressSpace, node_id: &NodeId) -> Variant {
        let node = address_space
            .find(node_id)
            .expect("variable node should exist");
        let NodeType::Variable(var) = &*node else {
            panic!("node should be a variable");
        };

        var.value(
            TimestampsToReturn::Neither,
            &NumericRange::None,
            &DataEncoding::Binary,
            0.0,
        )
        .value
        .expect("variable should have a value")
    }

    #[test]
    fn write_input_node_property_creates_and_updates_condition_property() {
        let address_space = test_address_space();
        let cfg = LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 100.0,
                deadband: 1.0,
                severity: 700,
            })
            .build()
            .expect("limit config should be valid");
        let mut alarm = LimitAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "HighTemperature",
            NodeId::new(2, "InitialSource"),
            cfg,
            LimitAlarmKind::Limit,
        );
        let condition_id = alarm.condition.condition_id.clone();
        let type_tree = DefaultTypeTree::new();

        assert!(address_space
            .find_node_by_browse_name(
                &condition_id,
                Some((ReferenceTypeId::HasProperty, false)),
                &type_tree,
                BrowseDirection::Forward,
                QualifiedName::new(0, "InputNode"),
            )
            .is_none());

        let source = NodeId::new(2, "DeviceA.Temperature");
        alarm.write_input_node_property(&address_space, &source);

        assert_eq!(alarm.source_node(), &source);
        let input_node_id = input_node_property_id(&address_space, &condition_id);
        assert!(address_space.has_reference(
            &condition_id,
            &input_node_id,
            ReferenceTypeId::HasProperty
        ));
        assert!(address_space.has_reference(
            &input_node_id,
            &NodeId::from(VariableTypeId::PropertyType),
            ReferenceTypeId::HasTypeDefinition
        ));

        let node = address_space
            .find(&input_node_id)
            .expect("InputNode property should exist");
        let NodeType::Variable(var) = &*node else {
            panic!("InputNode property should be a variable");
        };
        assert_eq!(var.data_type(), NodeId::from(DataTypeId::NodeId));
        drop(node);
        assert_eq!(
            node_value(&address_space, &input_node_id),
            Variant::from(source)
        );

        let updated_source = NodeId::new(2, "DeviceA.Pressure");
        alarm.write_input_node_property(&address_space, &updated_source);

        assert_eq!(alarm.source_node(), &updated_source);
        assert_eq!(
            input_node_property_id(&address_space, &condition_id),
            input_node_id
        );
        assert_eq!(
            node_value(&address_space, &input_node_id),
            Variant::from(updated_source)
        );
    }

    #[test]
    fn write_has_condition_reference_adds_forward_source_reference_idempotently() {
        let address_space = test_address_space();
        let cfg = LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 100.0,
                deadband: 1.0,
                severity: 700,
            })
            .build()
            .expect("limit config should be valid");
        let alarm = LimitAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "HighTemperature",
            NodeId::new(2, "InitialSource"),
            cfg,
            LimitAlarmKind::Limit,
        );
        let condition_id = alarm.condition.condition_id.clone();
        let source = NodeId::new(2, "DeviceA.Temperature");

        alarm.write_has_condition_reference(&address_space, &source);
        alarm.write_has_condition_reference(&address_space, &source);

        assert!(address_space.has_reference(&source, &condition_id, ReferenceTypeId::HasCondition));

        let type_tree = DefaultTypeTree::new();
        let reference_count = address_space
            .find_references(
                &source,
                Some((ReferenceTypeId::HasCondition, false)),
                &type_tree,
                BrowseDirection::Forward,
            )
            .into_iter()
            .filter(|reference| reference.target_id == condition_id)
            .count();

        assert_eq!(reference_count, 1);
    }

    #[test]
    fn write_has_condition_adds_has_event_source_from_server_to_source() {
        let address_space = test_address_space();
        let cfg = LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 100.0,
                deadband: 1.0,
                severity: 700,
            })
            .build()
            .expect("limit config should be valid");
        let alarm = LimitAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceB",
            "Pressure",
            NodeId::new(2, "InitialSrc"),
            cfg,
            LimitAlarmKind::Limit,
        );
        let source = NodeId::new(2, "DeviceB.Pressure");
        alarm.write_has_condition_reference(&address_space, &source);

        let server_id = NodeId::from(ObjectId::Server);
        assert!(
            address_space.has_reference(&server_id, &source, ReferenceTypeId::HasEventSource),
            "HasEventSource must be added from Server to the bound source"
        );
    }

    fn on_off_delay_cfg() -> LimitConfig {
        LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 100.0,
                deadband: 1.0,
                severity: 500,
            })
            .build()
            .expect("limit config should be valid")
    }

    // OPC-10000-9 §5.8.2: OnDelay/OffDelay defer the ActiveState transition itself until the
    // desired state has persisted for the configured duration; a same-state severity escalation
    // (T048 regression coverage) must still report immediately, un-gated.
    #[test]
    fn on_delay_defers_activation_until_elapsed_then_off_delay_defers_deactivation() {
        let address_space = test_address_space();
        let alarm = LimitAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "Delayed",
            NodeId::new(2, "InitialSource"),
            on_off_delay_cfg(),
            LimitAlarmKind::Limit,
        )
        .with_delays(1_000.0, 500.0);

        let t0 = DateTime::now();
        assert!(
            alarm.update_value_at(&address_space, 105.0, t0).is_none(),
            "activation must not commit before OnDelay elapses"
        );
        assert!(!alarm.condition_state_machine().get_active(&address_space));

        let t_plus_400ms = DateTime::from(t0.checked_ticks() + 4_000_000);
        assert!(
            alarm
                .update_value_at(&address_space, 105.0, t_plus_400ms)
                .is_none(),
            "400ms < 1000ms OnDelay -- still pending"
        );
        assert!(!alarm.condition_state_machine().get_active(&address_space));

        let t_plus_1100ms = DateTime::from(t0.checked_ticks() + 11_000_000);
        let event = alarm
            .update_value_at(&address_space, 105.0, t_plus_1100ms)
            .expect("1100ms >= 1000ms OnDelay -- activation should commit");
        assert!(event.active_state);
        assert!(alarm.condition_state_machine().get_active(&address_space));

        // Deactivation: OffDelay is 500ms.
        assert!(
            alarm
                .update_value_at(&address_space, 50.0, t_plus_1100ms)
                .is_none(),
            "deactivation must not commit before OffDelay elapses"
        );
        assert!(alarm.condition_state_machine().get_active(&address_space));

        let t_plus_1700ms = DateTime::from(t0.checked_ticks() + 17_000_000);
        let event = alarm
            .update_value_at(&address_space, 50.0, t_plus_1700ms)
            .expect("600ms >= 500ms OffDelay -- deactivation should commit");
        assert!(!event.active_state);
        assert!(!alarm.condition_state_machine().get_active(&address_space));
    }

    #[test]
    fn severity_escalation_while_active_reports_immediately_despite_on_delay() {
        let address_space = test_address_space();
        let cfg = LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 100.0,
                deadband: 1.0,
                severity: 400,
            })
            .with_high_high(LimitDef {
                value: 110.0,
                deadband: 1.0,
                severity: 700,
            })
            .build()
            .expect("valid config");
        let alarm = LimitAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "Delayed",
            NodeId::new(2, "InitialSource"),
            cfg,
            LimitAlarmKind::Limit,
        )
        .with_delays(1_000.0, 1_000.0);

        let t0 = DateTime::now();
        let t_plus_1100ms = DateTime::from(t0.checked_ticks() + 11_000_000);
        alarm.update_value_at(&address_space, 105.0, t0);
        let event = alarm
            .update_value_at(&address_space, 105.0, t_plus_1100ms)
            .expect("activation commits once OnDelay elapses");
        assert_eq!(event.severity, 400);

        // Escalate to HighHigh at the same instant: no further OnDelay wait, since ActiveState
        // itself isn't transitioning -- only the severity/message are escalating.
        let event = alarm
            .update_value_at(&address_space, 115.0, t_plus_1100ms)
            .expect("severity escalation while active must report immediately");
        assert!(event.active_state);
        assert_eq!(event.severity, 700);
    }

    // OPC-10000-9 §5.8.2/Annex B.1.5: an alarm still active+unacknowledged after ReAlarmTime is
    // re-alarmed (as if it just went into alarm) -- AckedState returns to false,
    // ReAlarmRepeatCount increments, and it re-notifies even with no new raw value change (e.g.
    // via periodic re-sampling). ReAlarmRepeatCount resets only when the alarm returns to normal.
    #[test]
    fn re_alarm_fires_after_re_alarm_time_while_still_active_and_unacked() {
        let address_space = test_address_space();
        let alarm = LimitAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "ReAlarmed",
            NodeId::new(2, "InitialSource"),
            on_off_delay_cfg(),
            LimitAlarmKind::Limit,
        )
        .with_re_alarm(5_000.0);

        let t0 = DateTime::now();
        let event = alarm
            .update_value_at(&address_space, 105.0, t0)
            .expect("initial activation");
        assert!(event.active_state && !event.acked_state);

        // Re-sampling the SAME value before ReAlarmTime elapses must not re-alarm.
        let t_plus_2s = DateTime::from(t0.checked_ticks() + 2 * 10_000_000);
        assert!(
            alarm
                .update_value_at(&address_space, 105.0, t_plus_2s)
                .is_none(),
            "2s < 5s ReAlarmTime -- must not re-alarm yet"
        );

        // Acknowledge it, then let ReAlarmTime elapse: it should come back unacknowledged.
        {
            let space = &address_space;
            alarm.condition.set_acked(space, true);
        }
        let t_plus_6s = DateTime::from(t0.checked_ticks() + 6 * 10_000_000);
        let event = alarm
            .update_value_at(&address_space, 105.0, t_plus_6s)
            .expect("ReAlarmTime elapsed while still active -- should re-alarm");
        assert!(event.active_state);
        assert!(
            !event.acked_state,
            "re-alarm returns the alarm to unacknowledged"
        );

        let repeat_count_id = alarm.condition.re_alarm_repeat_count_id.clone();
        let node = address_space
            .find(&repeat_count_id)
            .expect("ReAlarmRepeatCount node should exist");
        let NodeType::Variable(var) = &*node else {
            panic!("ReAlarmRepeatCount should be a variable");
        };
        let count = var
            .value(
                TimestampsToReturn::Neither,
                &NumericRange::None,
                &DataEncoding::Binary,
                0.0,
            )
            .value;
        assert_eq!(count, Some(Variant::from(1i16)));
        drop(node);

        // Returning to normal resets the counter.
        alarm.update_value_at(&address_space, 50.0, t_plus_6s);
        let node = address_space
            .find(&repeat_count_id)
            .expect("ReAlarmRepeatCount node should exist");
        let NodeType::Variable(var) = &*node else {
            panic!("ReAlarmRepeatCount should be a variable");
        };
        let count = var
            .value(
                TimestampsToReturn::Neither,
                &NumericRange::None,
                &DataEncoding::Binary,
                0.0,
            )
            .value;
        assert_eq!(count, Some(Variant::from(0i16)));
    }
}
