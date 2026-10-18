//! A player mod's vanilla inventory requests, made as the creative catalog and the recipe book
//! make them: the same ledger requests, which the server checks.

use inventory::inventory_ledger::{
    CreativeDestination, InventoryGestureError, InventoryTarget, PlayerInventorySlot,
};

use super::UiRuntime;

type Outcome = Result<i32, InventoryGestureError>;

impl UiRuntime {
    /// Takes `count` of the creative item `identifier` with `aux` into the first free inventory
    /// slot, else the empty cursor. Refused outside Creative and for an item the creative
    /// content lacks; a held stack is never replaced.
    pub fn mod_creative_take(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        identifier: &str,
        aux: u16,
        count: u8,
    ) -> Outcome {
        if player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let ledger = player_runtime.inventory.ledger();
        if ledger.cursor_stack().is_some() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let registry = self
            .inventory_ledger(player_runtime)
            .negotiated_item_registry()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let id = self
            .inventory_ledger(player_runtime)
            .creative_catalog()
            .and_then(|catalog| {
                catalog.items.iter().find(|item| {
                    item.stack.metadata == u32::from(aux)
                        && registry
                            .get(&item.stack.network_id)
                            .is_some_and(|entry| *entry.identifier == *identifier)
                })
            })
            .map(|item| item.creative_network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let destination = (0..protocol::PLAYER_INVENTORY_SLOTS)
            .find(|slot| {
                ledger
                    .target_stack(InventoryTarget::Player(*slot))
                    .is_none()
                    && matches!(ledger.slot_state(*slot), Some(PlayerInventorySlot::Empty))
            })
            .map_or(CreativeDestination::Cursor, CreativeDestination::Player);
        self.inventory_ledger_mut(player_runtime)
            .begin_creative_take(id, destination, Some(count))
    }

    /// Crafts crafting recipe `network_id` once from inventory ingredients onto the cursor, as
    /// a recipe book click does.
    pub fn mod_fill_recipe(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        network_id: u32,
    ) -> Outcome {
        let recipe = self
            .screen_catalog(player_runtime)
            .and_then(|catalog| {
                catalog
                    .crafting_recipes()
                    .find(|recipe| recipe.network_id() == network_id)
                    .cloned()
            })
            .ok_or(InventoryGestureError::InvalidRequest)?;
        player_runtime.inventory.begin_book_craft(&recipe)
    }
}
