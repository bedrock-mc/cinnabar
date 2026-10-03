//! Crafting-table recipe identification for a player's 2x2 or 3x3 grid.
//!
//! Shaped recipes match their exact orientation inside the occupied bounding
//! box (or its mirror when the recipe allows); shapeless recipes match a
//! one-to-one cell assignment.

use protocol::{
    MAX_RECIPE_INGREDIENTS as MAX_INGREDIENTS, RecipeCatalog, RecipeDefinition as Recipe,
    RecipeHandle, RecipeIngredient as Ingredient, vanilla_tag_contains,
};

/// One occupied grid cell as the matcher sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftGridItem<'a> {
    pub identifier: &'a str,
    pub metadata: u32,
    pub count: u16,
    /// No NBT, place or break data; recipe inputs never carry any.
    pub plain: bool,
    /// Tags the session item registry declares for this item.
    pub tags: &'a [std::sync::Arc<str>],
}

#[derive(Debug, Clone)]
pub enum CraftGridMatch {
    Unavailable,
    NoMatch,
    Ambiguous,
    Unique(RecipeHandle),
}

/// Whether a decoded ingredient view accepts an item and its declared tags.
pub fn ingredient_accepts(
    ingredient: &protocol::RecipeIngredientView,
    identifier: &str,
    metadata: u32,
    tags: &[std::sync::Arc<str>],
) -> bool {
    accepts_name(
        &ingredient.name,
        ingredient.tag,
        ingredient.aux,
        identifier,
        metadata,
        tags,
    )
}

/// Whether a decoded screen ingredient accepts an item and its declared tags.
pub fn screen_ingredient_accepts(
    ingredient: &protocol::ScreenIngredient,
    identifier: &str,
    metadata: u32,
    tags: &[std::sync::Arc<str>],
) -> bool {
    accepts_name(
        &ingredient.name,
        ingredient.tag,
        ingredient.aux,
        identifier,
        metadata,
        tags,
    )
}

/// Apply the existing tag-or-name matching rule to one decoded descriptor.
fn accepts_name(
    name: &str,
    tag: bool,
    aux: u16,
    identifier: &str,
    metadata: u32,
    tags: &[std::sync::Arc<str>],
) -> bool {
    if tag {
        tags.iter().any(|tag| &**tag == name)
            || vanilla_tag_contains(name, identifier) == Some(true)
    } else {
        name == identifier && (aux == protocol::RECIPE_ANY_AUX || u32::from(aux) == metadata)
    }
}

/// Whether one item satisfies the indexed nonempty ingredient for this many crafts.
#[must_use]
pub fn recipe_ingredient_accepts(
    recipe: &RecipeHandle,
    index: usize,
    item: &CraftGridItem<'_>,
    crafts: u8,
) -> bool {
    recipe
        .recipe()
        .ingredients()
        .iter()
        .flatten()
        .nth(index)
        .is_some_and(|ingredient| accepts(ingredient, item, u16::from(crafts)))
}

/// Apply one ingredient's identity, metadata and count requirements.
fn accepts(ingredient: &Ingredient, item: &CraftGridItem<'_>, crafts: u16) -> bool {
    let kind = if ingredient.is_tag() {
        // An unknown tag with no declaring item fails closed.
        item.tags.iter().any(|tag| **tag == *ingredient.name())
            || vanilla_tag_contains(ingredient.name(), item.identifier) == Some(true)
    } else {
        ingredient.name() == item.identifier && ingredient.accepts_metadata(item.metadata)
    };
    kind && item.plain
        && u16::from(ingredient.count())
            .checked_mul(crafts)
            .is_some_and(|needed| item.count >= needed)
}

