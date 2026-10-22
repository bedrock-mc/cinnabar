//! Settled ingredient sources shared by every candidate in one recipe-book projection.

use protocol::{ItemRegistryEntry, NetworkItemStack, RecipeHandle, RecipeIngredientView};

use super::{PLAYER_INVENTORY_SLOT_COUNT, allocation};

/// Borrowed source identities and bindings; byte validation happens once per inventory slot.
pub(crate) struct AutoCraftInputs<'a> {
    pub(super) stacks:
        [Option<(&'a NetworkItemStack, &'a ItemRegistryEntry)>; PLAYER_INVENTORY_SLOT_COUNT],
}

impl AutoCraftInputs<'_> {
    /// Whether these admitted sources can supply one craft of the candidate.
    pub(crate) fn can_craft(&self, recipe: &RecipeHandle) -> bool {
        self.sources(recipe, 1).is_some()
    }

    /// Player cells to draw `ingredient` from: `(cell, amount, stack id)`.
    fn ingredient_sources(
        &self,
        ingredient: &RecipeIngredientView,
        crafts: u8,
        taken: &[(u8, u16)],
    ) -> Option<Vec<(u8, u16, i32)>> {
        let mut needed = u16::from(ingredient.count) * u16::from(crafts);
        let mut sources = Vec::new();
        for slot in 0..PLAYER_INVENTORY_SLOT_COUNT as u8 {
            if needed == 0 {
                break;
            }
            let Some((stack, entry)) = self.stacks[usize::from(slot)] else {
                continue;
            };
            let already: u16 = taken
                .iter()
                .filter(|(cell, _)| *cell == slot)
                .map(|(_, amount)| *amount)
                .sum();
            if !crate::ingredient_accepts(
                ingredient,
                &entry.identifier,
                stack.metadata,
                &entry.item_tags,
            ) {
                continue;
            }
            let amount = needed.min(stack.count.saturating_sub(already));
            if amount > 0 {
                needed -= amount;
                sources.push((slot, amount, stack.stack_network_id));
            }
        }
        (needed == 0).then_some(sources)
    }

    /// Shares deterministic ingredient allocation between craftability and request construction.
    pub(crate) fn sources(&self, recipe: &RecipeHandle, crafts: u8) -> Option<Vec<(u8, u16, i32)>> {
        let mut taken: Vec<(u8, u16)> = Vec::new();
        let mut all = Vec::new();
        let ingredients = recipe.ingredient_views();
        for ingredient in ingredients.iter().flatten() {
            let Some(sources) = self.ingredient_sources(ingredient, crafts, &taken) else {
                return self.reassigned_sources(&ingredients, crafts);
            };
            taken.extend(sources.iter().map(|(cell, amount, _)| (*cell, *amount)));
            all.extend(sources);
        }
        Some(all)
    }

    /// Reassigns quantities only when the ordinary first-choice plan cannot satisfy every input.
    fn reassigned_sources(
        &self,
        views: &[Option<RecipeIngredientView>],
        crafts: u8,
    ) -> Option<Vec<(u8, u16, i32)>> {
        let ingredients: Vec<_> = views.iter().flatten().collect();
        let demands: Vec<_> = ingredients
            .iter()
            .map(|ingredient| u16::from(ingredient.count) * u16::from(crafts))
            .collect();
        let mut counts = [0; PLAYER_INVENTORY_SLOT_COUNT];
        let mut identities = [0; PLAYER_INVENTORY_SLOT_COUNT];
        let mut accepts = [[false; PLAYER_INVENTORY_SLOT_COUNT]; protocol::MAX_RECIPE_INGREDIENTS];
        for slot in 0..PLAYER_INVENTORY_SLOT_COUNT {
            let Some((stack, entry)) = self.stacks[slot] else {
                continue;
            };
            counts[slot] = stack.count;
            identities[slot] = stack.stack_network_id;
            for (index, ingredient) in ingredients.iter().enumerate() {
                accepts[index][slot] = crate::ingredient_accepts(
                    ingredient,
                    &entry.identifier,
                    stack.metadata,
                    &entry.item_tags,
                );
            }
        }
        let assigned = allocation::plan(&demands, &counts, &accepts)?;
        let mut sources = Vec::new();
        for row in assigned.iter().take(ingredients.len()) {
            for (slot, amount) in row
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, amount)| *amount > 0)
            {
                sources.push((slot as u8, amount, identities[slot]));
            }
        }
        Some(sources)
    }
}
