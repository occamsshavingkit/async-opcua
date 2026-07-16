//! Rate-of-change alarm evaluation and address-space wiring for
//! `ExclusiveRateOfChangeAlarmType` and `NonExclusiveRateOfChangeAlarmType` (OPC-10000-9 §5.8.23).

use crate::address_space::AddressSpace;
use crate::alarms::limit::{LimitAlarm, LimitAlarmKind, LimitConfig};
use crate::alarms::source_monitor::{source_value_as_f64, SourceMonitoredAlarm};
use crate::alarms::state_machine::ConditionStateMachine;
use opcua_core::events::AlarmEvent;
use opcua_types::{DataValue, DateTime, NodeId};
use std::sync::Mutex;

/// 100ns per OPC UA `DateTime` tick (Part 6 §5.1.3).
const TICKS_PER_SECOND: f64 = 10_000_000.0;

/// Address-space nodes and runtime state for a rate-of-change alarm: reuses `LimitAlarm`'s
/// threshold/deadband evaluator, but evaluates the process value's rate of change (per second)
/// between successive samples rather than the raw value (OPC-10000-9 §5.8.23).
#[derive(Debug)]
pub struct RateOfChangeAlarm {
    limit: LimitAlarm,
    prev_sample: Mutex<Option<(f64, DateTime)>>,
}

impl RateOfChangeAlarm {
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

    /// Ensures the AlarmConditionType InputNode property exists and writes the source NodeId.
    pub fn write_input_node_property(&mut self, address_space: &AddressSpace, source: &NodeId) {
        self.limit.write_input_node_property(address_space, source);
    }

    /// Ensures the bound source has a forward HasCondition reference to this condition.
    pub fn write_has_condition_reference(&self, address_space: &AddressSpace, source: &NodeId) {
        self.limit
            .write_has_condition_reference(address_space, source);
    }

    /// Creates an `ExclusiveRateOfChangeAlarmType` instance and its `LimitState` nodes.
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
            LimitAlarmKind::RateOfChange,
        );
        Self::from_limit(limit)
    }

    /// Creates a `NonExclusiveRateOfChangeAlarmType` instance and its limit state nodes.
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
            LimitAlarmKind::RateOfChange,
        );
        Self::from_limit(limit)
    }

    fn from_limit(limit: LimitAlarm) -> Self {
        Self {
            limit,
            prev_sample: Mutex::new(None),
        }
    }

    /// Updates with a new process sample taken at `now`, computing the rate of change (per
    /// second) since the previous sample and evaluating it against the configured thresholds.
    /// The first sample establishes a baseline (rate 0, never activates).
    pub fn update_value(
        &self,
        address_space: &AddressSpace,
        value: f64,
        now: DateTime,
    ) -> Option<AlarmEvent> {
        let rate = {
            let mut prev = self.prev_sample.lock().unwrap();
            let rate = match *prev {
                Some((prev_value, prev_time)) => {
                    let dt_seconds = (now.ticks() - prev_time.ticks()) as f64 / TICKS_PER_SECOND;
                    if dt_seconds > 0.0 {
                        (value - prev_value) / dt_seconds
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            };
            *prev = Some((value, now));
            rate
        };

        self.limit.update_value(address_space, rate)
    }
}

impl SourceMonitoredAlarm for RateOfChangeAlarm {
    fn source_node(&self) -> &NodeId {
        self.limit.source_node()
    }

    fn condition_id(&self) -> &NodeId {
        &self.limit.condition.condition_id
    }

    fn re_evaluate(&self, address_space: &AddressSpace, value: &DataValue) -> Option<AlarmEvent> {
        let now = value
            .source_timestamp
            .or(value.server_timestamp)
            .unwrap_or_else(DateTime::now);
        source_value_as_f64(value).and_then(|value| self.update_value(address_space, value, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarms::limit::{LimitDef, LimitMode};

    fn test_address_space() -> AddressSpace {
        let address_space = AddressSpace::new();
        address_space.add_namespace("http://opcfoundation.org/UA/", 0);
        address_space.add_namespace("urn:test", 2);
        address_space
    }

    fn rate_cfg() -> LimitConfig {
        LimitConfig::new(LimitMode::Exclusive)
            .with_high(LimitDef {
                value: 10.0, // 10 units/second
                deadband: 1.0,
                severity: 500,
            })
            .build()
            .expect("valid rate config")
    }

    #[test]
    fn first_sample_establishes_baseline_without_activating() {
        let address_space = test_address_space();
        let alarm = RateOfChangeAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "RateOfChange",
            NodeId::new(2, "InitialSource"),
            rate_cfg(),
        );

        let event = alarm.update_value(&address_space, 100.0, DateTime::now());
        assert!(
            event.is_none(),
            "the first sample has no prior baseline and must not activate"
        );
    }

    #[test]
    fn rate_of_change_activates_when_value_changes_faster_than_threshold() {
        let address_space = test_address_space();
        let alarm = RateOfChangeAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "RateOfChange",
            NodeId::new(2, "InitialSource"),
            rate_cfg(),
        );

        let t0 = DateTime::now();
        // 1 second later. Ticks are 100ns units (10,000,000 per second).
        let t1 = DateTime::from(t0.checked_ticks() + 10_000_000);

        assert!(alarm.update_value(&address_space, 0.0, t0).is_none());
        // Rate = (105 - 0) / 1s = 105 units/s, well above the 10 units/s threshold.
        let event = alarm
            .update_value(&address_space, 105.0, t1)
            .expect("rate above threshold should activate the alarm");
        assert!(event.active_state);
        assert!(alarm.condition_state_machine().get_active(&address_space));
    }

    #[test]
    fn rate_of_change_stays_inactive_below_threshold() {
        let address_space = test_address_space();
        let alarm = RateOfChangeAlarm::create_exclusive_in_address_space(
            &address_space,
            2,
            "DeviceA",
            "RateOfChange",
            NodeId::new(2, "InitialSource"),
            rate_cfg(),
        );

        let t0 = DateTime::now();
        let t1 = DateTime::from(t0.checked_ticks() + 10_000_000);

        alarm.update_value(&address_space, 0.0, t0);
        // Rate = (5 - 0) / 1s = 5 units/s, below the 10 units/s threshold.
        let event = alarm.update_value(&address_space, 5.0, t1);
        assert!(
            event.is_none(),
            "rate below threshold must not activate the alarm"
        );
        assert!(!alarm.condition_state_machine().get_active(&address_space));
    }
}
