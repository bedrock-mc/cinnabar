//! The inventory and crafting table's recipe book panel: the creative catalog
//! in creative, the craftable recipes otherwise, filed under the vanilla tabs.
//! Tab and layout toggles press the existing screen widgets.

mod cache;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod tests;
pub use cache::BookCache;

use json_ui::{CollectionItem, Context, DataSource, HitKind, HitRegion, Scalar};
use protocol::{
    CreativeGroup, CreativeItem, InventoryLayout, InventoryRightTab, InventoryTab, NetworkItemStack,
};
use serde_json::Value;

use crate::container_actions::{InventoryCellHit, SEARCH_TAB, Widget};
use crate::container_source::{ContainerFrame, ContainerSource};
use ui::IconRef;

/// The controller collection filled by the recipe book. Its cells have a literal pixel size,
/// draw at most a few pixels past it, and bind only per-index focus and collection details once.
pub const COLLECTION: &str = "recipe_book";

/// Builds nearby recipe cells only when the screen and templates retain the base behavior.
pub(crate) fn screen_binding(virtualize: bool) -> json_ui::BindState {
    let mut binding = json_ui::BindState::new();
    if virtualize {
        binding.virtualize_grid(COLLECTION);
    }
    binding
}

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
/// A recipe output whose ingredients the inventory cannot supply.
const RECIPE_DISABLED: &str = "textures/ui/recipe_book_red_button";

/// One entry of the recipe book: in creative a catalog item or a named group's
/// head, else a grouped recipe output and its current material availability.
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
    Recipe(inventory::BookRecipe),
    Furnace(&'a protocol::ScreenRecipe),
}

impl BookEntry<'_> {
    /// The stack the entry shows; a group head shows its icon.
    pub fn stack(&self) -> NetworkItemStack {
        match self {
            Self::Creative { item, .. } => item.stack.clone(),
            Self::Group { group, .. } => group.icon.clone().unwrap_or_default(),
            Self::Recipe(recipe) => {
                let output = recipe.recipe.output();
                NetworkItemStack {
                    network_id: output.network_id,
                    metadata: u32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
                    ..NetworkItemStack::empty()
                }
            }
            Self::Furnace(recipe) => crate::furnace_book::output_stack(recipe),
        }
    }
}

/// The crafting screens' static variables.
pub(crate) fn context(mut context: Context) -> Context {
    for (name, index) in [
        ("construction_index", InventoryTab::Construction.index()),
        ("equipment_index", InventoryTab::Equipment.index()),
        ("items_index", InventoryTab::Items.index()),
        ("nature_index", InventoryTab::Nature.index()),
        ("search_index", InventoryTab::Search.index()),
        ("survival_index", InventoryTab::Survival.index()),
        (
            "survival_layout_index",
            InventoryLayout::InventoryOnly.index(),
        ),
        (
            "recipe_book_layout_index",
            InventoryLayout::RecipeBook.index(),
        ),
        (
            "creative_layout_index",
            InventoryLayout::RecipeBookOnly.index(),
        ),
        ("fullscreen_index", InventoryRightTab::Fullscreen.index()),
        ("crafting_index", InventoryRightTab::Crafting.index()),
        ("armor_index", InventoryRightTab::Armor.index()),
    ] {
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
    let controller = state.inventory;
    let tab = controller.options.left_tab;
    option_bindings(data, controller, creative, shown);
    let label = crate::container_actions::inventory_tab_ui(tab)
        .and_then(|tab| TAB_LABELS.get(usize::from(tab)))
        .copied()
        .unwrap_or("craftingScreen.tab.allItems");
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
    let facts = source.player().inventory.ledger().item_facts();
    let row = |(index, entry): (usize, &BookEntry<'_>)| {
        let stack = entry.stack();
        cache::RowKey {
            count: stack.count,
            id_aux: facts.id_aux(&stack),
            background: background(source, entry),
            modifier: modifier(entry),
            icon: frame.book_entries.get(index).copied().flatten(),
        }
    };
    if BookCache::reuse_rows(
        cache,
        creative,
        entries.iter().enumerate().map(row),
        icons,
        data,
    ) {
        return;
    }
    let total = entries.len() as f64;
    let rows: Vec<_> = entries.iter().enumerate().map(row).collect();
    let items: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut item =
                CollectionItem::default().with("#item_renderer_data", Scalar::Json(Value::Null));
            if let Some(icon) = row.icon {
                icons.push(icon);
                item = item.with("#item_renderer_data", Scalar::Num((icons.len() - 1) as f64));
            }
            item.with("#item_id_aux", Scalar::Int(i64::from(row.id_aux)))
                .with(
                    "#recipe_craftable_count",
                    Scalar::Text(if row.count > 1 && !creative {
                        row.count.to_string()
                    } else {
                        String::new()
                    }),
                )
                .with("#recipe_hover_text", Scalar::Text(String::new()))
                .with("#is_creative_selected_slot", Scalar::Bool(false))
                .with(
                    "#container_item_background_texture",
                    Scalar::Text(row.background.to_owned()),
                )
                .with("#recipe_book_total_items", Scalar::Num(total))
                .with("#container_item_modifier", Scalar::Int(row.modifier))
        })
        .collect();
    let items = std::sync::Arc::from(items);
    *cache = Some(
        BookCache::capture(source, frame, first_icon, icons, &items)
            .unwrap_or_else(|| BookCache::capture_rows(creative, rows, first_icon, icons, &items)),
    );
    data.set_shared_collection(COLLECTION, items);
}

