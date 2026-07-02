use opcua_types::{
    DataChangeFilter, DataChangeTrigger, DataValue, DeadbandType, StatusCode, Variant,
};

#[derive(Debug, Clone)]
/// Parsed percent deadband. Percent deadband applies a threshold based on EURange.
struct PercentDeadband {
    low: f64,
    range: f64,
    trigger: f64,
}

impl PercentDeadband {
    /// Recompute the EURange parameters used by this percent deadband.
    fn update_eu_range(&mut self, low: f64, high: f64) {
        debug_assert!(low < high);
        self.low = low;
        self.range = high - low;
    }
}

#[derive(Debug, Clone)]
/// Deadband type used in data-change filters.
enum Deadband {
    /// No deadband.
    None,
    /// A positive absolute-change threshold.
    Absolute(f64),
    /// A percent-of-EURange threshold.
    Percent(PercentDeadband),
}

impl Deadband {
    /// Recompute EURange parameters if this is a percent deadband.
    fn update_eu_range(&mut self, low: f64, high: f64) {
        if let Deadband::Percent(percent_deadband) = self {
            percent_deadband.update_eu_range(low, high);
        }
    }

    fn is_changed_option(&self, v1: Option<&Variant>, v2: Option<&Variant>) -> bool {
        match (v1, v2) {
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
            (Some(v1), Some(v2)) => self.is_changed(v1, v2),
        }
    }

