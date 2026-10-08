//! Recipes the non-crafting-table screens use, and the multi-recipe ids the
//! anvil names in its requests.

use std::sync::Arc;

use super::crafting::RecipeOutput;

/// Retained screen recipes; extras are dropped.
pub(super) const MAX_SCREEN_RECIPES: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenRecipeKind {
    Stonecutter,
    Cartography,
    SmithingTransform,
    SmithingTrim,
    Furnace,
    BlastFurnace,
    Smoker,
}

/// One accepted input of a screen recipe: an item name or a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenIngredient {
    pub name: Arc<str>,
    pub tag: bool,
    pub aux: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRecipe {
    pub id: u32,
    pub kind: ScreenRecipeKind,
    /// Furnaces and stonecutter: the input. Cartography: map then modifier. Smithing:
    /// template, base, addition.
    pub ingredients: Vec<ScreenIngredient>,
    /// Absent for smithing trims, which decorate the base item.
    pub output: Option<RecipeOutput>,
}

/// A special recipe (item repair, banner copy, ...) identified by its UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiRecipe {
    pub uuid: [u8; 16],
    pub id: u32,
}

impl MultiRecipe {
    /// Whether this is the item-repair recipe (UUID ...0001); the wire's byte
    /// order is not relied on, only that a single byte holds the value one.
    #[must_use]
    pub fn is_repair(&self) -> bool {
        self.uuid.iter().filter(|byte| **byte != 0).count() == 1 && self.uuid.contains(&1)
    }
}

/// Everything one CraftingData update advertises for the screens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenRecipes {
    pub recipes: Vec<ScreenRecipe>,
    pub multi: Vec<MultiRecipe>,
}