/// Publishes navigation and layout choices independently of recipe row availability.
fn option_bindings(
    data: &mut DataSource,
    controller: inventory::InventoryController,
    creative: bool,
    shown: bool,
) {
    let options = controller.options;
    let tab = options.left_tab;
    let wide = shown
        && controller.allows_recipe_book_only(creative)
        && controller.layout() == InventoryLayout::RecipeBookOnly;
    for (name, value) in [
        ("#is_survival_layout", !shown),
        ("#is_recipe_book_layout", shown && !wide),
        ("#is_creative_layout", wide),
        ("#is_creative_mode", creative),
        (
            "#is_creative_layout_button_visible",
            controller.allows_recipe_book_only(creative),
        ),
        (
            "#is_creative_and_recipe_book_layout",
            creative && shown && !wide,
        ),
        ("#is_creative_and_creative_layout", wide),
        ("#filtering_enabled", options.filtering),
        ("#is_left_tab_inventory", tab == InventoryTab::Survival),
        (
            "#is_right_tab_fullscreen",
            options.right_tab == InventoryRightTab::Fullscreen,
        ),
        (
            "#is_right_tab_crafting",
            options.right_tab == InventoryRightTab::Crafting,
        ),
        (
            "#is_right_tab_armor",
            options.right_tab == InventoryRightTab::Armor,
        ),
        ("#construction_tab_visible", true),
        ("#equipment_tab_visible", true),
        ("#items_tab_visible", true),
        ("#nature_tab_visible", true),
        ("#is_left_tab_construct", tab == InventoryTab::Construction),
        ("#is_left_tab_nature", tab == InventoryTab::Nature),
        ("#is_left_tab_equipment", tab == InventoryTab::Equipment),
        ("#is_left_tab_items", tab == InventoryTab::Items),
        ("#is_left_tab_search", tab == InventoryTab::Search),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    if let Ok(layout) = usize::try_from(controller.layout().index()) {
        data.select_radio("layout_toggle", layout);
    }
    if let Ok(index) = usize::try_from(tab.index()) {
        data.select_radio("navigation_tab", index);
    }
    if let Ok(index) = usize::try_from(options.right_tab.index()) {
        data.select_radio("right_inventory_navigation_tab", index);
    }
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

/// Chooses a row's appearance from material availability and group state.
fn background(source: &dyn ContainerSource, entry: &BookEntry<'_>) -> &'static str {
    match entry {
        BookEntry::Recipe(recipe) => recipe_background(source.player(), recipe),
        BookEntry::Group {
            expanded: false, ..
        } => GROUP_FOLDED,
        BookEntry::Group { expanded: true, .. } => GROUP_UNFOLDED,
        BookEntry::Creative { grouped: true, .. } => GROUP_ITEM,
        _ => ITEM_BACKGROUND,
    }
}

/// Selects a recipe row's background independently of group and selection artwork.
fn recipe_background(
    player: &player_state::PlayerState,
    recipe: &inventory::BookRecipe,
) -> &'static str {
    if recipe.craftable
        || player.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative)
    {
        ITEM_BACKGROUND
    } else {
        RECIPE_DISABLED
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
    navigation_hit(region.control_name.as_deref()?, group)
}

/// Converts a navigation radio into the client control that owns its selection.
fn navigation_hit(control: &str, group: u64) -> Option<InventoryCellHit> {
    match control {
        "navigation_tab" => {
            let tab = crate::container_actions::inventory_tab_ui(InventoryTab::from_index(
                i32::try_from(group).ok()?,
            ))?;
            Some(if tab == SEARCH_TAB {
                InventoryCellHit::CreativeSearch
            } else {
                InventoryCellHit::CreativeTab(tab)
            })
        }
        "right_inventory_navigation_tab" => Some(InventoryCellHit::Widget(
            Widget::InventoryRightTab(u8::try_from(group).ok()?),
        )),
        "layout_toggle" => Some(InventoryCellHit::Widget(Widget::InventoryLayout(
            u8::try_from(group).ok()?,
        ))),
        _ => None,
    }
}
