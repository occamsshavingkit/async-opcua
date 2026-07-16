//! Deviation-alarm evaluation and address-space wiring for `ExclusiveDeviationAlarmType` and
//! `NonExclusiveDeviationAlarmType` (OPC-10000-9 §5.8.22).

use crate::address_space::AddressSpace;
use crate::alarms::limit::{ensure_node_ref_property, LimitAlarm, LimitAlarmKind, LimitConfig};
use crate::alarms::source_monitor::{source_value_as_f64, SourceMonitoredAlarm};
use crate::alarms::state_machine::ConditionStateMachine;
use opcua_core::events::AlarmEvent;
use opcua_nodes::NodeType;
use opcua_types::{DataValue, NodeId, NumericRange, Variant};
use std::sync::Mutex;

const SETPOINT_NODE_PROPERTY_NAME: &str = "SetpointNode";

/// Address-space nodes and runtime state for a deviation alarm: reuses `LimitAlarm`'s
/// threshold/deadband evaluator, but evaluates `processValue - setpointValue` rather than the
/// raw process value (OPC-10000-9 §5.8.22).
#[derive(Debug)]
pub struct DeviationAlarm {
    limit: LimitAlarm,
    setpoint_node_property_id: NodeId,
    setpoint_source: NodeId,
    process_value: Mutex<f64>,
    setpoint_value: Mutex<f64>,
}

impl DeviationAlarm {
    /// Returns the base condition state machine for registry integration.
    #[must_use]
    pub fn condition_state_machine(&self) -> ConditionStateMachine {
        self.limit.condition_state_machine()
    }

    /// Returns the bound process-value source Variable.
    #[must_use]
    pub fn source_node(&self) -> &NodeId {
        self.limit.source_node()
    }

    /// Returns the bound setpoint source Variable, or null when not yet bound.
    #[must_use]
    pub fn setpoint_node(&self) -> &NodeId {
        &self.setpoint_source
    }

    /// Ensures the AlarmConditionType InputNode property exists and writes the process source.
    pub fn write_input_node_property(&mut self, address_space: &AddressSpace, source: &NodeId) {
        self.limit.write_input_node_property(address_space, source);
    }

    /// Ensures the bound process source has a forward HasCondition reference to this condition.
    pub fn write_has_condition_reference(&self, address_space: &AddressSpace, source: &NodeId) {
        self.limit
            .write_has_condition_reference(address_space, source);
    }

    /// Ensures the `SetpointNode` property (OPC-10000-9 §5.8.22) exists and writes the bound
    /// setpoint source Variable NodeId.
    pub fn write_setpoint_node_property(
        &mut self,
        address_space: &AddressSpace,
        setpoint: &NodeId,
    ) {
        let property_id = ensure_node_ref_property(
            address_space,
            &self.limit.condition.condition_id,
            SETPOINT_NODE_PROPERTY_NAME,
        );
        set_node_ref_value(address_space, &property_id, setpoint.clone());
        self.setpoint_node_property_id = property_id;
        self.setpoint_source = setpoint.clone();
    }

    /// Creates an `ExclusiveDeviationAlarmType` instance and its `LimitState` nodes.
    pub fn create_exclusive_in_address_space(
        address_space: &AddressSpace,
        ns: u16,
        device: &str,
        alarm_name: &str,
        source_node_id: NodeId,
        cfg: LimitConfig,
    ) -> Self {
        let limit = LimitAlarm::create_exclusive_in_address_space(
            address_space,
            ns,
            device,
            alarm_name,
            source_node_id,
            cfg,
            LimitAlarmKind::Deviation,
        );
        Self::from_limit(limit)
    }

    /// Creates a `NonExclusiveDeviationAlarmType` instance and its limit state nodes.
    pub fn create_non_exclusive_in_address_space(
        address_space: &AddressSpace,
        ns: u16,
        device: &str,
        alarm_name: &str,
        source_node_id: NodeId,
        cfg: LimitConfig,
    ) -> Self {
        let limit = LimitAlarm::create_non_exclusive_in_address_space(
            address_space,
            ns,
            device,
            alarm_name,
            source_node_id,
            cfg,
            LimitAlarmKind::Deviation,
        );
        Self::from_limit(limit)
    }

    fn from_limit(limit: LimitAlarm) -> Self {
        Self {
            limit,
            setpoint_node_property_id: NodeId::null(),
            setpoint_source: NodeId::null(),
            process_value: Mutex::new(0.0),
            setpoint_value: Mutex::new(0.0),
        }
    }

