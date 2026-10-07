//! The inventory and crafting table's recipe book panel: the creative catalog
//! in creative, the craftable recipes otherwise, filed under the vanilla tabs.
//! Tab and layout toggles press the existing screen widgets.

mod cache;
#[cfg(test)]
mod tests;
pub(super) use cache::BookCache;

use json_ui::{CollectionItem, Context, DataSource, HitKind, HitRegion, Scalar};
use serde_json::Value;

use super::super::{HudFrame, IconRef};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::inventory_actions::{BookEntry, recipe_book_entries};
use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
use crate::ui_runtime::presentation::screens::{SEARCH_TAB, Widget};

/// The controller collection filled by the recipe book.
const COLLECTION: &str = "recipe_book";

/// Vanilla crafting screen variables: radio indexes of the tabs
/// and layout toggles.
const INDEXES: [(&str, u64); 9] = [
    ("construction_index", 1),
    ("equipment_index", 2),
    ("items_index", 3),
    ("nature_index", 4),
    ("search_index", 5),
    ("survival_index", 6),
    ("survival_layout_index", 1),
    ("recipe_book_layout_index", 2),
    ("creative_layout_index", 3),
];
/// Screen-state tabs by radio index: construction, equipment, items, nature, search.
const TABS: [(u64, u8); 5] = [(1, 0), (2, 2), (3, 3), (4, 1), (5, SEARCH_TAB)];
/// Tab labels by screen-state tab; the search tab's label is provisional.
const TAB_LABELS: [&str; 5] = [
    "craftingScreen.tab.construction",
    "craftingScreen.tab.nature",
    "craftingScreen.tab.equipment",
    "craftingScreen.tab.items",
    "craftingScreen.tab.allItems",
];
/// The filter toggle's `$toggle_name`.
const FILTER_TOGGLE: &str = "toggle.enableFiltering";
/// Cell backgrounds: a plain entry, a folded and an unfolded group head, and
/// an entry under an unfolded head.
const ITEM_BACKGROUND: &str = "textures/ui/recipe_book_item_bg";
const GROUP_FOLDED: &str = "textures/ui/recipe_book_light_button";
const GROUP_UNFOLDED: &str = "textures/ui/recipe_book_dark_button_pressed";
const GROUP_ITEM: &str = "textures/ui/recipe_book_dark_button";
/// A recipe the inventory cannot supply (`FilterResult::Disable`).
const RECIPE_DISABLED: &str = "textures/ui/recipe_book_red_button";

/// Whether the panel shows: creative opens on it and the toggle flips either way.
pub fn recipe_book_shown(player_runtime: &player_state::PlayerState, runtime: &UiRuntime) -> bool {
    let creative =
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
    creative != runtime.screen_state().book_open
}

/// The listed entries' icons, in list order.
pub fn recipe_book_icons(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    icon: impl Fn(&protocol::NetworkItemStack) -> Option<IconRef>,
) -> Vec<Option<IconRef>> {
    recipe_book_entries(player_runtime, runtime)
        .iter()
        .map(|entry| icon(&entry.stack()))
        .collect()
}

/// The stack a hovered entry's tooltip describes; a group head names its group.
pub fn recipe_book_hover(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    index: u16,
) -> Option<(protocol::NetworkItemStack, Option<std::sync::Arc<str>>)> {
    let entries = recipe_book_entries(player_runtime, runtime);
    let entry = entries.get(usize::from(index))?;
    let name = match entry {
        BookEntry::Group { group, .. } => Some(
            runtime
                .translation(&group.name)
                .unwrap_or_else(|| std::sync::Arc::clone(&group.name)),
        ),
        _ => None,
    };
    Some((entry.stack(), name))
}

/// The crafting screens' static variables.
pub(super) fn context(mut context: Context) -> Context {
    for (name, index) in INDEXES {
        context = context.with_var(name, Value::from(index));
    }
    context
}

