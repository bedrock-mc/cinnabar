//! Fixed structural observations; these values convey no request authority.
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{catalog::RecipeCatalog, model::Recipe};
use crate::RecipeRegistrySnapshot;

pub const MAX_RECIPE_OBSERVATIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IngredientObservation {
    #[serde(rename = "n")]
    pub name_sha256: [u8; 32],
    #[serde(rename = "a")]
    pub aux: u16,
    #[serde(rename = "c")]
    pub count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecipeObservation {
    #[serde(rename = "r")]
    pub recipe_id: u32,
    #[serde(rename = "d")]
    pub dimensions: [u8; 2],
    #[serde(rename = "i")]
    pub ingredients: [Option<IngredientObservation>; 4],
    #[serde(rename = "o")]
    pub output_id: i32,
    #[serde(rename = "n")]
    pub output_name_sha256: Option<[u8; 32]>,
    #[serde(rename = "a")]
    pub output_aux: u16,
    #[serde(rename = "c")]
    pub output_count: u8,
    #[serde(rename = "b")]
    pub output_block: u32,
    #[serde(rename = "m")]
    pub output_capacity: Option<u16>,
    #[serde(rename = "s")]
    pub output_binding_supported: bool,
    #[serde(rename = "f")]
    pub output_fits_capacity: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecipeObservations {
    #[serde(rename = "a")]
    pub available: bool,
    #[serde(rename = "r")]
    pub revision: u64,
    #[serde(rename = "n")]
    pub supported_count: u16,
    #[serde(rename = "t")]
    pub truncated: bool,
    #[serde(rename = "e")]
    pub entries: [Option<RecipeObservation>; MAX_RECIPE_OBSERVATIONS],
}

fn summary(id: u32, recipe: &Recipe, registry: &RecipeRegistrySnapshot) -> RecipeObservation {
    let output = registry.get(recipe.output.id);
    let capacity = output
        .and_then(|entry| entry.negotiated_max_stack_size)
        .map(u16::from);
    RecipeObservation {
        recipe_id: id,
        dimensions: [recipe.width, recipe.height],
        // Observed recipes fit two-by-two, so only the first four cells exist.
        ingredients: std::array::from_fn(|index| {
            recipe.ingredients[index]
                .as_ref()
                .map(|ingredient| IngredientObservation {
                    name_sha256: Sha256::digest(ingredient.name.as_bytes()).into(),
                    aux: ingredient.aux,
                    count: ingredient.count,
                })
        }),
        output_id: recipe.output.id,
        output_name_sha256: output.map(|entry| Sha256::digest(entry.identifier.as_bytes()).into()),
        output_aux: recipe.output.aux,
        output_count: recipe.output.count,
        output_block: recipe.output.block,
        output_capacity: capacity,
        output_binding_supported: output.is_some_and(super::recipe_binding_supported),
        output_fits_capacity: capacity.map(|limit| u16::from(recipe.output.count) <= limit),
    }
}

impl RecipeCatalog {
    /// Caller must reserve its observation output/work budget before calling.
    /// Borrows credited owners; never returns text, handles or a prepared plan.
    pub fn observations(&self, registry: &RecipeRegistrySnapshot) -> RecipeObservations {
        let mut result = RecipeObservations {
            available: self.is_available(),
            revision: self.revision(),
            supported_count: 0,
            truncated: false,
            entries: [None; MAX_RECIPE_OBSERVATIONS],
        };
        if !result.available {
            return result;
        }
        for (id, recipe) in self.observation_entries() {
            let index = usize::from(result.supported_count);
            result.supported_count = result.supported_count.saturating_add(1);
            if let Some(target) = result.entries.get_mut(index) {
                *target = Some(summary(id, recipe, registry));
            } else {
                result.truncated = true;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests;
