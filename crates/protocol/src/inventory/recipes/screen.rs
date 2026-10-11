//! Recipes the non-crafting-table screens use, and the multi-recipe ids the
//! anvil names in its requests.

use std::sync::Arc;
use valentine::bedrock::codec::BedrockCodec;

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

/// Exact identifier of the item-repair multi recipe.
const REPAIR_MULTI_UUID: uuid::Uuid = uuid::Uuid::from_u128(1);

impl MultiRecipe {
    /// Decode the wire's little-endian UUID halves and match the exact repair identifier.
    #[must_use]
    pub fn is_repair(&self) -> bool {
        uuid::Uuid::decode(&mut &self.uuid[..], ()).is_ok_and(|uuid| uuid == REPAIR_MULTI_UUID)
    }
}

/// Everything one CraftingData update advertises for the screens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenRecipes {
    pub recipes: Vec<ScreenRecipe>,
    pub multi: Vec<MultiRecipe>,
    /// Station recipes skipped because their inputs or output cannot be represented.
    pub skipped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_wire_uuid_is_repair() {
        for position in 0..16 {
            let mut uuid = [0; 16];
            uuid[position] = 1;
            assert_eq!(MultiRecipe { uuid, id: 1 }.is_repair(), position == 8);
        }
        assert!(
            !MultiRecipe {
                uuid: [0; 16],
                id: 1
            }
            .is_repair()
        );
    }
}
