//! The furnace family's JSON-UI recipe panel and its controller bindings.

use json_ui::{CollectionItem, Context, DataSource, HitKind, HitRegion, Scalar};
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

fn category(player: &player_state::PlayerState, runtime: &UiRuntime, recipe: &ScreenRecipe) -> u8 {
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

pub fn entries<'a>(
    player: &'a player_state::PlayerState,
    runtime: &UiRuntime,
) -> Vec<&'a ScreenRecipe> {
    let tab = runtime.screen_state().furnace_tab.unwrap_or(4);
    let query = runtime.screen_state().search.to_lowercase();
    player
        .inventory
        .furnace_recipes(runtime.recipe_filtering(player))
        .into_iter()
        .filter(|recipe| tab == 4 || category(player, runtime, recipe) == tab)
        .filter(|recipe| {
            if tab != 4 || query.is_empty() {
                return true;
            }
            player
                .inventory
                .ledger()
                .negotiated_item_entry(recipe.output.unwrap().network_id)
                .is_some_and(|entry| {
                    runtime
                        .localized_item_name(&entry.identifier)
                        .to_lowercase()
                        .contains(&query)
                })
        })
        .collect()
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
) {
    let state = runtime.screen_state();
    let shown = state.furnace_book_open;
    let tab = state.furnace_tab.unwrap_or(4);
    let all = player.inventory.furnace_recipes(false);
    let food = all
        .iter()
        .any(|recipe| category(player, runtime, recipe) == 1);
    let items = all
        .iter()
        .any(|recipe| category(player, runtime, recipe) == 2);
    let blocks = all
        .iter()
        .any(|recipe| category(player, runtime, recipe) == 3);
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
    let entries = entries(player, runtime);
    let total = entries.len() as f64;
    let rows = entries
        .iter()
        .enumerate()
        .map(|(index, recipe)| {
            let icon = frame
                .window_icons
                .book_entries
                .get(index)
                .copied()
                .flatten();
            let renderer = icon.map_or(Scalar::Json(Value::Null), |icon| {
                icons.push(icon);
                Scalar::Num((icons.len() - 1) as f64)
            });
            let supplied = player.inventory.ledger().can_supply_furnace_recipe(recipe);
            CollectionItem::default()
                .with("#item_renderer_data", renderer)
                .with("#recipe_book_total_items", Scalar::Num(total))
                .with("#recipe_craftable_count", Scalar::Text(String::new()))
                .with("#recipe_hover_text", Scalar::Text(String::new()))
                .with("#is_recipe_selected_slot", Scalar::Bool(false))
                .with(
                    "#container_item_background_texture",
                    Scalar::Text(
                        if supplied {
                            "textures/ui/recipe_book_item_bg"
                        } else {
                            "textures/ui/recipe_book_red_button"
                        }
                        .to_owned(),
                    ),
                )
        })
        .collect();
    data.set_collection("recipe_book", rows);
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
        _ => return None,
    };
    Some(InventoryCellHit::Widget(widget))
}
