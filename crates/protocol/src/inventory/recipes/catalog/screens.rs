//! Credits for the new screen vectors; decoded names stay owned by retained batches.

use std::mem::size_of;

use super::super::{
    model::Record,
    screen::{
        BrewingRecipe, MAX_SCREEN_RECIPES, MultiRecipe, ScreenIngredient, ScreenRecipe,
        ScreenRecipes,
    },
};

/// Whether a record needs a decoded owner after its update leaves the queue.
pub(super) fn contains(screen: &ScreenRecipes, id: u32) -> bool {
    screen
        .recipes
        .binary_search_by_key(&id, |recipe| recipe.id)
        .is_ok()
        || screen
            .multi
            .binary_search_by_key(&id, |recipe| recipe.id)
            .is_ok()
}

/// Counts the exact new vector elements before their owning catalog reserves credits.
pub(super) fn merged_charge(
    old: &ScreenRecipes,
    incoming: &ScreenRecipes,
    records: &[Record],
) -> Option<usize> {
    let recipes = merged_views(&old.recipes, &incoming.recipes, records, |recipe| recipe.id)
        .take(MAX_SCREEN_RECIPES)
        .try_fold(0usize, |bytes, recipe| {
            bytes.checked_add(size_of::<ScreenRecipe>())?.checked_add(
                recipe
                    .ingredients
                    .len()
                    .checked_mul(size_of::<ScreenIngredient>())?,
            )
        })?;
    let multi = merged_views(&old.multi, &incoming.multi, records, |recipe| recipe.id).count();
    let brewing = brewing(old, incoming).len();
    recipes
        .checked_add(multi.checked_mul(size_of::<MultiRecipe>())?)?
        .checked_add(brewing.checked_mul(size_of::<BrewingRecipe>())?)
}

/// Copies structural vectors only after the caller holds their complete reservation.
pub(super) fn merge(
    old: &ScreenRecipes,
    incoming: &ScreenRecipes,
    records: &[Record],
) -> Option<ScreenRecipes> {
    let count = merged_views(&old.recipes, &incoming.recipes, records, |recipe| recipe.id)
        .take(MAX_SCREEN_RECIPES)
        .count();
    let mut recipes = Vec::new();
    recipes.try_reserve_exact(count).ok()?;
    for recipe in merged_views(&old.recipes, &incoming.recipes, records, |recipe| recipe.id)
        .take(MAX_SCREEN_RECIPES)
    {
        let mut ingredients = Vec::new();
        ingredients
            .try_reserve_exact(recipe.ingredients.len())
            .ok()?;
        ingredients.extend_from_slice(&recipe.ingredients);
        recipes.push(ScreenRecipe {
            id: recipe.id,
            kind: recipe.kind,
            ingredients,
            output: recipe.output,
        });
    }
    let count = merged_views(&old.multi, &incoming.multi, records, |recipe| recipe.id).count();
    let mut multi = Vec::new();
    multi.try_reserve_exact(count).ok()?;
    multi.extend(merged_views(&old.multi, &incoming.multi, records, |recipe| recipe.id).copied());
    let mut brewing_recipes = Vec::new();
    brewing_recipes
        .try_reserve_exact(brewing(old, incoming).len())
        .ok()?;
    brewing_recipes.extend_from_slice(brewing(old, incoming));
    Some(ScreenRecipes {
        recipes,
        multi,
        brewing: brewing_recipes,
        skipped: 0,
    })
}

/// Brewing recipes have no IDs to merge by: an update that lists any replaces them all.
fn brewing<'a>(old: &'a ScreenRecipes, incoming: &'a ScreenRecipes) -> &'a [BrewingRecipe] {
    if incoming.brewing.is_empty() {
        &old.brewing
    } else {
        &incoming.brewing
    }
}

/// Merges sorted borrowed views while every advertised ID, including a tombstone, replaces old data.
pub(super) fn merged_views<'a, T>(
    old: &'a [T],
    incoming: &'a [T],
    records: &'a [Record],
    id: impl Fn(&T) -> u32 + 'a,
) -> impl Iterator<Item = &'a T> {
    let (mut previous, mut next, mut record) = (0, 0, 0);
    std::iter::from_fn(move || {
        loop {
            if next < incoming.len()
                && (previous == old.len() || id(&incoming[next]) <= id(&old[previous]))
            {
                if previous < old.len() && id(&incoming[next]) == id(&old[previous]) {
                    previous += 1;
                }
                let value = &incoming[next];
                next += 1;
                return Some(value);
            }
            let value = old.get(previous)?;
            previous += 1;
            while record < records.len() && records[record].id < id(value) {
                record += 1;
            }
            if record == records.len() || records[record].id != id(value) {
                return Some(value);
            }
        }
    })
}
