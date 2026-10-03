//! Bounded recipe admission. Limits are client policy, not server maxima.
mod budget;
mod catalog;
mod crafting;
mod decode;
mod grammar;
mod item_tags;
pub(super) mod model;
mod observation;
mod reader;
mod screen;

pub use budget::RECIPE_OWNED_BYTES;
pub use catalog::RecipeCatalog;
pub use crafting::{RecipeIngredientView, RecipeOutput};
pub use item_tags::vanilla_tag_contains;
pub use model::{
    ANY_AUX as RECIPE_ANY_AUX, Ingredient as RecipeIngredient,
    MAX_INGREDIENTS as MAX_RECIPE_INGREDIENTS, Recipe as RecipeDefinition, RecipeHandle,
    RecipeUpdate,
};
pub use observation::{
    IngredientObservation, MAX_RECIPE_OBSERVATIONS, RecipeObservation, RecipeObservations,
};
pub use screen::{MultiRecipe, ScreenIngredient, ScreenRecipe, ScreenRecipeKind, ScreenRecipes};

pub fn decode_recipe_update(body: &[u8]) -> Result<RecipeUpdate, super::InventoryPacketError> {
    decode::decode(body)
}

/// Whether a registry entry has the plain descriptor admitted by recipe wire observations.
pub fn recipe_binding_supported(entry: &crate::ItemRegistryEntry) -> bool {
    (!entry.component_based || entry.canonical_empty_component_data)
        && entry.identifier.len() <= 16384
        && grammar::identifier(&entry.identifier)
}
/// Whether the user-data bytes are the canonical empty recipe envelope.
pub fn empty_recipe_extra(extra: &[u8]) -> bool {
    canonical_empty_extra(extra)
}

fn canonical_empty_extra(extra: &[u8]) -> bool {
    if extra.is_empty() {
        return super::validate_item_user_data(extra).is_ok();
    }
    let mut fields = extra;
    let Some(header) = fields.get(..2) else {
        return false;
    };
    if i16::from_le_bytes([header[0], header[1]]) != 0 {
        return false;
    }
    fields = &fields[2..];
    for _ in 0..2 {
        let Some(count) = fields.get(..4) else {
            return false;
        };
        if i32::from_le_bytes([count[0], count[1], count[2], count[3]]) != 0 {
            return false;
        }
        fields = &fields[4..];
    }
    fields.is_empty() && super::validate_item_user_data(extra).is_ok()
}
