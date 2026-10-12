//! The furnace recipe listing the client's input and the furnace panel share: each recipe's tab
//! and the tab and search projection.

mod projection;
#[cfg(test)]
mod tests;
pub(crate) use projection::ProjectionCache;
pub use projection::{entries, icons, projection};
#[cfg(test)]
pub(in crate::ui_runtime::presentation) use tests::{
    assert_empty_search_clears_grid, assert_selection_publication,
};

use protocol::ScreenRecipe;

use crate::ui_runtime::UiRuntime;

/// The furnace tab filing `recipe`: `1` food, `2` items, `3` blocks.
pub(super) fn category(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    recipe: &ScreenRecipe,
) -> u8 {
    let output = recipe.output.unwrap();
    let Some(entry) = player
        .inventory
        .ledger()
        .negotiated_item_entry(output.network_id)
    else {
        return 2;
    };
    let components = runtime.item_components(&entry.identifier);
    if protocol::vanilla_tag_contains("minecraft:is_food", &entry.identifier) == Some(true)
        || components.is_some_and(|item| item.food)
    {
        1
    } else if output.block_runtime_id != 0
        || components.is_some_and(|item| item.block_placer.is_some())
    {
        3
    } else {
        2
    }
}
