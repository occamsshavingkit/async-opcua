use std::collections::BTreeSet;

use super::MonitoredItem;

pub(super) type TriggeredItems = BTreeSet<u32>;

impl MonitoredItem {
    /// Adds or removes other monitored items which will be triggered when this monitored item changes.
    pub(in crate::subscriptions) fn set_triggering(
        &mut self,
        items_to_add: &[u32],
        items_to_remove: &[u32],
    ) {
        // Spec says to process remove items before adding new ones.
        items_to_remove.iter().for_each(|i| {
            self.triggered_items.remove(i);
        });
        items_to_add.iter().for_each(|i| {
            self.triggered_items.insert(*i);
        });
    }

    pub(in crate::subscriptions) fn remove_dead_trigger(&mut self, id: u32) {
        self.triggered_items.remove(&id);
    }

    /// Return `true` if this item has any new notifications.
    /// Note that this clears the `any_new_notification` flag and should be used with care.
    pub(in crate::subscriptions) fn has_new_notifications(&mut self) -> bool {
        let any_new = self.any_new_notification;
        self.any_new_notification = false;
        any_new
    }

    /// Items that are triggered by updates to this monitored item.
    pub fn triggered_items(&self) -> &BTreeSet<u32> {
        &self.triggered_items
    }
}
