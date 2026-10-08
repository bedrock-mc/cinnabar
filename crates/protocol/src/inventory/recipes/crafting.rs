//! Immutable public views of decoded crafting recipes.

use super::catalog::RecipeCatalog;
use super::model::{Ingredient, RecipeHandle};

/// The output a recipe declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeOutput {
    pub network_id: i32,
    pub aux: u16,
    pub count: u8,
    pub block_runtime_id: u32,
    /// The output carried the canonical empty user-data envelope.
    pub empty_envelope: bool,
}

/// One ingredient of a crafting recipe as callers may read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeIngredientView {
    pub name: std::sync::Arc<str>,
    pub tag: bool,
    pub aux: u16,
    pub count: u8,
}

impl RecipeCatalog {
    /// Every retained crafting-table recipe, any shape.
    #[must_use]
    pub fn crafting_handles(&self) -> Vec<RecipeHandle> {
        self.crafting_recipes().cloned().collect()
    }
}

impl RecipeHandle {
    /// The recipe's ingredients: shaped recipes in row-major order with `None`
    /// for empty cells, shapeless ones without gaps.
    #[must_use]
    pub fn ingredient_views(&self) -> Vec<Option<RecipeIngredientView>> {
        let recipe = self.recipe();
        let view = |ingredient: &Ingredient| RecipeIngredientView {
            name: std::sync::Arc::from(ingredient.name.as_str()),
            tag: ingredient.tag,
            aux: ingredient.aux,
            count: ingredient.count,
        };
        if recipe.shapeless {
            recipe
                .ingredients
                .iter()
                .flatten()
                .map(|i| Some(view(i)))
                .collect()
        } else {
            let cells = usize::from(recipe.width) * usize::from(recipe.height);
            recipe
                .ingredients
                .iter()
                .take(cells)
                .map(|slot| slot.as_ref().map(view))
                .collect()
        }
    }

    /// Whether the recipe is a shapeless one.
    #[must_use]
    pub fn is_shapeless(&self) -> bool {
        self.recipe().shapeless
    }

    #[must_use]
    pub fn output(&self) -> RecipeOutput {
        self.recipe().output()
    }

    /// Per-craft ingredient counts in recipe order, empty shape cells skipped.
    pub fn ingredient_counts(&self) -> impl Iterator<Item = u8> + '_ {
        self.recipe()
            .ingredients
            .iter()
            .flatten()
            .map(|ingredient| ingredient.count)
    }
}
