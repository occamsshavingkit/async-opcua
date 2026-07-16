//! `DiscrepancyAlarmType` evaluation and address-space wiring (OPC-10000-9 §5.8.25). Based
//! directly on `AlarmConditionType` (not `LimitAlarmType`): active when the InputNode's value
//! differs from `TargetValueNode`'s value by more than `Tolerance` for longer than
//! `ExpectedTime`; deactivates immediately once the value returns within tolerance.

use crate::address_space::{AddressSpace, VariableBuilder};
use crate::alarms::limit::ensure_node_ref_property;
use crate::alarms::replace_condition_type_definition;
use crate::alarms::source_monitor::{source_value_as_f64, SourceMonitoredAlarm};
use crate::alarms::state_machine::ConditionStateMachine;
use opcua_core::events::AlarmEvent;
use opcua_nodes::NodeType;
use opcua_types::{
    DataTypeId, DataValue, DateTime, LocalizedText, NodeId, NumericRange, ObjectTypeId,
    VariableTypeId, Variant,
};
use std::sync::Mutex;

const ACTIVE_SEVERITY: u16 = 500;
const INACTIVE_SEVERITY: u16 = 0;
const TARGET_VALUE_NODE_PROPERTY_NAME: &str = "TargetValueNode";
/// 100ns per OPC UA `DateTime` tick (Part 6 §5.1.3); OPC UA `Duration` properties are
/// milliseconds (Part 3 §8.13).
const TICKS_PER_MS: f64 = 10_000.0;

/// Address-space nodes and runtime state for a `DiscrepancyAlarmType` instance.
#[derive(Debug)]
pub struct DiscrepancyAlarm {
    /// Base A&C lifecycle state machine.
    pub condition: ConditionStateMachine,
    target_value_node_property_id: NodeId,
    target_source: NodeId,
    expected_time_ms: f64,
    tolerance: f64,
    process_value: Mutex<f64>,
    target_value: Mutex<f64>,
    discrepancy_since: Mutex<Option<DateTime>>,
    prev_active: Mutex<bool>,
}

impl DiscrepancyAlarm {
    /// Returns the base condition state machine for registry integration.
    #[must_use]
    pub fn condition_state_machine(&self) -> ConditionStateMachine {
        self.condition.clone()
    }

    /// Returns the bound process-value source Variable.
    #[must_use]
    pub fn source_node(&self) -> &NodeId {
        &self.condition.source_node_id
    }

    /// Returns the bound target-value source Variable, or null when not yet bound.
    #[must_use]
    pub fn target_value_node(&self) -> &NodeId {
        &self.target_source
    }

    /// Creates a `DiscrepancyAlarmType` instance and its `TargetValueNode`/`ExpectedTime`/
    /// `Tolerance` properties. `expected_time_ms` is the duration the value must remain outside
    /// `tolerance` of the target before the alarm activates.
    pub fn create_in_address_space(
        address_space: &AddressSpace,
        ns: u16,
        device: &str,
        alarm_name: &str,
        source_node_id: NodeId,
        expected_time_ms: f64,
        tolerance: f64,
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
            NodeId::from(ObjectTypeId::DiscrepancyAlarmType),
        );

        let base_s = format!("Alarm_{}_{}", device, alarm_name);
        let expected_time_id = NodeId::new(ns, format!("{}_ExpectedTime", base_s));
        let tolerance_id = NodeId::new(ns, format!("{}_Tolerance", base_s));

        VariableBuilder::new(&expected_time_id, "ExpectedTime", "ExpectedTime")
            .data_type(DataTypeId::Duration)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(expected_time_ms)
            .writable()
            .property_of(condition.condition_id.clone())
            .insert(address_space);

        VariableBuilder::new(&tolerance_id, "Tolerance", "Tolerance")
            .data_type(DataTypeId::Double)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(tolerance)
            .writable()
            .property_of(condition.condition_id.clone())
            .insert(address_space);

