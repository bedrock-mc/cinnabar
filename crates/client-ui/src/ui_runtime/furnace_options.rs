//! Furnace controllers snapshot player preferences and save explicit changes.

use inventory::inventory_ledger::InventoryGestureError;
use protocol::{FurnaceKind, FurnaceLayout, FurnaceOptions};

use super::UiRuntime;

/// Resolves an unspecified or oversized stored layout when constructing the screen.
fn screen_options(mut options: FurnaceOptions) -> FurnaceOptions {
    if options.layout == FurnaceLayout::None
        || options.layout.index() > FurnaceLayout::RecipeBook.index()
    {
        options.layout = FurnaceLayout::RecipeBook;
    }
    options
}

impl UiRuntime {
    /// Starts one controller per window generation, retaining incoming updates separately.
    pub fn observe_inventory_window(&mut self, player: &player_state::PlayerState) {
        self.screen
            .observe_window(player.inventory.ledger().storage_generation());
        if self.inventory_open() {
            self.initialize_inventory_controller(player);
        } else {
            self.screen.inventory = None;
            self.screen.search_focused = false;
        }
        if self.screen.furnace.is_none()
            && let Some(kind) = player
                .inventory
                .ledger()
                .window_kind()
                .and_then(FurnaceKind::from_window)
        {
            self.screen.furnace = Some(screen_options(player.inventory.furnace_options(kind)));
        }
    }

    /// Reads the open controller's choices, or its initial snapshot before input runs.
    pub fn furnace_options(&self, player: &player_state::PlayerState) -> Option<FurnaceOptions> {
        let kind = player
            .inventory
            .ledger()
            .window_kind()
            .and_then(FurnaceKind::from_window)?;
        Some(
            self.screen
                .furnace
                .filter(|_| {
                    self.screen
                        .observes_window(player.inventory.ledger().storage_generation())
                })
                .unwrap_or_else(|| screen_options(player.inventory.furnace_options(kind))),
        )
    }

    /// Saves changed preferences only after their packet fits the bounded client queue.
    pub(super) fn change_furnace_options(
        &mut self,
        player: &mut player_state::PlayerState,
        change: impl FnOnce(&mut FurnaceOptions),
    ) -> Result<i32, InventoryGestureError> {
        self.observe_inventory_window(player);
        let kind = player
            .inventory
            .ledger()
            .window_kind()
            .and_then(FurnaceKind::from_window)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let mut options = self
            .screen
            .furnace
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let previous = options;
        change(&mut options);
        if options == previous {
            return Ok(0);
        }
        let packet = protocol::furnace_options_packet(kind, options);
        let conflicts_with_pending = self
            .client_packets
            .iter()
            .rev()
            .find(|pending| kind.owns_packet(pending))
            .is_some_and(|pending| pending != &packet);
        let needs_packet =
            options != player.inventory.furnace_options(kind) || conflicts_with_pending;
        if needs_packet && !self.queue_client_packet(packet) {
            return Err(InventoryGestureError::Busy);
        }
        player.inventory.set_furnace_options(kind, options);
        self.screen.furnace = Some(options);
        self.screen.container_scroll.clear();
        Ok(0)
    }
}
