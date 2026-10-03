use std::sync::Arc;

use crate::manual_craft::{ManualCraftError, binding_entry, validate_grid};
use protocol::{RecipeCatalog, RecipeRegistrySnapshot, VerifiedNetworkItemStack};

#[derive(Debug, Clone, Copy)]
pub enum ManualCraftCell<'a> {
    Unknown,
    Empty,
    Present(&'a VerifiedNetworkItemStack),
}

/// Display values only: no recipe ID, stack ID, prepared plan or send authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualCraftPreview {
    pub identifier: Arc<str>,
    pub count: u16,
    pub metadata: u32,
    pub block_runtime_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualCraftMatch {
    Unavailable,
    NoMatch,
    Ambiguous,
    Unique(ManualCraftPreview),
}

/// Pure, bounded matching of the personal top-left, stride-two grid. Unknown
/// cells are unavailable; callers must retire stale cell authority to Unknown.
///
/// At most 8,192 catalog entries are visited; registry lookups are indexed.
/// Consumers should cache display output on catalog, registry and grid revision
/// changes, not scan every frame. Such a cache is never request authority.
pub fn match_manual_grid(
    catalog: &RecipeCatalog,
    session: u64,
    registry: &RecipeRegistrySnapshot,
    grid: &[ManualCraftCell<'_>; 4],
) -> ManualCraftMatch {
    if session == 0 || catalog.session() != session || !catalog.is_available() {
        return ManualCraftMatch::Unavailable;
    }
    if grid
        .iter()
        .any(|cell| matches!(cell, ManualCraftCell::Unknown))
    {
        return ManualCraftMatch::Unavailable;
    }
    let grid = grid.map(|cell| match cell {
        ManualCraftCell::Present(stack) => Some(stack),
        ManualCraftCell::Unknown | ManualCraftCell::Empty => None,
    });
    let mut unique = None;
    for handle in catalog.crafting_recipes() {
        let recipe = handle.recipe();
        let Ok((output, _)) = validate_grid(recipe, &grid, |id| {
            binding_entry(registry.get(id).ok_or(ManualCraftError::Unsupported)?)
        }) else {
            continue;
        };
        if unique.is_some() {
            return ManualCraftMatch::Ambiguous;
        }
        unique = Some((output, recipe.output()));
    }
    unique.map_or(ManualCraftMatch::NoMatch, |(binding, output)| {
        ManualCraftMatch::Unique(ManualCraftPreview {
            identifier: Arc::clone(&binding.identifier),
            count: u16::from(output.count),
            metadata: u32::from(output.aux),
            block_runtime_id: output.block_runtime_id,
        })
    })
}
