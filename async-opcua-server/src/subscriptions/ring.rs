use opcua_nodes::Event;
use opcua_types::DataValue;

use super::MonitoredItemHandle;

#[allow(dead_code)]
pub(crate) enum NotificationWorkItem {
    Data {
        handle: MonitoredItemHandle,
        value: DataValue,
    },
    RangeChanged {
        handle: MonitoredItemHandle,
        low: f64,
        high: f64,
    },
    Event {
        handle: MonitoredItemHandle,
        event: Box<dyn Event + Send>,
    },
    Refresh {
        subscription_id: u32,
        monitored_item: Option<MonitoredItemHandle>,
        events: Vec<Box<dyn Event + Send>>,
    },
}
