use std::sync::Arc;

use protocol::{InventoryEvent, ItemRegistryEvent};

use super::UiRuntime;

impl UiRuntime {
    pub(crate) fn selected_hotbar_slot(&self) -> Option<u8> {
        self.local_selected_equipment
            .as_ref()
            .map(|equipment| equipment.event.selected_slot)
            .or_else(|| {
                self.player_game_mode
                    .filter(|game_mode| game_mode.shows_hotbar())
                    .map(|_| 0)
            })
    }

    pub(crate) fn apply_item_registry(&mut self, registry: &ItemRegistryEvent) {
        self.item_registry.clear();
        for entry in registry.entries.iter() {
            if entry.network_id > 0 {
                self.item_registry
                    .insert(entry.network_id, Arc::clone(&entry.identifier));
            }
        }
    }

    pub(crate) fn item_identifier(&self, network_id: i32) -> Option<&str> {
        self.item_registry.get(&network_id).map(Arc::as_ref)
    }

    pub(crate) const fn hotbar(&self) -> &[protocol::NetworkItemStack; 9] {
        &self.hotbar
    }

    pub(super) fn apply_inventory_visual_state(&mut self, event: &InventoryEvent) {
        match event {
            InventoryEvent::Content(content) if content.container.window_id == Some(0) => {
                for (slot, stack) in content.slots.iter().take(self.hotbar.len()).enumerate() {
                    self.hotbar[slot] = stack.clone();
                }
            }
            InventoryEvent::Slot(slot)
                if slot.identity.container.window_id == Some(0)
                    && usize::from(slot.identity.slot) < self.hotbar.len() =>
            {
                self.hotbar[usize::from(slot.identity.slot)] = slot.stack.clone();
            }
            _ => {}
        }
    }
}
