//! Inventory controllers retain snapshots and save whole tuples through the bounded queue.

use inventory::{InventoryContext, InventoryController, inventory_ledger::InventoryGestureError};
use protocol::{InventoryLayout, InventoryRightTab, InventoryTab, PlayerGameMode, WindowKind};
use ui_screens::container_actions::{SEARCH_TAB, inventory_tab, inventory_tab_ui};

use super::UiRuntime;

/// Resolves the Classic controller owner without treating unrelated stations as crafting tables.
fn context(player: &player_state::PlayerState) -> Option<InventoryContext> {
    match player.inventory.ledger().window_kind() {
        None => Some(InventoryContext::Personal),
        Some(WindowKind::Workbench) => Some(InventoryContext::Workbench),
        _ => None,
    }
}

impl UiRuntime {
    /// Reads this screen's captured tuple, or its initial choices before window admission.
    pub fn inventory_controller(
        &self,
        player: &player_state::PlayerState,
    ) -> Option<InventoryController> {
        let context = context(player)?;
        Some(
            self.screen
                .inventory
                .filter(|controller| {
                    controller.context == context
                        && self
                            .screen
                            .observes_window(player.inventory.ledger().storage_generation())
                })
                .unwrap_or_else(|| {
                    InventoryController::new(
                        player.inventory.inventory_options(),
                        context,
                        player.facts.player_game_mode() == Some(PlayerGameMode::Creative),
                    )
                }),
        )
    }

    /// Captures preferences at screen admission without touching pointer or station input state.
    pub(super) fn initialize_inventory_controller(&mut self, player: &player_state::PlayerState) {
        if self.screen.inventory.is_none()
            && let Some(controller) = self.inventory_controller(player)
        {
            self.screen.set_inventory_controller(controller);
        }
    }

    /// Saves changed choices atomically, accounting for preference packets still awaiting transport.
    pub(super) fn change_inventory_options(
        &mut self,
        player: &mut player_state::PlayerState,
        save: bool,
        change: impl FnOnce(&mut InventoryController),
    ) -> Result<i32, InventoryGestureError> {
        let previous = self
            .inventory_controller(player)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let mut next = previous;
        change(&mut next);
        if next == previous {
            return Ok(0);
        }
        if save {
            let packet = protocol::inventory_options_packet(next.options);
            let pending = self
                .client_packets
                .iter()
                .rev()
                .find(|queued| queued.kind() == packet.kind());
            let needs_send = next.options != player.inventory.inventory_options()
                || pending.is_some_and(|queued| *queued != packet);
            if needs_send && !self.queue_client_packet(packet) {
                return Err(InventoryGestureError::Busy);
            }
            player.inventory.set_inventory_options(next.options);
        }
        self.screen.set_inventory_controller(next);
        Ok(0)
    }

    /// Selects a known left tab and focuses Search only after the change is admitted.
    pub(super) fn select_inventory_tab(
        &mut self,
        player: &mut player_state::PlayerState,
        tab: u8,
    ) -> Result<i32, InventoryGestureError> {
        let selected = inventory_tab(tab).ok_or(InventoryGestureError::InvalidRequest)?;
        let previous = self
            .inventory_controller(player)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        if previous.options.left_tab == selected {
            if selected == InventoryTab::Search {
                self.screen.search_focused = true;
            }
            return Ok(0);
        }
        let result = self.change_inventory_options(player, true, |controller| {
            controller.options.left_tab = selected
        });
        if result.is_ok() {
            self.screen.select_tab(tab);
        }
        result
    }

    /// Rejects layouts that this Classic controller does not offer.
    pub(super) fn select_inventory_layout(
        &mut self,
        player: &mut player_state::PlayerState,
        index: u8,
    ) -> Result<i32, InventoryGestureError> {
        let layout = InventoryLayout::from_index(i32::from(index));
        let controller = self
            .inventory_controller(player)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let creative = player.facts.player_game_mode() == Some(PlayerGameMode::Creative);
        if !matches!(
            layout,
            InventoryLayout::InventoryOnly | InventoryLayout::RecipeBook
        ) && !(layout == InventoryLayout::RecipeBookOnly
            && controller.allows_recipe_book_only(creative))
        {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if controller.layout() == layout {
            return Ok(0);
        }
        let result =
            self.change_inventory_options(player, true, |controller| controller.set_layout(layout));
        if result.is_ok() && layout == InventoryLayout::InventoryOnly {
            self.screen.search_focused = false;
        }
        result
    }

    /// Selects a right panel, retaining unknown stored values until a valid control changes them.
    pub(super) fn select_inventory_right_tab(
        &mut self,
        player: &mut player_state::PlayerState,
        index: u8,
    ) -> Result<i32, InventoryGestureError> {
        let tab = InventoryRightTab::from_index(i32::from(index));
        let creative = player.facts.player_game_mode() == Some(PlayerGameMode::Creative);
        if !matches!(tab, InventoryRightTab::Crafting | InventoryRightTab::Armor)
            && !(creative && tab == InventoryRightTab::Fullscreen)
        {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.change_inventory_options(player, true, |controller| {
            controller.options.right_tab = tab
        })
    }

    /// Projects the resolved controller's left tab into the existing category order.
    pub fn creative_tab(&self, player: &player_state::PlayerState) -> u8 {
        inventory_tab_ui(
            self.inventory_controller(player)
                .map_or(InventoryTab::Search, |controller| {
                    controller.options.left_tab
                }),
        )
        .unwrap_or(SEARCH_TAB)
    }
}