/// Identifies the unique recipe a `width`-by-`width` grid forms.
#[must_use]
pub fn match_crafting_grid(
    catalog: &RecipeCatalog,
    width: u8,
    grid: &[Option<CraftGridItem<'_>>],
) -> CraftGridMatch {
    if !catalog.is_available()
        || !matches!(width, 2 | 3)
        || grid.len() != usize::from(width) * usize::from(width)
    {
        return CraftGridMatch::Unavailable;
    }
    // The lowest priority wins; recipes tied at it must agree on output.
    let mut best: Option<(i32, RecipeHandle)> = None;
    let mut ambiguous = false;
    for handle in catalog.crafting_recipes() {
        let recipe = handle.recipe();
        let matched = if recipe.is_shapeless() {
            shapeless_matches(recipe, grid)
        } else {
            shaped_matches(recipe, width, grid, false)
                || (recipe.allows_mirror() && shaped_matches(recipe, width, grid, true))
        };
        if !matched {
            continue;
        }
        match &best {
            Some((priority, _)) if recipe.priority() > *priority => {}
            Some((priority, chosen)) if recipe.priority() == *priority => {
                ambiguous |= chosen.recipe().output() != recipe.output();
            }
            _ => {
                best = Some((recipe.priority(), handle.clone()));
                ambiguous = false;
            }
        }
    }
    match best {
        None => CraftGridMatch::NoMatch,
        Some(_) if ambiguous => CraftGridMatch::Ambiguous,
        Some((_, handle)) => CraftGridMatch::Unique(handle),
    }
}

/// Match a shaped recipe against the grid's occupied bounds.
fn shaped_matches(
    recipe: &Recipe,
    width: u8,
    grid: &[Option<CraftGridItem<'_>>],
    mirrored: bool,
) -> bool {
    let width = usize::from(width);
    let occupied = || {
        grid.iter()
            .enumerate()
            .filter(|(_, cell)| cell.is_some())
            .map(|(index, _)| (index / width, index % width))
    };
    let (Some(top), Some(left)) = (
        occupied().map(|(row, _)| row).min(),
        occupied().map(|(_, column)| column).min(),
    ) else {
        return false;
    };
    let bottom = occupied().map(|(row, _)| row).max().unwrap_or(top);
    let right = occupied().map(|(_, column)| column).max().unwrap_or(left);
    let (rows, columns) = (
        usize::from(recipe.dimensions().1),
        usize::from(recipe.dimensions().0),
    );
    // Leading or trailing empty recipe rows/columns cannot be represented by
    // a bounding box, so the recipe's own extent must match it exactly.
    if bottom - top + 1 != rows || right - left + 1 != columns {
        return false;
    }
    (0..rows).all(|row| {
        (0..columns).all(|column| {
            let source = if mirrored {
                columns - 1 - column
            } else {
                column
            };
            let expected = recipe.ingredients()[row * columns + source].as_ref();
            let cell = grid[(top + row) * width + left + column].as_ref();
            match (expected, cell) {
                (None, None) => true,
                (Some(ingredient), Some(item)) => accepts(ingredient, item, 1),
                _ => false,
            }
        })
    })
}

/// Match each shapeless ingredient to a distinct occupied grid cell.
fn shapeless_matches(recipe: &Recipe, grid: &[Option<CraftGridItem<'_>>]) -> bool {
    let ingredients: Vec<&Ingredient> = recipe.ingredients().iter().flatten().collect();
    let items: Vec<&CraftGridItem<'_>> = grid.iter().flatten().collect();
    if ingredients.len() != items.len() || items.len() > MAX_INGREDIENTS {
        return false;
    }
    assign(&ingredients, &items, 0, &mut [false; MAX_INGREDIENTS])
}

/// Backtracking one-to-one assignment; at most nine cells.
fn assign(
    ingredients: &[&Ingredient],
    items: &[&CraftGridItem<'_>],
    next: usize,
    used: &mut [bool; MAX_INGREDIENTS],
) -> bool {
    let Some(ingredient) = ingredients.get(next) else {
        return true;
    };
    for (index, item) in items.iter().enumerate() {
        if used[index] || !accepts(ingredient, item, 1) {
            continue;
        }
        used[index] = true;
        if assign(ingredients, items, next + 1, used) {
            return true;
        }
        used[index] = false;
    }
    false
}

#[cfg(test)]
mod tests;
