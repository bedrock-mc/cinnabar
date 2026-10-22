//! Recipe book and creative catalog clicks.

use super::*;

impl UiRuntime {
    /// Crafts the fallback book's `index` entry on the current page once, or a stack with `all`.
    pub(super) fn book_widget_craft(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        index: u8,
        all: bool,
    ) -> Outcome {
        let skip = self.screen_state().book_page * BOOK_CELLS + usize::from(index);
        let recipe = self
            .book_recipes(player_runtime, skip, 1)
            .into_iter()
            .next()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        player_runtime
            .inventory
            .begin_book_craft(&recipe.recipe, all)
    }

    /// A click on a recipe book entry: a creative item as on the catalog grid, else
    /// auto-crafting the recipe into the inventory; `into_inventory` (shift) crafts a stack
    /// and `requested` marks a secondary click.
    pub(super) fn recipe_book_click(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        index: u16,
        into_inventory: bool,
        requested: Option<u8>,
    ) -> Outcome {
        if ui_screens::furnace_book::active(player_runtime) {
            let recipe =
                super::super::presentation::forms::furnace_book::entries(player_runtime, self)
                    .get(usize::from(index))
                    .cloned()
                    .ok_or(InventoryGestureError::InvalidRequest)?;
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_furnace_recipe(&recipe);
        }
        let entry = recipe_book_entries(player_runtime, self)
            .into_iter()
            .nth(usize::from(index))
            .map(|entry| match entry {
                BookEntry::Creative { item, .. } => Clicked::Item(item.creative_network_id),
                BookEntry::Group { index, .. } => Clicked::Group(index),
                BookEntry::Recipe(recipe) => Clicked::Recipe(recipe.recipe),
                BookEntry::Furnace(recipe) => Clicked::Furnace(recipe.clone()),
            });
        match entry {
            Some(Clicked::Item(id)) => {
                self.creative_take(player_runtime, id, into_inventory, requested)
            }
            // A head folds or unfolds its group; a held stack still deletes.
            Some(Clicked::Group(_))
                if player_runtime.inventory.ledger().cursor_stack().is_some() =>
            {
                self.inventory_ledger_mut(player_runtime)
                    .begin_destroy_cursor(requested)
            }
            Some(Clicked::Group(index)) => {
                let expanded = &mut self.screen_state_mut().creative_expanded;
                if !expanded.remove(&index) {
                    expanded.insert(index);
                }
                Ok(0)
            }
            // A secondary click crafts once only with an empty cursor.
            Some(Clicked::Recipe(_))
                if requested.is_some()
                    && player_runtime.inventory.ledger().cursor_stack().is_some() =>
            {
                Err(InventoryGestureError::InvalidRequest)
            }
            Some(Clicked::Recipe(recipe)) => player_runtime
                .inventory
                .begin_book_craft(&recipe, into_inventory),
            Some(Clicked::Furnace(recipe)) => self
                .inventory_ledger_mut(player_runtime)
                .begin_furnace_recipe(&recipe),
            None if player_runtime.facts.player_game_mode()
                == Some(protocol::PlayerGameMode::Creative)
                && player_runtime.inventory.ledger().cursor_stack().is_some() =>
            {
                self.inventory_ledger_mut(player_runtime)
                    .begin_destroy_cursor(requested)
            }
            None => Err(InventoryGestureError::EmptyGesture),
        }
    }

    /// A click on a catalog cell: take the item, or delete the held stack.
    pub(super) fn creative_click(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        index: u8,
        into_inventory: bool,
        requested: Option<u8>,
    ) -> Outcome {
        if player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if player_runtime.inventory.ledger().cursor_stack().is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor(requested);
        }
        let id = {
            let entries = visible_creative_entries(player_runtime, self);
            let position = self.screen_state().creative_row * GRID_COLUMNS + usize::from(index);
            if usize::from(index) >= GRID_CELLS {
                return Err(InventoryGestureError::InvalidRequest);
            }
            entries.get(position).map(|item| item.creative_network_id)
        };
        let id = id.ok_or(InventoryGestureError::EmptyGesture)?;
        self.creative_take(player_runtime, id, into_inventory, requested)
    }

    /// Takes catalog item `id` to the cursor (or the first free inventory cell),
    /// or deletes the held stack.
    pub(super) fn creative_take(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        id: u32,
        into_inventory: bool,
        requested: Option<u8>,
    ) -> Outcome {
        if player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if player_runtime.inventory.ledger().cursor_stack().is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor(requested);
        }
        let destination = if into_inventory {
            let ledger = player_runtime.inventory.ledger();
            (0..protocol::PLAYER_INVENTORY_SLOTS)
                .find(|slot| {
                    ledger
                        .target_stack(InventoryTarget::Player(*slot))
                        .is_none()
                        && matches!(
                            ledger.slot_state(*slot),
                            Some(inventory::inventory_ledger::PlayerInventorySlot::Empty)
                        )
                })
                .map_or(CreativeDestination::Cursor, CreativeDestination::Player)
        } else {
            CreativeDestination::Cursor
        };
        self.inventory_ledger_mut(player_runtime)
            .begin_creative_take(id, destination, requested)
    }
}
