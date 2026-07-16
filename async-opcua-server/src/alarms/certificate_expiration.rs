//! `CertificateExpirationAlarmType` evaluation and address-space wiring (OPC-10000-9 §5.8.24.7).
//! A `SystemOffNormalAlarmType` subtype raised when a Certificate is within its configured
//! `ExpirationLimit` of expiring; returns to normal automatically once the certificate is
//! updated to a later `ExpirationDate`.

use crate::address_space::{AddressSpace, VariableBuilder};
use crate::alarms::replace_condition_type_definition;
use crate::alarms::state_machine::ConditionStateMachine;
use opcua_core::events::AlarmEvent;
use opcua_nodes::NodeType;
use opcua_types::{
    ByteString, DataTypeId, DateTime, LocalizedText, NodeId, NumericRange, ObjectTypeId,
    TimestampsToReturn, VariableTypeId, Variant,
};
use std::sync::Mutex;

const ACTIVE_SEVERITY: u16 = 600;
const INACTIVE_SEVERITY: u16 = 0;
/// 100ns per OPC UA `DateTime` tick (Part 6 §5.1.3); OPC UA `Duration` properties are
/// milliseconds (Part 3 §8.13).
const TICKS_PER_MS: f64 = 10_000.0;

/// Address-space nodes and runtime state for a `CertificateExpirationAlarmType` instance.
#[derive(Debug)]
pub struct CertificateExpirationAlarm {
    /// Base A&C lifecycle state machine.
    pub condition: ConditionStateMachine,
    expiration_date_id: NodeId,
    expiration_limit_id: NodeId,
    prev_active: Mutex<bool>,
}

impl CertificateExpirationAlarm {
    /// Returns the base condition state machine for registry integration.
    #[must_use]
    pub fn condition_state_machine(&self) -> ConditionStateMachine {
        self.condition.clone()
    }

    /// Creates a `CertificateExpirationAlarmType` instance and its `ExpirationDate`/
    /// `ExpirationLimit`/`CertificateType`/`Certificate` properties.
    ///
    /// `expiration_limit_ms` is the time interval before `ExpirationDate` at which this alarm
    /// triggers (OPC-10000-9 §5.8.24.7 recommends a default of two weeks when unspecified).
    pub fn create_in_address_space(
        address_space: &AddressSpace,
        ns: u16,
        device: &str,
        alarm_name: &str,
        source_node_id: NodeId,
        expiration_date: DateTime,
        expiration_limit_ms: f64,
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
            NodeId::from(ObjectTypeId::CertificateExpirationAlarmType),
        );

        let base_s = format!("Alarm_{}_{}", device, alarm_name);
        let expiration_date_id = NodeId::new(ns, format!("{}_ExpirationDate", base_s));
        let expiration_limit_id = NodeId::new(ns, format!("{}_ExpirationLimit", base_s));
        let certificate_type_id = NodeId::new(ns, format!("{}_CertificateType", base_s));
        let certificate_id = NodeId::new(ns, format!("{}_Certificate", base_s));

        VariableBuilder::new(&expiration_date_id, "ExpirationDate", "ExpirationDate")
            .data_type(DataTypeId::DateTime)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(expiration_date)
            .writable()
            .property_of(condition.condition_id.clone())
            .insert(address_space);

        VariableBuilder::new(&expiration_limit_id, "ExpirationLimit", "ExpirationLimit")
            .data_type(DataTypeId::Duration)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(expiration_limit_ms)
            .writable()
            .property_of(condition.condition_id.clone())
            .insert(address_space);

        VariableBuilder::new(&certificate_type_id, "CertificateType", "CertificateType")
            .data_type(DataTypeId::NodeId)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(NodeId::null())
            .writable()
            .property_of(condition.condition_id.clone())
            .insert(address_space);

        VariableBuilder::new(&certificate_id, "Certificate", "Certificate")
            .data_type(DataTypeId::ByteString)
            .has_type_definition(VariableTypeId::PropertyType)
            .value(ByteString::null())
            .writable()
            .property_of(condition.condition_id.clone())
            .insert(address_space);

