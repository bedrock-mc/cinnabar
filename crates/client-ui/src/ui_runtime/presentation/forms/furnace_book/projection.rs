use std::sync::Arc;

use protocol::ScreenRecipe;

use super::super::super::IconRef;
use super::{UiRuntime, category};

#[derive(Debug, Default)]
pub(crate) struct ProjectionCache {
    inputs: Option<Inputs>,
    positions: Arc<[usize]>,
    icons: Option<Icons>,
}

#[derive(Debug)]
struct Icons {
    positions: Arc<[usize]>,
    source: Option<Arc<crate::ui_runtime::presentation::SessionIcons>>,
    values: Arc<[Option<IconRef>]>,
}

#[derive(Debug)]
struct Inputs {
    supplied: Arc<[usize]>,
    tab: u8,
    query: String,
    components: Option<Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    base: Option<Arc<assets::RuntimeLangCatalog>>,
    active: Option<Arc<assets::RuntimeLangCatalog>>,
    server: Option<Arc<assets::ServerLangOverlay>>,
}

fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// Tab and search results share storage until their semantic inputs change.
pub fn projection(player: &player_state::PlayerState, runtime: &UiRuntime) -> Arc<[usize]> {
    let supplied = player
        .inventory
        .furnace_recipes(runtime.recipe_filtering(player));
    let state = runtime.screen_state();
    let tab = state.furnace_tab.unwrap_or(4);
    let mut cache = runtime.furnace_projection.lock().unwrap();
    let reusable = cache.inputs.as_ref().is_some_and(|previous| {
        Arc::ptr_eq(&previous.supplied, supplied.shared_indices())
            && previous.tab == tab
            && previous.query == state.search
            && same(&previous.components, &runtime.session_items)
            && same(&previous.base, &runtime.lang_catalog)
            && same(&previous.active, &runtime.active_lang)
            && same(&previous.server, &runtime.server_lang)
    });
    if !reusable {
        let query = state.search.to_lowercase();
        let catalog = player.inventory.screen_catalog();
        cache.positions = supplied
            .shared_indices()
            .iter()
            .copied()
            .filter(|index| {
                let Some(recipe) =
                    catalog.and_then(|catalog| catalog.screen_recipe_entries().get(*index))
                else {
                    return false;
                };
                if tab != 4 {
                    return category(player, runtime, recipe) == tab;
                }
                query.is_empty()
                    || player
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
            .collect();
        cache.inputs = Some(Inputs {
            supplied: Arc::clone(supplied.shared_indices()),
            tab,
            query: state.search.clone(),
            components: runtime.session_items.clone(),
            base: runtime.lang_catalog.clone(),
            active: runtime.active_lang.clone(),
            server: runtime.server_lang.clone(),
        });
    }
    Arc::clone(&cache.positions)
}

/// A shared projection borrowed against its unchanged recipe catalog.
pub struct Entries<'a> {
    catalog: &'a [ScreenRecipe],
    positions: Arc<[usize]>,
}

impl<'a> Entries<'a> {
    pub fn len(&self) -> usize {
        self.positions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&'a ScreenRecipe> {
        self.catalog.get(*self.positions.get(index)?)
    }
    pub fn iter(&self) -> impl Iterator<Item = &'a ScreenRecipe> + '_ {
        self.positions
            .iter()
            .filter_map(|index| self.catalog.get(*index))
    }
}

pub fn icons(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    icon: impl Fn(&protocol::NetworkItemStack) -> Option<IconRef>,
) -> Arc<[Option<IconRef>]> {
    let entries = entries(player, runtime);
    let mut cache = runtime.furnace_projection.lock().unwrap();
    if !cache.icons.as_ref().is_some_and(|cached| {
        Arc::ptr_eq(&cached.positions, &entries.positions)
            && same(&cached.source, &runtime.session_icons)
    }) {
        cache.icons = Some(Icons {
            positions: Arc::clone(&entries.positions),
            source: runtime.session_icons.clone(),
            values: entries
                .iter()
                .map(|recipe| icon(&super::output_stack(recipe)))
                .collect(),
        });
    }
    Arc::clone(&cache.icons.as_ref().unwrap().values)
}

pub fn entries<'a>(player: &'a player_state::PlayerState, runtime: &UiRuntime) -> Entries<'a> {
    Entries {
        catalog: player
            .inventory
            .screen_catalog()
            .map_or(&[], |catalog| catalog.screen_recipe_entries()),
        positions: projection(player, runtime),
    }
}
