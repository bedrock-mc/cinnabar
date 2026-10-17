//! Recipe lookups for the stonecutter, smithing table and cartography table,
//! answered from the CraftingData catalog and the items in the open screen.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use protocol::{RecipeCatalog, RecipeHandle, ScreenRecipe, ScreenRecipeKind, WindowKind};

use super::inventory_ledger::{InventoryTarget, PlayerInventoryLedger};
use crate::InventorySession;

struct Item {
    identifier: Arc<str>,
    metadata: u32,
    tags: Arc<[Arc<str>]>,
}

/// Resolve one screen cell through the current session registry.
fn item_in(ledger: &PlayerInventoryLedger, slot: u8) -> Option<Item> {
    let stack = ledger.target_stack(InventoryTarget::Craft(slot))?;
    let entry = ledger.negotiated_item_entry(stack.network_id)?;
    Some(Item {
        identifier: Arc::clone(&entry.identifier),
        metadata: stack.metadata,
        tags: Arc::clone(&entry.item_tags),
    })
}

/// Match a screen ingredient against the already-resolved item.
fn accepts(recipe: &ScreenRecipe, index: usize, item: &Item) -> bool {
    recipe.ingredients.get(index).is_some_and(|ingredient| {
        crate::screen_ingredient_accepts(ingredient, &item.identifier, item.metadata, &item.tags)
    })
}

impl InventorySession {
    /// The committed recipe catalog while it is available.
    pub fn screen_catalog(&self) -> Option<&RecipeCatalog> {
        self.crafting_authority.catalog()
    }

    /// Stonecutter recipes the input item feeds, in catalog order.
    pub fn stonecutter_options(&self) -> Vec<&ScreenRecipe> {
        let (Some(catalog), Some(input)) = (self.screen_catalog(), item_in(self.ledger(), 3))
        else {
            return Vec::new();
        };
        catalog
            .screen_recipes(ScreenRecipeKind::Stonecutter)
            .filter(|recipe| accepts(recipe, 0, &input))
            .collect()
    }

    /// The recipe the open stonecutter, smithing or cartography screen applies now.
    pub fn active_screen_recipe(&self, choice: Option<u32>) -> Option<&ScreenRecipe> {
        let ledger = self.ledger();
        let catalog = self.screen_catalog()?;
        match ledger.window_kind()? {
            WindowKind::Stonecutter => {
                let choice = choice?;
                self.stonecutter_options()
                    .into_iter()
                    .find(|recipe| recipe.id == choice)
            }
            WindowKind::Smithing => {
                let (template, base, addition) = (
                    item_in(ledger, 53)?,
                    item_in(ledger, 51)?,
                    item_in(ledger, 52)?,
                );
                catalog
                    .screen_recipes(ScreenRecipeKind::SmithingTransform)
                    .find(|recipe| {
                        accepts(recipe, 0, &template)
                            && accepts(recipe, 1, &base)
                            && accepts(recipe, 2, &addition)
                    })
            }
            WindowKind::Cartography => {
                let (map, extra) = (item_in(ledger, 12)?, item_in(ledger, 13)?);
                catalog
                    .screen_recipes(ScreenRecipeKind::Cartography)
                    .find(|recipe| {
                        (accepts(recipe, 0, &map) && accepts(recipe, 1, &extra))
                            || (accepts(recipe, 0, &extra) && accepts(recipe, 1, &map))
                    })
            }
            _ => None,
        }
    }