/// The layout, tab and search globals and the `recipe_book` collection.
pub(super) fn book_data(
    player_runtime: &player_state::PlayerState,
    data: &mut DataSource,
    runtime: &UiRuntime,
    frame: &HudFrame,
    icons: &mut Vec<IconRef>,
    shown: bool,
    cache: &mut Option<BookCache>,
) {
    let creative =
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
    let state = runtime.screen_state();
    let tab = state.creative_tab;
    let wide = shown && creative && state.creative_wide;
    for (name, value) in [
        ("#is_survival_layout", !shown),
        ("#is_recipe_book_layout", shown && !wide),
        ("#is_creative_layout", wide),
        ("#is_creative_mode", creative),
        ("#is_creative_layout_button_visible", creative),
        (
            "#is_creative_and_recipe_book_layout",
            creative && shown && !wide,
        ),
        ("#is_creative_and_creative_layout", wide),
        (
            "#filtering_enabled",
            runtime.recipe_filtering(player_runtime),
        ),
        ("#is_left_tab_inventory", !shown),
        ("#construction_tab_visible", true),
        ("#equipment_tab_visible", true),
        ("#items_tab_visible", true),
        ("#nature_tab_visible", true),
        ("#is_left_tab_construct", tab == 0),
        ("#is_left_tab_nature", tab == 1),
        ("#is_left_tab_equipment", tab == 2),
        ("#is_left_tab_items", tab == 3),
        ("#is_left_tab_search", tab == SEARCH_TAB),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    let layout = match (shown, wide) {
        (false, _) => 1,
        (true, false) => 2,
        (true, true) => 3,
    };
    data.select_radio("layout_toggle", layout);
    if let Some((index, _)) = TABS.iter().find(|(_, java)| *java == tab) {
        data.select_radio("navigation_tab", *index as usize);
    }
    let label = TAB_LABELS[usize::from(tab.min(SEARCH_TAB))];
    let label = runtime
        .translation(label)
        .map_or_else(|| label.to_owned(), |text| text.to_string());
    data.set_global("#tab_label_text", Scalar::Text(label));
    data.set_global("#text_box_item_name", Scalar::Text(state.search.clone()));
    if !shown {
        return;
    }
    if BookCache::reuse(player_runtime, cache, runtime, frame, icons, data) {
        return;
    }
    let first_icon = icons.len();
    let entries = recipe_book_entries(player_runtime, runtime);
    let total = entries.len() as f64;
    let items: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut item =
                CollectionItem::default().with("#item_renderer_data", Scalar::Json(Value::Null));
            if let Some(icon) = frame
                .window_icons
                .book_entries
                .get(index)
                .copied()
                .flatten()
            {
                icons.push(icon);
                item = item.with("#item_renderer_data", Scalar::Num((icons.len() - 1) as f64));
            }
            let count = entry.stack().count;
            item.with(
                "#recipe_craftable_count",
                Scalar::Text(if count > 1 && !creative {
                    count.to_string()
                } else {
                    String::new()
                }),
            )
            .with("#recipe_hover_text", Scalar::Text(String::new()))
            .with("#is_creative_selected_slot", Scalar::Bool(false))
            .with(
                "#container_item_background_texture",
                Scalar::Text(background(player_runtime, runtime, entry).to_owned()),
            )
            .with("#recipe_book_total_items", Scalar::Num(total))
            .with("#container_item_modifier", Scalar::Int(modifier(entry)))
        })
        .collect();
    let items = std::sync::Arc::from(items);
    *cache = BookCache::capture(player_runtime, runtime, frame, first_icon, icons, &items);
    data.set_shared_collection(COLLECTION, items);
}

/// `#container_item_modifier`: a folded group head shows the expand icon (2),
/// an unfolded one the collapse icon (1), anything else neither.
fn modifier(entry: &BookEntry<'_>) -> i64 {
    match entry {
        BookEntry::Group {
            expanded: false, ..
        } => 2,
        BookEntry::Group { expanded: true, .. } => 1,
        _ => 0,
    }
}

fn background(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    entry: &BookEntry<'_>,
) -> &'static str {
    match entry {
        BookEntry::Recipe(recipe)
            if !runtime
                .inventory_ledger(player_runtime)
                .can_auto_craft(recipe) =>
        {
            RECIPE_DISABLED
        }
        BookEntry::Group {
            expanded: false, ..
        } => GROUP_FOLDED,
        BookEntry::Group { expanded: true, .. } => GROUP_UNFOLDED,
        BookEntry::Creative { grouped: true, .. } => GROUP_ITEM,
        _ => ITEM_BACKGROUND,
    }
}

/// The widget a recipe book control presses: an entry, a tab, the search
/// field, or the layout toggle that flips the panel.
pub(super) fn book_hit(region: &HitRegion, shown: bool) -> Option<InventoryCellHit> {
    if region.collection.as_deref() == Some(COLLECTION) {
        return Some(InventoryCellHit::RecipeBook(
            u16::try_from(region.collection_index?).ok()?,
        ));
    }
    if region.kind == HitKind::EditBox && shown {
        return Some(InventoryCellHit::CreativeSearch);
    }
    if region.control_name.as_deref() == Some(FILTER_TOGGLE) {
        return Some(InventoryCellHit::Widget(Widget::RecipeFilter));
    }
    let group = region.group_index? as u64;
    match region.control_name.as_deref()? {
        "navigation_tab" => {
            let (_, tab) = TABS.iter().find(|(index, _)| *index == group)?;
            Some(if *tab == SEARCH_TAB {
                InventoryCellHit::CreativeSearch
            } else {
                InventoryCellHit::CreativeTab(*tab)
            })
        }
        "layout_toggle" => Some(InventoryCellHit::Widget(Widget::InventoryLayout(
            u8::try_from(group).ok()?,
        ))),
        _ => None,
    }
}