        Self {
            condition,
            target_value_node_property_id: NodeId::null(),
            target_source: NodeId::null(),
            expected_time_ms,
            tolerance,
            process_value: Mutex::new(0.0),
            target_value: Mutex::new(0.0),
            discrepancy_since: Mutex::new(None),
            prev_active: Mutex::new(false),
        }
    }

    /// Ensures the `TargetValueNode` property exists and writes the bound target source Variable
    /// NodeId.
    pub fn write_target_value_node_property(
        &mut self,
        address_space: &AddressSpace,
        target: &NodeId,
    ) {
        let property_id = ensure_node_ref_property(
            address_space,
            &self.condition.condition_id,
            TARGET_VALUE_NODE_PROPERTY_NAME,
        );
        set_variable_value(address_space, &property_id, Variant::from(target.clone()));
        self.target_value_node_property_id = property_id;
        self.target_source = target.clone();
    }

    /// Updates the cached process value and re-evaluates the discrepancy at time `now`,
    /// returning an alarm event when the active state changes.
    pub fn update_process_value(
        &self,
        address_space: &AddressSpace,
        value: f64,
        now: DateTime,
    ) -> Option<AlarmEvent> {
        *self.process_value.lock().unwrap() = value;
        self.recompute(address_space, now)
    }

    /// Updates the cached target value and re-evaluates the discrepancy at time `now`, returning
    /// an alarm event when the active state changes.
    pub fn update_target_value(
        &self,
        address_space: &AddressSpace,
        value: f64,
        now: DateTime,
    ) -> Option<AlarmEvent> {
        *self.target_value.lock().unwrap() = value;
        self.recompute(address_space, now)
    }

    fn recompute(&self, address_space: &AddressSpace, now: DateTime) -> Option<AlarmEvent> {
        if !self.condition.get_enabled(address_space) {
            return None;
        }

        let process = *self.process_value.lock().unwrap();
        let target = *self.target_value.lock().unwrap();
        let within_tolerance = (process - target).abs() <= self.tolerance;

        let should_be_active = {
            let mut since = self.discrepancy_since.lock().unwrap();
            if within_tolerance {
                *since = None;
                false
            } else {
                let started = *since.get_or_insert(now);
                let elapsed_ms =
                    (now.checked_ticks() - started.checked_ticks()) as f64 / TICKS_PER_MS;
                elapsed_ms >= self.expected_time_ms
            }
        };

        {
            let mut prev_active = self.prev_active.lock().unwrap();
            if should_be_active == *prev_active {
                return None;
            }
            *prev_active = should_be_active;
        }

        let severity = if should_be_active {
            ACTIVE_SEVERITY
        } else {
            INACTIVE_SEVERITY
        };
        let message = LocalizedText::new(
            "en",
            if should_be_active {
                "Value has not reached target within the expected time"
            } else {
                "Value at target"
            },
        );

        let was_active = self.condition.get_active(address_space);
        let was_acked = self.condition.get_acked(address_space);
        if was_active && !was_acked && !should_be_active {
            self.condition.create_branch(address_space);
        }
        self.condition.set_active(address_space, should_be_active);
        self.condition.set_severity(address_space, severity);
        self.condition.set_message(address_space, message.clone());

        if should_be_active {
            self.condition.set_acked(address_space, false);
            self.condition.set_confirmed(address_space, false);
        }

        let acked = self.condition.get_acked(address_space);
        let confirmed = self.condition.get_confirmed(address_space);
        let retain = should_be_active || !acked || !confirmed;
        self.condition.set_retain(address_space, retain);

        let event_id = uuid::Uuid::new_v4().as_bytes().to_vec();
        self.condition.set_current_event_id(&event_id);

        Some(AlarmEvent {
            event_id,
            event_type: NodeId::from(ObjectTypeId::DiscrepancyAlarmType),
            source_node: self.condition.source_node_id.clone(),
            source_name: self.condition.condition_name.clone(),
            time: DateTime::now(),
            message,
            severity,
            condition_id: self.condition.condition_id.clone(),
            branch_id: NodeId::null(),
            condition_name: self.condition.condition_name.clone(),
            active_state: should_be_active,
            acked_state: acked,
            confirmed_state: confirmed,
            retain,
        })
    }
}

