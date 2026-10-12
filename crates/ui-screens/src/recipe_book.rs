//! The inventory and crafting table's recipe book panel: the creative catalog
//! in creative, the craftable recipes otherwise, filed under the vanilla tabs.
//! Tab and layout toggles press the existing screen widgets.

mod cache;
#[cfg(test)]
mod tests;
pub use cache::BookCache;

use json_ui::{CollectionItem, Context, DataSource, HitKind, HitRegion, Scalar};
use protocol::{CreativeGroup, CreativeItem, NetworkItemStack, RecipeHandle};
use serde_json::Value;

use crate::container_actions::{InventoryCellHit, SEARCH_TAB, Widget};
use crate::container_source::{ContainerFrame, ContainerSource};
use ui::IconRef;

/// The controller collection filled by the recipe book. Its cells have a literal pixel size,
/// draw at most a few pixels past it, and bind only per-index focus and collection details once.
pub const COLLECTION: &str = "recipe_book";

/// A container screen's starting live binding. The recipe grid builds only the cells near view
/// while no server pack's ui files apply, as those may draw its cells anywhere.
pub(crate) fn screen_binding(server_ui: bool) -> json_ui::BindState {
    let mut binding = json_ui::BindState::new();
    if !server_ui {
        binding.virtualize_grid(COLLECTION);
    }
    binding
}

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

/// One entry of the recipe book: in creative a catalog item or a named group's
/// head, else a recipe the player can craft now.
pub enum BookEntry<'a> {
    Creative {
        item: &'a CreativeItem,
        /// Listed under an expanded group's head.
        grouped: bool,
    },
    Group {
        index: u32,
        group: &'a CreativeGroup,
        expanded: bool,
    },
    Recipe(RecipeHandle),
    Furnace(&'a protocol::ScreenRecipe),
}

impl BookEntry<'_> {
    /// The stack the entry shows; a group head shows its icon.
    pub fn stack(&self) -> NetworkItemStack {
        match self {
            Self::Creative { item, .. } => item.stack.clone(),
            Self::Group { group, .. } => group.icon.clone().unwrap_or_default(),
            Self::Recipe(recipe) => {
                let output = recipe.output();
                NetworkItemStack {
                    network_id: output.network_id,
                    metadata: u32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: i32::try_from(output.block_runtime_id).unwrap_or(0),
                    ..NetworkItemStack::empty()
                }
            }
            Self::Furnace(recipe) => crate::furnace_book::output_stack(recipe),
        }
    }
}

/// The crafting screens' static variables.
pub(crate) fn context(mut context: Context) -> Context {
    for (name, index) in INDEXES {
        context = context.with_var(name, Value::from(index));
    }
    context
}

/// The layout, tab and search globals and the `recipe_book` collection.
pub fn book_data(
    source: &dyn ContainerSource,
    data: &mut DataSource,
    frame: &ContainerFrame<'_>,
    icons: &mut Vec<IconRef>,
    shown: bool,
    cache: &mut Option<BookCache>,
) {
    let creative =
        source.player().facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
    let state = source.screen();
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
        ("#filtering_enabled", source.recipe_filtering()),
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
    let label = source
        .translation(label)
        .map_or_else(|| label.to_owned(), |text| text.to_string());
    data.set_global("#tab_label_text", Scalar::Text(label));
    data.set_global("#text_box_item_name", Scalar::Text(state.search.to_owned()));
    if !shown {
        return;
    }
    data.set_collection_defaults(
        COLLECTION,
        [("#recipe_book_total_items".into(), Scalar::Num(0.0))].into(),
    );
    if BookCache::reuse(cache, source, frame, icons, data) {
        return;
    }
    let first_icon = icons.len();
    let entries = source.book_entries();
    let total = entries.len() as f64;
    let items: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut item =
                CollectionItem::default().with("#item_renderer_data", Scalar::Json(Value::Null));
            if let Some(icon) = frame.book_entries.get(index).copied().flatten() {
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
                Scalar::Text(background(source, entry).to_owned()),
            )
            .with("#recipe_book_total_items", Scalar::Num(total))
            .with("#container_item_modifier", Scalar::Int(modifier(entry)))
        })
        .collect();
    let items = std::sync::Arc::from(items);
    *cache = BookCache::capture(source, frame, first_icon, icons, &items);
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

fn background(source: &dyn ContainerSource, entry: &BookEntry<'_>) -> &'static str {
    match entry {
        BookEntry::Recipe(recipe) if !source.player().inventory.ledger().can_auto_craft(recipe) => {
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
pub(crate) fn book_hit(region: &HitRegion, shown: bool) -> Option<InventoryCellHit> {
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
