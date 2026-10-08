//! App configuration for the passive crafting observation probe.
use super::UiRuntime;

impl UiRuntime {
    /// Passes the app-owned opt-in marker to the domain observation probe.
    pub fn configure_crafting_observation(address: Option<&str>) {
        let marker = std::env::var(crate::diagnostic_markers::CRAFT_OBSERVATION).ok();
        inventory::InventorySession::configure_crafting_observation(address, marker.as_deref());
    }

    /// Retires the observation probe at the existing terminal boundary.
    pub fn retire_crafting_observation() {
        inventory::InventorySession::retire_crafting_observation();
    }

    /// Samples the domain owner after the FIFO drain completes.
    pub(super) fn sample_crafting_observation(&self, player_runtime: &player_state::PlayerState) {
        player_runtime
            .inventory
            .sample_crafting_observation(self.inventory_open);
    }
}