        Self {
            condition,
            expiration_date_id,
            expiration_limit_id,
            prev_active: Mutex::new(false),
        }
    }

    /// Writes a new `ExpirationDate` (e.g. after the certificate is renewed).
    pub fn set_expiration_date(&self, address_space: &AddressSpace, expiration_date: DateTime) {
        set_variable_value(
            address_space,
            &self.expiration_date_id,
            Variant::from(expiration_date),
        );
    }

    /// Evaluates the alarm at time `now` against the configured `ExpirationDate`/
    /// `ExpirationLimit`, returning an alarm event when the active state changes.
    pub fn evaluate(&self, address_space: &AddressSpace, now: DateTime) -> Option<AlarmEvent> {
        if !self.condition.get_enabled(address_space) {
            return None;
        }

        let expiration_date = read_date_time(address_space, &self.expiration_date_id);
        let expiration_limit_ms = read_double(address_space, &self.expiration_limit_id);
        let remaining_ms =
            (expiration_date.checked_ticks() - now.checked_ticks()) as f64 / TICKS_PER_MS;
        let active = remaining_ms <= expiration_limit_ms;

        {
            let mut prev_active = self.prev_active.lock().unwrap();
            if active == *prev_active {
                return None;
            }
            *prev_active = active;
        }

        let severity = if active {
            ACTIVE_SEVERITY
        } else {
            INACTIVE_SEVERITY
        };
        let message = LocalizedText::new(
            "en",
            if active {
                "Certificate is within its expiration limit"
            } else {
                "Certificate expiration normal"
            },
        );

        let was_active = self.condition.get_active(address_space);
        let was_acked = self.condition.get_acked(address_space);
        if was_active && !was_acked && !active {
            self.condition.create_branch(address_space);
        }
        self.condition.set_active(address_space, active);
        self.condition.set_severity(address_space, severity);
        self.condition.set_message(address_space, message.clone());

        if active {
            self.condition.set_acked(address_space, false);
            self.condition.set_confirmed(address_space, false);
        }

        let acked = self.condition.get_acked(address_space);
        let confirmed = self.condition.get_confirmed(address_space);
        let retain = active || !acked || !confirmed;
        self.condition.set_retain(address_space, retain);

        let event_id = uuid::Uuid::new_v4().as_bytes().to_vec();
        self.condition.set_current_event_id(&event_id);

        Some(AlarmEvent {
            event_id,
            event_type: NodeId::from(ObjectTypeId::CertificateExpirationAlarmType),
            source_node: self.condition.source_node_id.clone(),
            source_name: self.condition.condition_name.clone(),
            time: DateTime::now(),
            message,
            severity,
            condition_id: self.condition.condition_id.clone(),
            branch_id: NodeId::null(),
            condition_name: self.condition.condition_name.clone(),
            active_state: active,
            acked_state: acked,
            confirmed_state: confirmed,
            retain,
        })
    }
}

fn set_variable_value(address_space: &AddressSpace, node_id: &NodeId, value: Variant) {
    if let Some(mut node) = address_space.find_mut(node_id) {
        if let NodeType::Variable(ref mut var) = &mut *node {
            let _ = var.set_value(&NumericRange::None, value);
        }
    }
}

fn read_date_time(address_space: &AddressSpace, node_id: &NodeId) -> DateTime {
    read_value(address_space, node_id)
        .and_then(|value| match value {
            Variant::DateTime(dt) => Some(*dt),
            _ => None,
        })
        .unwrap_or_else(DateTime::now)
}

fn read_double(address_space: &AddressSpace, node_id: &NodeId) -> f64 {
    read_value(address_space, node_id)
        .and_then(|value| match value {
            Variant::Double(v) => Some(v),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn read_value(address_space: &AddressSpace, node_id: &NodeId) -> Option<Variant> {
    let node = address_space.find(node_id)?;
    let NodeType::Variable(ref var) = *node else {
        return None;
    };
    var.value(
        TimestampsToReturn::Neither,
        &NumericRange::None,
        &opcua_types::DataEncoding::Binary,
        0.0,
    )
    .value
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

    const TWO_WEEKS_MS: f64 = 14.0 * 24.0 * 60.0 * 60.0 * 1000.0;

    #[test]
    fn activates_when_within_expiration_limit_and_clears_on_renewal() {
        let address_space = test_address_space();
        let now = DateTime::now();
        // Expires in 1 day; ExpirationLimit is 2 weeks -- should be active immediately.
        let one_day_out = DateTime::from(now.checked_ticks() + 24 * 60 * 60 * 10_000_000);
        let alarm = CertificateExpirationAlarm::create_in_address_space(
            &address_space,
            2,
            "ServerCert",
            "Expiration",
            NodeId::new(2, "ServerCert"),
            one_day_out,
            TWO_WEEKS_MS,
        );

        let event = alarm
            .evaluate(&address_space, now)
            .expect("certificate within its expiration limit should activate");
        assert!(event.active_state);
        assert!(alarm.condition_state_machine().get_active(&address_space));

        // Renew: push the expiration date a year out -- alarm should clear.
        let far_future = DateTime::from(now.checked_ticks() + 365 * 24 * 60 * 60 * 10_000_000);
        alarm.set_expiration_date(&address_space, far_future);
        let event = alarm
            .evaluate(&address_space, now)
            .expect("renewed certificate should deactivate the alarm");
        assert!(!event.active_state);
        assert!(!alarm.condition_state_machine().get_active(&address_space));
    }

    #[test]
    fn stays_inactive_when_outside_expiration_limit() {
        let address_space = test_address_space();
        let now = DateTime::now();
        let far_future = DateTime::from(now.checked_ticks() + 365 * 24 * 60 * 60 * 10_000_000);
        let alarm = CertificateExpirationAlarm::create_in_address_space(
            &address_space,
            2,
            "ServerCert",
            "Expiration",
            NodeId::new(2, "ServerCert"),
            far_future,
            TWO_WEEKS_MS,
        );

        assert!(
            alarm.evaluate(&address_space, now).is_none(),
            "certificate far from expiring must not activate the alarm"
        );
        assert!(!alarm.condition_state_machine().get_active(&address_space));
    }
}