    fn is_changed(&self, v1: &Variant, v2: &Variant) -> bool {
        if let (Some(v1), Some(v2)) = (v1.as_array(), v2.as_array()) {
            // OPC 10000-4 §7.22.2: arrays change if their size/dimensions change or any element
            // exceeds the deadband.
            if v1.len() != v2.len() {
                return true;
            }

            v1.iter()
                .zip(v2.iter())
                .any(|(v1, v2)| self.is_changed(v1, v2))
        } else {
            match self {
                Deadband::None => v1 != v2,
                Deadband::Absolute(deadband) => {
                    let (Some(v1), Some(v2)) = (v1.as_f64(), v2.as_f64()) else {
                        return true;
                    };
                    (v1 - v2).abs() > *deadband
                }
                Deadband::Percent(percent_deadband) => {
                    let (Some(v1), Some(v2)) = (v1.as_f64(), v2.as_f64()) else {
                        return true;
                    };
                    let v1_pct = (v1 - percent_deadband.low) / percent_deadband.range;
                    let v2_pct = (v2 - percent_deadband.low) / percent_deadband.range;
                    (v1_pct - v2_pct).abs() > percent_deadband.trigger
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
/// Parsed variant of DataChangeFilter that can be evaluated directly.
pub struct ParsedDataChangeFilter {
    /// What is considered a change.
    pub trigger: DataChangeTrigger,
    /// Deadband filter.
    deadband: Deadband,
}

impl ParsedDataChangeFilter {
    /// Recompute percent-deadband EURange parameters, if this filter uses one.
    pub(super) fn update_eu_range(&mut self, low: f64, high: f64) {
        self.deadband.update_eu_range(low, high);
    }

    /// Check whether this data-change filter considers `new` different from `old`.
    pub(super) fn is_changed(&self, new: &DataValue, old: &DataValue) -> bool {
        match self.trigger {
            DataChangeTrigger::Status => new.status != old.status,
            DataChangeTrigger::StatusValue => {
                new.status != old.status
                    || self
                        .deadband
                        .is_changed_option(new.value.as_ref(), old.value.as_ref())
            }
            DataChangeTrigger::StatusValueTimestamp => {
                new.status != old.status
                    || new.source_timestamp != old.source_timestamp
                    || new.source_picoseconds != old.source_picoseconds
                    || self
                        .deadband
                        .is_changed_option(new.value.as_ref(), old.value.as_ref())
            }
        }
    }

    /// Parse a raw data-change filter and optional node EURange.
    pub(super) fn parse(
        filter: DataChangeFilter,
        eu_range: Option<(f64, f64)>,
    ) -> Result<Self, StatusCode> {
        let as_int: i32 = filter
            .deadband_type
            .try_into()
            .map_err(|_| StatusCode::BadDeadbandFilterInvalid)?;
        let ty =
            DeadbandType::try_from(as_int).map_err(|_| StatusCode::BadDeadbandFilterInvalid)?;
        let deadband = match ty {
            DeadbandType::None => Deadband::None,
            DeadbandType::Absolute => {
                if filter.deadband_value < 0.0 {
                    return Err(StatusCode::BadDeadbandFilterInvalid);
                }
                Deadband::Absolute(filter.deadband_value)
            }
            DeadbandType::Percent => {
                if filter.deadband_value < 0.0 || filter.deadband_value > 100.0 {
                    return Err(StatusCode::BadDeadbandFilterInvalid);
                }
                let Some((low, high)) = eu_range else {
                    return Err(StatusCode::BadDeadbandFilterInvalid);
                };
                if low >= high {
                    return Err(StatusCode::BadDeadbandFilterInvalid);
                }
                Deadband::Percent(PercentDeadband {
                    low,
                    range: high - low,
                    trigger: filter.deadband_value / 100.0,
                })
            }
        };
        Ok(Self {
            trigger: filter.trigger,
            deadband,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeDelta, Utc};
    use opcua_types::{
        AttributeId, DataChangeFilter, DataChangeTrigger, DataValue, DeadbandType, MonitoringMode,
        NodeId, ReadValueId, Variant,
    };

    use super::{Deadband, ParsedDataChangeFilter};
    use crate::subscriptions::monitored_item::{
        tests::new_monitored_item, FilterType, Notification, SamplingInterval,
    };

    #[test]
    fn data_change_deadband_abs() {
        let filter = DataChangeFilter {
            trigger: DataChangeTrigger::StatusValue,
            deadband_type: DeadbandType::Absolute as u32,
            deadband_value: 1f64,
        };
        let filter = ParsedDataChangeFilter::parse(filter, None).unwrap();

        let v1 = DataValue {
            value: Some(Variant::Double(10f64)),
            status: None,
            source_timestamp: None,
            source_picoseconds: None,
            server_timestamp: None,
            server_picoseconds: None,
        };

        let mut v2 = DataValue {
            value: Some(Variant::Double(10f64)),
            status: None,
            source_timestamp: None,
            source_picoseconds: None,
            server_timestamp: None,
            server_picoseconds: None,
        };

        assert!(!filter.is_changed(&v2, &v1));

        v2.value = Some(Variant::Double(10.9f64));
        assert!(!filter.is_changed(&v2, &v1));

        v2.value = Some(Variant::Double(11f64));
        assert!(!filter.is_changed(&v2, &v1));

        v2.value = Some(Variant::Double(11.00001f64));
        assert!(filter.is_changed(&v2, &v1));
    }

    #[test]
    fn percent_deadband_uses_refreshed_eu_range() {
        let start = Utc::now();
        let filter = DataChangeFilter {
            trigger: DataChangeTrigger::StatusValue,
            deadband_type: DeadbandType::Percent as u32,
            deadband_value: 10f64,
        };
        let filter = ParsedDataChangeFilter::parse(filter, Some((0.0, 100.0))).unwrap();
        let mut item = new_monitored_item(
            1,
            ReadValueId {
                node_id: NodeId::null(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            },
            MonitoringMode::Reporting,
            FilterType::DataChangeFilter(filter),
            SamplingInterval::Zero,
            true,
            Some(DataValue::new_at(0.0, start.into())),
        );
        item.eu_range = Some((0.0, 100.0));
        item.notification_queue.clear();

        let t = start + Duration::try_milliseconds(100).unwrap();
        assert!(!item.notify_data_value(DataValue::new_at(9.0, t.into()), &t.into(), false));
        assert_eq!(item.notification_queue.len(), 0);

        item.notify_eu_range_changed(0.0, 1000.0);
        assert_eq!(item.eu_range, Some((0.0, 1000.0)));

        let t = start + Duration::try_milliseconds(200).unwrap();
        assert!(!item.notify_data_value(DataValue::new_at(50.0, t.into()), &t.into(), false));
        assert_eq!(item.notification_queue.len(), 0);

        let t = start + Duration::try_milliseconds(300).unwrap();
        assert!(item.notify_data_value(DataValue::new_at(101.0, t.into()), &t.into(), false));
        assert_eq!(item.notification_queue.len(), 1);
        let Some(Notification::MonitoredItemNotification(n)) = item.notification_queue.back()
        else {
            panic!("Wrong notification type");
        };
        assert!(n.value.status().semantics_changed());

        let t = start + Duration::try_milliseconds(400).unwrap();
        assert!(item.notify_data_value(DataValue::new_at(202.0, t.into()), &t.into(), false));
        assert_eq!(item.notification_queue.len(), 2);
        let Some(Notification::MonitoredItemNotification(n)) = item.notification_queue.back()
        else {
            panic!("Wrong notification type");
        };
        assert!(!n.value.status().semantics_changed());
    }

    #[test]
    fn monitored_item_deadband_filter() {
        let start = Utc::now();
        let mut item = new_monitored_item(
            1,
            ReadValueId {
                node_id: NodeId::null(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            },
            MonitoringMode::Reporting,
            FilterType::DataChangeFilter(ParsedDataChangeFilter {
                trigger: DataChangeTrigger::StatusValue,
                deadband: Deadband::Absolute(0.9),
            }),
            SamplingInterval::NonZero(TimeDelta::milliseconds(100)),
            true,
            Some(DataValue::new_at(1.0, start.into())),
        );

        assert!(item.notify_data_value(
            DataValue::new_at(
                2.0,
                (start + Duration::try_milliseconds(50).unwrap()).into()
            ),
            &start.into(),
            false
        ));
        assert_eq!(1, item.notification_queue.len());
        assert!(item.sample_skipped_data_value.is_some());

        assert!(!item.notify_data_value(
            DataValue::new_at(
                1.5,
                (start + Duration::try_milliseconds(100).unwrap()).into()
            ),
            &start.into(),
            false
        ));

        item.set_monitoring_mode(MonitoringMode::Disabled);
        assert!(!item.notify_data_value(
            DataValue::new_at(
                3.0,
                (start + Duration::try_milliseconds(250).unwrap()).into()
            ),
            &start.into(),
            false
        ));
        item.set_monitoring_mode(MonitoringMode::Reporting);

        assert!(item.notify_data_value(
            DataValue::new_at(
                2.0,
                (start + Duration::try_milliseconds(100).unwrap()).into()
            ),
            &start.into(),
            false
        ));
        assert!(!item.notify_data_value(
            DataValue::new_at(
                2.5,
                (start + Duration::try_milliseconds(200).unwrap()).into()
            ),
            &start.into(),
            false
        ));
        assert!(item.notify_data_value(
            DataValue::new_at(
                3.0,
                (start + Duration::try_milliseconds(250).unwrap()).into()
            ),
            &start.into(),
            false
        ));
        assert_eq!(item.notification_queue.len(), 3);
    }
}
