//! Transient state of the open inventory screen: hover, drag, creative tab,
//! search text and beacon effect choice.

use protocol::CreativeCategory;

mod creative_filter;
use creative_filter::CreativeFilterCache;
pub use creative_filter::{CreativeEntries, creative_entries};

use super::inventory_drag::InventoryPointer;
use super::presentation::inventory_pointer::InventoryCellHit;
use super::presentation::screens::{GRID_CELLS, GRID_COLUMNS, SEARCH_TAB};

/// Longest creative search text.
const MAX_SEARCH_CHARS: usize = 32;
/// Longest item name an anvil accepts.
const MAX_ANVIL_NAME_CHARS: usize = 50;

#[derive(Clone, Debug, Default)]
pub struct ScreenState {
    pub pointer: InventoryPointer,
    pub hover: Option<InventoryCellHit>,
    /// Chosen beacon effects; `0` means none.
    pub beacon: (i32, i32),
    pub creative_tab: u8,
    /// First visible row of the creative grid.
    pub creative_row: usize,
    pub search: String,
    pub search_focused: bool,
    /// The stonecutter recipe the player picked.
    pub recipe_choice: Option<u32>,
    pub loom_pattern: Option<std::sync::Arc<str>>,
    /// First visible row of the loom pattern grid.
    pub loom_row: usize,
    pub anvil_name: String,
    pub anvil_focused: bool,
    /// The beacon's pyramid level, from its block entity.
    pub beacon_level: Option<u8>,
    pub crafter: CrafterView,
    /// The open mount inventory's entity identifier.
    pub mount_identifier: Option<std::sync::Arc<str>>,
    pub book_open: bool,
    /// Creative's wide list stands in for its recipe book layout.
    pub creative_wide: bool,
    /// The recipe book's filter toggle, once flipped on this screen.
    pub recipe_filtering: Option<bool>,
    /// The book, sign-off or lectern reader being shown, if any.
    pub book: Option<super::book_screen::BookState>,
    /// Page of the recipe book, in whole grids.
    pub book_page: usize,
    /// Creative catalog groups the player expanded, by group index.
    pub creative_expanded: std::collections::BTreeSet<u32>,
    /// Scroll offsets of the engine-drawn screen's scroll views, by view key.
    pub container_scroll: std::collections::BTreeMap<String, f64>,
    window: Option<u64>,
    creative_filter: std::sync::Arc<std::sync::Mutex<Option<CreativeFilterCache>>>,
}

/// What the open crafter's screen shows of its block.
#[derive(Clone, Debug, Default)]
pub struct CrafterView {
    /// Disabled slots as the block entity reports them, one bit per slot.
    pub disabled: u16,
    /// The block's `triggered_bit`.
    pub powered: bool,
    /// Toggles not yet echoed: the local mask and when it was set, stamped on
    /// the next observation.
    pub pending: Option<(u16, Option<u64>)>,
}

/// How long vanilla's local crafter toggles override the block entity.
pub const CRAFTER_TOGGLE_HOLD_MILLIS: u64 = 1_000;

impl CrafterView {
    /// The disabled slots the screen shows.
    pub fn shown_disabled(&self) -> u16 {
        self.pending.map_or(self.disabled, |(mask, _)| mask)
    }

    pub fn is_disabled(&self, slot: u8) -> bool {
        slot < 9 && self.shown_disabled() & (1 << slot) != 0
    }

    /// Adopts the block entity's state, keeping recent local toggles.
    pub fn observe(&mut self, disabled: u16, powered: bool, now_millis: u64) {
        self.disabled = disabled & 0x1ff;
        self.powered = powered;
        self.pending = match self.pending {
            Some((mask, None)) => Some((mask, Some(now_millis))),
            Some((mask, Some(at))) if now_millis <= at + CRAFTER_TOGGLE_HOLD_MILLIS => {
                Some((mask, Some(at)))
            }
            _ => None,
        };
    }
}

impl ScreenState {
    /// Clears per-window choices when a different window becomes current.
    pub fn observe_window(&mut self, generation: Option<u64>) {
        if self.window != generation {
            self.window = generation;
            self.pointer.reset();
            self.beacon = (0, 0);
            self.search_focused = false;
            self.recipe_choice = None;
            self.loom_pattern = None;
            self.loom_row = 0;
            self.anvil_name.clear();
            self.anvil_focused = false;
            self.beacon_level = None;
            self.crafter = CrafterView::default();
            self.mount_identifier = None;
            self.container_scroll.clear();
            self.creative_expanded.clear();
            self.recipe_filtering = None;
        }
    }

    pub fn select_tab(&mut self, tab: u8) {
        self.search_focused = tab == SEARCH_TAB;
        self.creative_tab = tab;
        self.creative_row = 0;
        self.container_scroll.clear();
    }

