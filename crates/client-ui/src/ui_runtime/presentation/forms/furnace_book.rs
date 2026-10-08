//! The furnace family's JSON-UI recipe panel and its controller bindings.

mod cache;
mod projection;
#[cfg(test)]
mod tests;
pub(super) use cache::FurnaceBookCache;
pub(crate) use projection::ProjectionCache;
pub use projection::{Entries, entries, icons, projection};
#[cfg(test)]
pub(in crate::ui_runtime::presentation) use tests::{
    assert_empty_search_clears_grid, assert_selection_publication,
};

use json_ui::{Context, DataSource, HitKind, HitRegion, Scalar};
use protocol::{NetworkItemStack, ScreenRecipe, WindowKind};
use serde_json::Value;

use super::super::{HudFrame, IconRef};
use crate::ui_runtime::{
    UiRuntime,
    presentation::{inventory_pointer::InventoryCellHit, screens::Widget},
};

pub fn active(player: &player_state::PlayerState) -> bool {
    matches!(
        player.inventory.ledger().window_kind(),
        Some(WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker)
    )
}

pub fn output_stack(recipe: &ScreenRecipe) -> NetworkItemStack {
    let Some(output) = recipe.output else {
        return NetworkItemStack::empty();
    };
    NetworkItemStack {
        network_id: output.network_id,
        metadata: u32::from(output.aux),
        count: u16::from(output.count),
        block_runtime_id: output.block_runtime_id as i32,
        ..NetworkItemStack::empty()
    }
}

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

pub(super) fn context(mut context: Context) -> Context {
    for (name, index) in [
        ("food_index", 1),
        ("items_index", 2),
        ("blocks_index", 3),
        ("search_index", 4),
        ("survival_layout_index", 1),
        ("recipe_book_layout_index", 2),
    ] {
        context = context.with_var(name, Value::from(index));
    }
    context
}

pub(super) fn data(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    data: &mut DataSource,
    icons: &mut Vec<IconRef>,
    cache: &mut Option<FurnaceBookCache>,
) {
    let state = runtime.screen_state();
    let shown = state.furnace_book_open;
    let tab = state.furnace_tab.unwrap_or(4);
    let publication = cache::publication(player, runtime, frame, icons.len(), cache);
    let [food, items, blocks] = publication.categories;
    for (name, value) in [
        ("#is_survival_layout", !shown),
        ("#is_recipe_book_layout", shown),
        ("#is_left_tab_inventory", !shown),
        ("#is_left_tab_food", tab == 1),
        ("#is_left_tab_items", tab == 2),
        ("#is_left_tab_blocks", tab == 3),
        ("#is_left_tab_search", tab == 4),
        ("#food_tab_visible", food),
        ("#items_tab_is_leftmost", items && !food),
        ("#items_tab_visible_not_leftmost", items && food),
        ("#blocks_tab_is_leftmost", blocks && !food && !items),
        (
            "#blocks_tab_visible_not_leftmost",
            blocks && (food || items),
        ),
        ("#filtering_enabled", runtime.recipe_filtering(player)),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    for (name, missing) in [
        ("#food_tab_offset", 0),
        ("#items_tab_offset", i32::from(!food)),
        ("#blocks_tab_offset", i32::from(!food) + i32::from(!items)),
    ] {
        data.set_global(name, Scalar::Json(serde_json::json!([-25 * missing, 0])));
    }
    data.select_radio("layout_toggle", if shown { 2 } else { 1 });
    data.select_radio("navigation_tab", usize::from(tab));
    let label = match tab {
        1 => "furnaceScreen.tab.food",
        2 => "furnaceScreen.tab.items",
        3 => "furnaceScreen.tab.blocks",
        _ => "craftingScreen.tab.allRecipes",
    };
    data.set_global(
        "#tab_label_text",
        Scalar::Text(
            runtime
                .translation(label)
                .map_or_else(|| label.to_owned(), |text| text.to_string()),
        ),
    );
    data.set_global("#text_box_item_name", Scalar::Text(state.search.clone()));
    if !shown {
        return;
    }
    data.set_collection_defaults(
        "recipe_book",
        [("#recipe_book_total_items".into(), Scalar::Num(0.0))].into(),
    );
    icons.extend_from_slice(&publication.icons);
    data.set_shared_collection("recipe_book", std::sync::Arc::clone(&publication.rows));
}

pub(super) fn hit(screen: &str, region: &HitRegion) -> Option<InventoryCellHit> {
    if !matches!(
        screen,
        "furnace.furnace_screen" | "blast_furnace.blast_furnace_screen" | "smoker.smoker_screen"
    ) {
        return None;
    }
    if region.collection.as_deref() == Some("recipe_book") {
        return Some(InventoryCellHit::RecipeBook(
            u16::try_from(region.collection_index?).ok()?,
        ));
    }
    if region.kind == HitKind::EditBox {
        return Some(InventoryCellHit::CreativeSearch);
    }
    let widget = match region.control_name.as_deref()? {
        "toggle.enable_filtering" => Widget::RecipeFilter,
        "layout_toggle" => Widget::InventoryLayout(u8::try_from(region.group_index?).ok()?),
        "navigation_tab" => Widget::FurnaceTab(u8::try_from(region.group_index?).ok()?),
        "button.clear_selected_recipe" => Widget::FurnaceClearRecipe,
        _ => return None,
    };
    Some(InventoryCellHit::Widget(widget))
}
