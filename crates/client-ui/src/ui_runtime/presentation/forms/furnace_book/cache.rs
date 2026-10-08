use std::sync::Arc;

use json_ui::{CollectionItem, Scalar};
use serde_json::Value;

use super::{HudFrame, IconRef, UiRuntime, category, entries};

pub(in super::super) struct FurnaceBookCache {
    all: Arc<[usize]>,
    supplied: Arc<[usize]>,
    shown: bool,
    filtering: bool,
    tab: Option<u8>,
    search: String,
    components: Option<Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    lang: Option<Arc<assets::RuntimeLangCatalog>>,
    source_icons: Vec<Option<IconRef>>,
    first_icon: usize,
    pub(super) categories: [bool; 3],
    pub(super) icons: Vec<IconRef>,
    pub(super) rows: Arc<[CollectionItem]>,
}

fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

pub(super) fn publication<'a>(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    first_icon: usize,
    cache: &'a mut Option<FurnaceBookCache>,
) -> &'a FurnaceBookCache {
    let all = player.inventory.furnace_recipes(false);
    let supplied = player.inventory.furnace_recipes(true);
    let state = runtime.screen_state();
    let filtering = runtime.recipe_filtering(player);
    let shown = state.furnace_book_open;
    let reusable = cache.as_ref().is_some_and(|cache| {
        Arc::ptr_eq(&cache.all, all.shared_indices())
            && Arc::ptr_eq(&cache.supplied, supplied.shared_indices())
            && cache.shown == shown
            && cache.filtering == filtering
            && cache.tab == state.furnace_tab
            && cache.search == state.search
            && cache.first_icon == first_icon
            && cache.source_icons == frame.window_icons.book_entries
            && same(&cache.components, &runtime.session_items)
            && same(&cache.lang, &runtime.lang_catalog)
    });
    if !reusable {
        let categories = std::array::from_fn(|index| {
            all.iter()
                .any(|recipe| category(player, runtime, recipe) == index as u8 + 1)
        });
        let mut icons = Vec::new();
        let rows = if shown {
            rows(player, runtime, frame, first_icon, &mut icons)
        } else {
            Arc::from([])
        };
        *cache = Some(FurnaceBookCache {
            all: Arc::clone(all.shared_indices()),
            supplied: Arc::clone(supplied.shared_indices()),
            shown,
            filtering,
            tab: state.furnace_tab,
            search: state.search.clone(),
            components: runtime.session_items.clone(),
            lang: runtime.lang_catalog.clone(),
            source_icons: frame.window_icons.book_entries.clone(),
            first_icon,
            categories,
            icons,
            rows,
        });
    }
    cache.as_ref().unwrap()
}

fn rows(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    first_icon: usize,
    icons: &mut Vec<IconRef>,
) -> Arc<[CollectionItem]> {
    let entries = entries(player, runtime);
    let total = entries.len() as f64;
    entries
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
                Scalar::Num((first_icon + icons.len() - 1) as f64)
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
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furnace_recipe_rows_reuse_scroll_and_hover_frames() {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::inventory_session(&mut player);
        runtime.screen_state_mut().furnace_book_open = true;
        let frame = HudFrame::default();
        let mut cache = None;
        let first = Arc::clone(&publication(&player, &runtime, &frame, 0, &mut cache).rows);
        runtime
            .screen_state_mut()
            .container_scroll
            .insert("recipes".into(), 24.0);
        runtime.screen_state_mut().search_focused = true;
        assert!(Arc::ptr_eq(
            &first,
            &publication(&player, &runtime, &frame, 0, &mut cache).rows
        ));
        runtime.screen_state_mut().search = "iron".into();
        let searched = Arc::clone(&publication(&player, &runtime, &frame, 0, &mut cache).rows);
        assert!(!Arc::ptr_eq(&first, &searched));
        assert!(Arc::ptr_eq(
            &searched,
            &publication(&player, &runtime, &frame, 0, &mut cache).rows
        ));
    }
}
