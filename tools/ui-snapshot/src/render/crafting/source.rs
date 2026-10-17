//! Borrowed adapter from real session authority to the production container painter.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::RuntimeLangCatalog;
use player_state::PlayerState;
use protocol::ScreenRecipe;
use ui_screens::{
    container_source::{ContainerSource, NameSources, ScreenView},
    furnace_book::Entries,
    recipe_book::BookEntry,
};

pub struct Source<'a> {
    pub player: &'a PlayerState,
    pub lang: &'a RuntimeLangCatalog,
    pub screen: ScreenView<'a>,
}

impl<'a> Source<'a> {
    /// Opens the survival recipe book on its unfiltered all-items tab, with no station controls.
    pub fn new(
        player: &'a PlayerState,
        lang: &'a RuntimeLangCatalog,
        groups: &'a BTreeSet<u32>,
        scrolls: &'a BTreeMap<String, f64>,
    ) -> Self {
        Self {
            player,
            lang,
            screen: ScreenView {
                mount_equipment: Default::default(),
                anvil_name: "",
                anvil_cost: None,
                recipe_choice: None,
                beacon: (0, 0),
                beacon_level: None,
                loom_pattern: None,
                crafter_disabled: [false; 9],
                crafter_powered: None,
                furnace_book_open: false,
                furnace_tab: None,
                inventory: inventory::InventoryController::new(
                    Default::default(),
                    inventory::InventoryContext::Personal,
                    false,
                ),
                creative_expanded: groups,
                search: "",
                container_scroll: scrolls,
                text_focused: false,
                book: None,
            },
        }
    }
}

impl ContainerSource for Source<'_> {
    fn player(&self) -> &PlayerState {
        self.player
    }
    fn screen(&self) -> &ScreenView<'_> {
        &self.screen
    }
    fn translation(&self, key: &str) -> Option<Arc<str>> {
        self.lang.lookup(key)
    }
    fn experience_level(&self) -> u32 {
        0
    }
    fn enchantment_name(&self, _: i16, _: u8) -> String {
        String::new()
    }
    fn recipe_filtering(&self) -> bool {
        false
    }
    fn item_name_generation(&self) -> u64 {
        0
    }
    fn book_entries(&self) -> Vec<BookEntry<'_>> {
        self.player
            .inventory
            .book_recipes(
                self.recipe_filtering(),
                !self.screen.search.is_empty(),
                0,
                usize::MAX,
            )
            .into_iter()
            .map(BookEntry::Recipe)
            .collect()
    }
    fn furnace_entries(&self) -> Entries<'_> {
        Entries::new(&[], Arc::from([]))
    }
    fn furnace_projection(&self) -> Arc<[usize]> {
        Arc::from([])
    }
    fn furnace_category(&self, _: &ScreenRecipe) -> u8 {
        0
    }
    fn name_sources(&self) -> NameSources {
        NameSources::default()
    }
}
