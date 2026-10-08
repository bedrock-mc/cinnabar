//! Furnace recipe outputs and the inventory ingredients that feed them.

use protocol::{ScreenRecipe, ScreenRecipeKind, WindowKind};

use crate::inventory_ledger::{InventoryGestureError, InventoryTarget, PlayerInventoryLedger};
use crate::{InventorySession, screen_ingredient_accepts};

#[cfg(test)]
mod tests;

mod cache;
pub(crate) use cache::Cache;
pub use cache::{FurnaceRecipeIter, FurnaceRecipes};

mod selection;
pub(crate) use selection::Selection;

fn kind(window: WindowKind) -> Option<ScreenRecipeKind> {
    Some(match window {
        WindowKind::Furnace => ScreenRecipeKind::Furnace,
        WindowKind::BlastFurnace => ScreenRecipeKind::BlastFurnace,
        WindowKind::Smoker => ScreenRecipeKind::Smoker,
        _ => return None,
    })
}

impl PlayerInventoryLedger {
    fn furnace_input_matches(
        &self,
        recipe: &ScreenRecipe,
        stack: &protocol::NetworkItemStack,
    ) -> bool {
        let (Some(ingredient), Some(entry)) = (
            recipe.ingredients.first(),
            self.negotiated_item_entry(stack.network_id),
        ) else {
            return false;
        };
        screen_ingredient_accepts(
            ingredient,
            &entry.identifier,
            stack.metadata,
            &entry.item_tags,
        )
    }

    /// Supplied recipes include the ingredient already loaded in the furnace.
    pub fn can_supply_furnace_recipe(&self, recipe: &ScreenRecipe) -> bool {
        self.storage_stack(0)
            .is_some_and(|stack| self.furnace_input_matches(recipe, stack))
            || self.furnace_source_count(recipe) != 0
    }

    fn furnace_source_count(&self, recipe: &ScreenRecipe) -> u32 {
        (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| self.displayed_stack(slot))
            .chain(self.storage_stack(1))
            .filter(|stack| self.furnace_input_matches(recipe, stack))
            .map(|stack| u32::from(stack.count))
            .sum()
    }

    /// The first player inventory stack that matches this recipe's input.
    pub fn furnace_ingredient_slot(&self, recipe: &ScreenRecipe) -> Option<u8> {
        (0..protocol::PLAYER_INVENTORY_SLOTS).find(|slot| {
            let Some(stack) = self.displayed_stack(*slot) else {
                return false;
            };
            self.furnace_input_matches(recipe, stack)
        })
    }

    /// Places a supplied recipe ingredient into the furnace's ingredient role.
    pub fn begin_furnace_recipe(
        &mut self,
        recipe: &ScreenRecipe,
    ) -> Result<i32, InventoryGestureError> {
        if self.window_kind().and_then(kind) != Some(recipe.kind) || recipe.ingredients.len() != 1 {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if self.furnace_result_selected(recipe) {
            return self.begin_clear_furnace_recipe();
        }
        let supplied = self.furnace_source_count(recipe) != 0;
        let mut placed = self.prepare_furnace_selection(recipe, supplied)?;
        if !supplied {
            return Ok(placed.unwrap_or(0));
        }
        let mut first_error = None;
        if self
            .storage_stack(1)
            .is_some_and(|stack| self.furnace_input_matches(recipe, stack))
        {
            match self.begin_move_to_furnace_input(InventoryTarget::Storage(1)) {
                Ok(request) => placed = Some(request),
                Err(error) => {
                    first_error = Some(error);
                }
            }
        }
        for slot in 0..protocol::PLAYER_INVENTORY_SLOTS {
            if !self
                .displayed_stack(slot)
                .is_some_and(|stack| self.furnace_input_matches(recipe, stack))
            {
                continue;
            }
            match self.begin_move_to_furnace_input(InventoryTarget::Player(slot)) {
                Ok(request) => placed = Some(request),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        let outcome = placed
            .or_else(|| {
                self.storage_stack(0)
                    .is_some_and(|stack| self.furnace_input_matches(recipe, stack))
                    .then_some(0)
            })
            .ok_or_else(|| first_error.unwrap_or(InventoryGestureError::EmptyGesture));
        if outcome.is_err() {
            self.clear_furnace_recipe();
        }
        outcome
    }
}

impl InventorySession {
    /// Any accepted alternative can keep the result in the supplied-only list.
    pub fn can_supply_furnace_result(&self, recipe: &ScreenRecipe) -> bool {
        cache::supplied(self, recipe)
    }
    /// Result identities reuse one allocation until catalog or supplied ingredients change.
    pub fn furnace_recipes(&self, filtering: bool) -> FurnaceRecipes<'_> {
        cache::project(self, filtering)
    }
}