    /// Updates the cached process value and re-evaluates the deviation from the last known
    /// setpoint value, returning an alarm event when the limit state changes.
    pub fn update_process_value(
        &self,
        address_space: &AddressSpace,
        value: f64,
    ) -> Option<AlarmEvent> {
        *self.process_value.lock().unwrap() = value;
        self.recompute(address_space)
    }

    /// Updates the cached setpoint value and re-evaluates the deviation from the last known
    /// process value, returning an alarm event when the limit state changes.
    pub fn update_setpoint_value(
        &self,
        address_space: &AddressSpace,
        value: f64,
    ) -> Option<AlarmEvent> {
        *self.setpoint_value.lock().unwrap() = value;
        self.recompute(address_space)
    }

    fn recompute(&self, address_space: &AddressSpace) -> Option<AlarmEvent> {
        let deviation = *self.process_value.lock().unwrap() - *self.setpoint_value.lock().unwrap();
        self.limit.update_value(address_space, deviation)
    }
}

impl SourceMonitoredAlarm for DeviationAlarm {
    fn source_node(&self) -> &NodeId {
        self.limit.source_node()
    }

    fn condition_id(&self) -> &NodeId {
        &self.limit.condition.condition_id
    }

    fn re_evaluate(&self, address_space: &AddressSpace, value: &DataValue) -> Option<AlarmEvent> {
        source_value_as_f64(value).and_then(|value| self.update_process_value(address_space, value))
    }
}

fn set_node_ref_value(address_space: &AddressSpace, node_id: &NodeId, value: NodeId) {
    if let Some(mut node) = address_space.find_mut(node_id) {
        if let NodeType::Variable(ref mut var) = &mut *node {
            let _ = var.set_value(&NumericRange::None, Variant::from(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarms::limit::{LimitDef, LimitMode};
    use opcua_nodes::DefaultTypeTree;
    use opcua_types::{
        BrowseDirection, DataEncoding, QualifiedName, ReferenceTypeId, TimestampsToReturn,
    };

    fn test_address_space() -> AddressSpace {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 2);
        address_space
    }

    fn deviation_cfg() -> LimitConfig {
        LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 5.0,
                deadband: 0.5,
                severity: 500,
            })
            .build()
            .expect("valid deviation config")
    }

    #[test]
    fn deviation_alarm_activates_when_process_exceeds_setpoint_by_more_than_threshold() {
        let address_space = test_address_space();
        let alarm = DeviationAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "Deviation",
            NodeId::new(2, "InitialSource"),
            deviation_cfg(),
        );

        // Setpoint is 100; deviation threshold is 5. A process value of 103 (deviation = 3) must
        // not activate the alarm, but 106 (deviation = 6) must.
        assert!(alarm.update_setpoint_value(&address_space, 100.0).is_none());
        assert!(alarm.update_process_value(&address_space, 103.0).is_none());
        assert!(!alarm.condition_state_machine().get_active(&address_space));

        let event = alarm
            .update_process_value(&address_space, 106.0)
            .expect("deviation beyond threshold should activate the alarm");
        assert!(event.active_state);
        assert!(alarm.condition_state_machine().get_active(&address_space));
        assert_eq!(
            event.event_type,
            NodeId::from(opcua_types::ObjectTypeId::ExclusiveDeviationAlarmType)
        );
    }

    #[test]
    fn write_setpoint_node_property_creates_and_updates_condition_property() {
        let address_space = test_address_space();
        let mut alarm = DeviationAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "Deviation",
            NodeId::new(2, "InitialSource"),
            deviation_cfg(),
        );
        let condition_id = alarm.condition_state_machine().condition_id.clone();
        let setpoint = NodeId::new(2, "DeviceA.Setpoint");

        alarm.write_setpoint_node_property(&address_space, &setpoint);

        assert_eq!(alarm.setpoint_node(), &setpoint);
        let type_tree = DefaultTypeTree::new();
        let property_id = address_space
            .find_node_by_browse_name(
                &condition_id,
                Some((ReferenceTypeId::HasProperty, false)),
                &type_tree,
                BrowseDirection::Forward,
                QualifiedName::new(0, SETPOINT_NODE_PROPERTY_NAME),
            )
            .map(|node| node.as_node().node_id().clone())
            .expect("SetpointNode property should exist");

        let node = address_space
            .find(&property_id)
            .expect("SetpointNode property should exist");
        let NodeType::Variable(var) = &*node else {
            panic!("SetpointNode property should be a variable");
        };
        let value = var
            .value(
                TimestampsToReturn::Neither,
                &NumericRange::None,
                &DataEncoding::Binary,
                0.0,
            )
            .value;
        assert_eq!(value, Some(Variant::from(setpoint)));
    }
}