    /// Whether a text field owns the keyboard.
    pub fn text_focused(&self) -> bool {
        self.search_focused
            || self.anvil_focused
            || self.book.as_ref().is_some_and(|book| book.editable)
    }

    /// Appends typed text to the focused field, dropping control characters.
    pub fn type_text(&mut self, text: &str) {
        if let Some(book) = self.book.as_mut().filter(|book| book.editable) {
            book.type_text(text);
            return;
        }
        let (field, limit) = if self.anvil_focused {
            (&mut self.anvil_name, MAX_ANVIL_NAME_CHARS)
        } else {
            (&mut self.search, MAX_SEARCH_CHARS)
        };
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            if field.chars().count() < limit {
                field.push(ch);
            }
        }
        self.creative_row = 0;
        self.container_scroll.clear();
    }

    pub fn backspace_text(&mut self) {
        if let Some(book) = self.book.as_mut().filter(|book| book.editable) {
            book.backspace();
        } else if self.anvil_focused {
            self.anvil_name.pop();
        } else {
            self.search.pop();
            self.creative_row = 0;
            self.container_scroll.clear();
        }
    }

    /// Scrolls the loom pattern grid by whole rows.
    pub fn scroll_loom(&mut self, rows: isize, total: usize) {
        let max_row = total
            .div_ceil(super::presentation::screens::LOOM_COLUMNS)
            .saturating_sub(
                super::presentation::screens::LOOM_CELLS
                    / super::presentation::screens::LOOM_COLUMNS,
            );
        self.loom_row = self.loom_row.saturating_add_signed(rows).min(max_row);
    }

    /// Scrolls the creative grid by whole rows, keeping the last page reachable.
    pub fn scroll_creative(&mut self, rows: isize, total: usize) {
        let max_row = total
            .div_ceil(GRID_COLUMNS)
            .saturating_sub(GRID_CELLS / GRID_COLUMNS);
        self.creative_row = self.creative_row.saturating_add_signed(rows).min(max_row);
    }
}

fn tab_category(tab: u8) -> Option<CreativeCategory> {
    match tab {
        0 => Some(CreativeCategory::Construction),
        1 => Some(CreativeCategory::Nature),
        2 => Some(CreativeCategory::Equipment),
        3 => Some(CreativeCategory::Items),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{CreativeContentEvent, CreativeGroup, CreativeItem, NetworkItemStack};

    use super::*;

    // Local toggles show until the block entity has had a second to echo them.
    #[test]
    fn crafter_toggles_hold_before_the_block_entity_wins() {
        let mut crafter = CrafterView {
            pending: Some((0b10, None)),
            ..CrafterView::default()
        };
        crafter.observe(0, false, 100);
        assert_eq!(crafter.shown_disabled(), 0b10);
        crafter.observe(0, false, 100 + CRAFTER_TOGGLE_HOLD_MILLIS);
        assert_eq!(crafter.shown_disabled(), 0b10);
        crafter.observe(0b1, true, 101 + CRAFTER_TOGGLE_HOLD_MILLIS);
        assert_eq!(crafter.shown_disabled(), 0b1);
        assert!(crafter.powered);
    }

    fn catalog() -> CreativeContentEvent {
        let item = |id: u32, group: u32| CreativeItem {
            creative_network_id: id,
            stack: NetworkItemStack::default(),
            group,
        };
        CreativeContentEvent {
            groups: Arc::from([
                CreativeGroup {
                    category: CreativeCategory::Construction,
                    name: Arc::from("a"),
                    icon: None,
                },
                CreativeGroup {
                    category: CreativeCategory::Nature,
                    name: Arc::from("b"),
                    icon: None,
                },
                CreativeGroup {
                    category: CreativeCategory::CommandOnly,
                    name: Arc::from("c"),
                    icon: None,
                },
            ]),
            items: Arc::from([item(1, 0), item(2, 1), item(3, 2), item(4, 0)]),
            skipped: 0,
        }
    }

    #[test]
    fn tabs_filter_by_category_and_hide_command_only() {
        let catalog = catalog();
        let ids = |tab| {
            creative_entries(&catalog, tab, "", |_| None)
                .iter()
                .map(|item| item.creative_network_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(0), vec![1, 4]);
        assert_eq!(ids(1), vec![2]);
        assert_eq!(ids(SEARCH_TAB), vec![1, 2, 4]);
    }

    #[test]
    fn search_matches_names_case_insensitively() {
        let catalog = catalog();
        let found = creative_entries(&catalog, SEARCH_TAB, "STONE", |item| {
            (item.creative_network_id == 2).then(|| "Stone Brick".to_owned())
        });
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn scrolling_stops_at_the_last_page() {
        let mut state = ScreenState::default();
        state.scroll_creative(50, 100);
        assert_eq!(state.creative_row, 100_usize.div_ceil(9) - 5);
        state.scroll_creative(-50, 100);
        assert_eq!(state.creative_row, 0);
    }
}