    /// The result the open screen previews before it is taken: the chosen
    /// recipe's on the stonecutter, smithing and cartography tables, the
    /// grid's recipe on a crafter.
    pub fn predicted_screen_output(&self, choice: Option<u32>) -> Option<protocol::RecipeOutput> {
        match self.ledger().window_kind()? {
            WindowKind::Stonecutter | WindowKind::Smithing | WindowKind::Cartography => {
                self.active_screen_recipe(choice)?.output
            }
            WindowKind::Crafter => {
                let cells = self.ledger().crafter_grid_cells()?;
                if cells.iter().all(Option::is_none) {
                    return None;
                }
                let items: Vec<_> = cells
                    .iter()
                    .map(|cell| {
                        cell.as_ref()
                            .map(super::inventory_ledger::CraftGridCell::item)
                    })
                    .collect();
                match crate::match_crafting_grid(self.screen_catalog()?, 3, &items) {
                    crate::CraftGridMatch::Unique(recipe) => Some(recipe.output()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether a current book recipe is discovered and its ingredients can be supplied now.
    pub fn book_recipe_craftable(&self, recipe: &RecipeHandle) -> bool {
        self.crafting_authority.contains_recipe(recipe)
            && self.crafting_authority.book_recipe_unlocked(recipe)
            && self.ledger().can_auto_craft(recipe)
    }

    /// Rechecks recipe discovery at the action frontier before allocating any request.
    pub fn begin_book_craft(
        &mut self,
        recipe: &RecipeHandle,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        if !self.crafting_authority.contains_recipe(recipe)
            || !self.crafting_authority.book_recipe_unlocked(recipe)
        {
            return Err(super::inventory_ledger::InventoryGestureError::InvalidRequest);
        }
        self.ledger_mut().begin_auto_craft(recipe)
    }

    /// Lists one variant per output for the open grid, with pagination after grouping.
    /// Discovery takes precedence over ingredients and priority. Filtering keeps held
    /// ingredients, with craftable results first. Nonempty search may show locked
    /// results disabled when limited crafting is off.
    pub fn book_recipes(
        &self,
        filtering: bool,
        search_active: bool,
        skip: usize,
        take: usize,
    ) -> Vec<RecipeHandle> {
        let (Some(catalog), Some(rules)) = (self.screen_catalog(), self.crafting_rules()) else {
            return Vec::new();
        };
        let ledger = self.ledger();
        let small = ledger.window_kind() != Some(WindowKind::Workbench);
        let mut listed: Vec<(bool, bool, bool, RecipeHandle)> = Vec::new();
        let mut outputs = std::collections::HashMap::new();
        for recipe in catalog.crafting_recipes().filter(|recipe| {
            let (width, height) = recipe.dimensions();
            !small
                || if recipe.is_shapeless() {
                    recipe.ingredient_views().len() <= 4
                } else {
                    width <= 2 && height <= 2
                }
        }) {
            let unlocked = self.crafting_authority.book_recipe_unlocked(recipe);
            let craftable = unlocked && ledger.can_auto_craft(recipe);
            let held = craftable || ledger.holds_any_ingredient(recipe);
            if filtering && !held {
                continue;
            }
            let output = recipe.output();
            let key = (output.network_id, output.aux, output.block_runtime_id);
            if let Some(&index) = outputs.get(&key) {
                let previous: &(bool, bool, bool, RecipeHandle) = &listed[index];
                let rank = |unlocked: bool, craftable: bool, held: bool, recipe: &RecipeHandle| {
                    (!unlocked, !craftable, !held, recipe.recipe().priority())
                };
                if rank(unlocked, craftable, held, recipe)
                    < rank(previous.0, previous.1, previous.2, &previous.3)
                {
                    listed[index] = (unlocked, craftable, held, recipe.clone());
                }
            } else {
                outputs.insert(key, listed.len());
                listed.push((unlocked, craftable, held, recipe.clone()));
            }
        }
        if filtering {
            listed.sort_by_key(|(_, craftable, _, _)| !craftable);
        }
        listed
            .into_iter()
            .filter(|(unlocked, _, _, _)| {
                *unlocked || (search_active && !rules.limited_crafting_enabled())
            })
            .map(|(_, _, _, recipe)| recipe)
            .skip(skip)
            .take(take)
            .collect()
    }
}