impl SourceMonitoredAlarm for DiscrepancyAlarm {
    fn source_node(&self) -> &NodeId {
        &self.condition.source_node_id
    }

    fn condition_id(&self) -> &NodeId {
        &self.condition.condition_id
    }

    fn re_evaluate(&self, address_space: &AddressSpace, value: &DataValue) -> Option<AlarmEvent> {
        let now = value
            .source_timestamp
            .or(value.server_timestamp)
            .unwrap_or_else(DateTime::now);
        source_value_as_f64(value)
            .and_then(|value| self.update_process_value(address_space, value, now))
    }
}

fn set_variable_value(address_space: &AddressSpace, node_id: &NodeId, value: Variant) {
    if let Some(mut node) = address_space.find_mut(node_id) {
        if let NodeType::Variable(ref mut var) = &mut *node {
            let _ = var.set_value(&NumericRange::None, value);
        }
    }
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

    #[test]
    fn activates_only_after_expected_time_elapses_outside_tolerance() {
        let address_space = test_address_space();
        let alarm = DiscrepancyAlarm::create_in_address_space(
            &address_space,
            2,
            "Motor1",
            "StartDiscrepancy",
            NodeId::new(2, "Motor1.Running"),
            5_000.0, // 5 seconds
            0.5,
        );

        let t0 = DateTime::now();
        assert!(alarm.update_target_value(&address_space, 1.0, t0).is_none());
        // Discrepancy begins now (process 0.0 vs target 1.0, tolerance 0.5).
        assert!(alarm
            .update_process_value(&address_space, 0.0, t0)
            .is_none());

        // 2s later: still within ExpectedTime, must not yet activate.
        let t_plus_2s = DateTime::from(t0.checked_ticks() + 2 * 10_000_000);
        assert!(alarm
            .update_process_value(&address_space, 0.0, t_plus_2s)
            .is_none());
        assert!(!alarm.condition_state_machine().get_active(&address_space));

        // 6s later: ExpectedTime (5s) has elapsed with the discrepancy still present.
        let t_plus_6s = DateTime::from(t0.checked_ticks() + 6 * 10_000_000);
        let event = alarm
            .update_process_value(&address_space, 0.0, t_plus_6s)
            .expect("discrepancy persisting beyond ExpectedTime should activate the alarm");
        assert!(event.active_state);
        assert!(alarm.condition_state_machine().get_active(&address_space));

        // Reaching the target (within tolerance) clears the alarm immediately.
        let event = alarm
            .update_process_value(&address_space, 1.0, t_plus_6s)
            .expect("reaching the target value should deactivate the alarm");
        assert!(!event.active_state);
        assert!(!alarm.condition_state_machine().get_active(&address_space));
    }

    #[test]
    fn never_activates_when_within_tolerance() {
        let address_space = test_address_space();
        let alarm = DiscrepancyAlarm::create_in_address_space(
            &address_space,
            2,
            "Motor1",
            "StartDiscrepancy",
            NodeId::new(2, "Motor1.Running"),
            5_000.0,
            0.5,
        );

        let t0 = DateTime::now();
        let t_plus_10s = DateTime::from(t0.checked_ticks() + 10 * 10_000_000);
        alarm.update_target_value(&address_space, 1.0, t0);
        assert!(alarm
            .update_process_value(&address_space, 0.9, t_plus_10s)
            .is_none());
        assert!(!alarm.condition_state_machine().get_active(&address_space));
    }
}
