use std::sync::Arc;

use json_ui::{CollectionItem, Scalar};
use serde_json::Value;

use {
    crate::container_source::{ContainerFrame, ContainerSource, NameSources},
    ui::IconRef,
};

/// The furnace rows last published, with every input they were built from.
pub struct FurnaceBookCache {
    all: Arc<[usize]>,
    supplied: Arc<[usize]>,
    projection: Arc<[usize]>,
    shown: bool,
    filtering: bool,
    tab: Option<i32>,
    search: String,
    names: NameSources,
    source_icons: Arc<[Option<IconRef>]>,
    first_icon: usize,
    selected: Option<(i32, u32, i32)>,
    pub(super) categories: [bool; 3],
    pub(super) icons: Vec<IconRef>,
    pub(super) rows: Arc<[CollectionItem]>,
}

/// The furnace rows for this frame, reusing the cached allocation while every input matches.
pub fn publication<'a>(
    source: &dyn ContainerSource,
    frame: &ContainerFrame<'_>,
    first_icon: usize,
    cache: &'a mut Option<FurnaceBookCache>,
) -> &'a FurnaceBookCache {
    let player = source.player();
    let all = player.inventory.furnace_recipes(false);
    let supplied = player.inventory.furnace_recipes(true);
    let projection = source.furnace_projection();
    let state = source.screen();
    let filtering = source.recipe_filtering();
    let names = source.name_sources();
    let shown = state.furnace_book_open;
    let selected = player
        .inventory
        .ledger()
        .selected_furnace_result()
        .map(|stack| (stack.network_id, stack.metadata, stack.block_runtime_id));
    let reusable = cache.as_ref().is_some_and(|cache| {
        Arc::ptr_eq(&cache.all, all.shared_indices())
            && Arc::ptr_eq(&cache.supplied, supplied.shared_indices())
            && Arc::ptr_eq(&cache.projection, &projection)
            && cache.shown == shown
            && cache.filtering == filtering
            && cache.tab == state.furnace_tab
            && cache.search == state.search
            && cache.first_icon == first_icon
            && cache.selected == selected
            && Arc::ptr_eq(&cache.source_icons, frame.furnace_entries)
            && cache.names.same(&names)
    });
    if !reusable {
        let categories = std::array::from_fn(|index| {
            all.iter()
                .any(|recipe| source.furnace_category(recipe) == index as u8 + 1)
        });
        let mut icons = Vec::new();
        let rows = if shown {
            rows(source, frame, first_icon, &mut icons)
        } else {
            Arc::from([])
        };
        *cache = Some(FurnaceBookCache {
            all: Arc::clone(all.shared_indices()),
            supplied: Arc::clone(supplied.shared_indices()),
            projection,
            shown,
            filtering,
            tab: state.furnace_tab,
            search: state.search.to_owned(),
            names,
            source_icons: Arc::clone(frame.furnace_entries),
            first_icon,
            selected,
            categories,
            icons,
            rows,
        });
    }
    cache.as_ref().unwrap()
}

fn rows(
    source: &dyn ContainerSource,
    frame: &ContainerFrame<'_>,
    first_icon: usize,
    icons: &mut Vec<IconRef>,
) -> Arc<[CollectionItem]> {
    let player = source.player();
    let entries = source.furnace_entries();
    let total = entries.len() as f64;
    entries
        .iter()
        .enumerate()
        .map(|(index, recipe)| {
            let icon = frame.furnace_entries.get(index).copied().flatten();
            let renderer = icon.map_or(Scalar::Json(Value::Null), |icon| {
                icons.push(icon);
                Scalar::Num((first_icon + icons.len() - 1) as f64)
            });
            let supplied = player.inventory.can_supply_furnace_result(recipe);
            let selected = player
                .inventory
                .ledger()
                .selected_furnace_result()
                .is_some_and(|selected| {
                    recipe.output.is_some_and(|output| {
                        selected.network_id == output.network_id
                            && selected.metadata == u32::from(output.aux)
                            && selected.block_runtime_id == output.block_runtime_id as i32
                    })
                });
            CollectionItem::default()
                .with("#item_renderer_data", renderer)
                .with("#recipe_book_total_items", Scalar::Num(total))
                .with("#recipe_craftable_count", Scalar::Text(String::new()))
                .with("#recipe_hover_text", Scalar::Text(String::new()))
                .with("#is_recipe_selected_slot", Scalar::Bool(selected))
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
        .collect()
}

#[cfg(any(test, feature = "test-support"))]
impl FurnaceBookCache {
    /// The published rows.
    pub fn rows(&self) -> &Arc<[CollectionItem]> {
        &self.rows
    }
}
