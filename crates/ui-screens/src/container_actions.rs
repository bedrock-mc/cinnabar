//! What a press on a container screen addresses, shared by the engine-drawn screens, the
//! Java-styled fallback and the client's inventory input.

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InventoryCellHit {
    Player(u8),
    Storage(u8),
    Armor(u8),
    Offhand,
    /// A screen input or crafting cell by UI inventory slot.
    Craft(u8),
    /// The created-output cell of a crafting-style screen.
    CraftOutput,
    Widget(Widget),
    /// A creative catalog grid cell by position on the current page.
    CreativeGrid(u8),
    CreativeTab(u8),
    CreativeSearch,
    /// An entry of the engine-drawn recipe book (the creative catalog in
    /// creative) by position in its list.
    RecipeBook(u16),
}

impl InventoryCellHit {
    /// Whether the hit is a cell holding an item (not a control or tab).
    pub const fn is_item_cell(self) -> bool {
        if matches!(
            self,
            Self::Widget(Widget::TradeOffer { item: Some(_), .. } | Widget::TradePreview(_))
        ) {
            return true;
        }
        !matches!(
            self,
            Self::Widget(_) | Self::CreativeTab(_) | Self::CreativeSearch
        )
    }
}

/// A clickable control that is not an item cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Widget {
    TradeOffer {
        tier: usize,
        row: usize,
        /// The hovered input/input/output preview, when the hit is on an item.
        item: Option<u8>,
    },
    /// A selected offer's preview in the classic layout.
    TradePreview(u8),
    TradeCycle {
        next: bool,
    },
    Trade,
    EnchantOption(u8),
    BeaconEffect {
        id: i32,
        secondary: bool,
    },
    /// The secondary effect that upgrades the chosen primary one.
    BeaconUpgrade,
    BeaconConfirm,
    /// One stonecutter recipe cell by position on the visible page.
    StonecutterRecipe(u8),
    /// One loom pattern cell by position on the visible page.
    LoomPattern(u8),
    /// One loom pattern by position in the whole list.
    LoomPatternAt(u8),
    AnvilName,
    /// Opens or closes the recipe book.
    BookToggle,
    /// Flips the survival recipe book between craftable-only and every recipe.
    RecipeFilter,
    /// Picks the inventory layout by its radio index: survival, recipe book, or
    /// the creative wide list.
    InventoryLayout(u8),
    /// Selects the inventory's right panel by its wire radio index.
    InventoryRightTab(u8),
    FurnaceTab(u8),
    FurnaceClearRecipe,
    /// Re-enables one disabled crafter slot.
    CrafterSlot(u8),
    /// One recipe cell by position on the visible page.
    BookRecipe(u8),
    BookPage {
        next: bool,
    },
    /// A book reader or editor button.
    Reader(ReaderButton),
    /// The screen's close (X) button, which closes it as the inventory key does.
    Close,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ReaderButton {
    Prev,
    Next,
    Done,
    Sign,
    Finalize,
    Cancel,
    /// The two-page spread's arrows.
    PrevSpread,
    NextSpread,
    /// Edits of the spread's left (`0`) or right (`1`) page; `EditPage`
    /// shows or hides that page's edit controls.
    EditPage(u8),
    InsertPage(u8),
    DeletePage(u8),
    SwapLeft(u8),
    SwapRight(u8),
    /// Typing goes to the spread's left (`0`) or right (`1`) page.
    FocusPage(u8),
}

pub const TAB_COUNT: u8 = 5;
/// The last tab is the search tab.
pub const SEARCH_TAB: u8 = TAB_COUNT - 1;
/// The inventory tab follows the five recipe tabs in the UI's index space.
pub const SURVIVAL_TAB: u8 = TAB_COUNT;

/// Converts the UI's category order into a protocol tab without guessing unknown values.
pub fn inventory_tab(tab: u8) -> Option<protocol::InventoryTab> {
    use protocol::InventoryTab;
    Some(match tab {
        0 => InventoryTab::Construction,
        1 => InventoryTab::Nature,
        2 => InventoryTab::Equipment,
        3 => InventoryTab::Items,
        SEARCH_TAB => InventoryTab::Search,
        SURVIVAL_TAB => InventoryTab::Survival,
        _ => return None,
    })
}

/// Finds the UI category index for a known wire tab; unfamiliar values stay unselected.
pub fn inventory_tab_ui(tab: protocol::InventoryTab) -> Option<u8> {
    (0..=SURVIVAL_TAB).find(|index| inventory_tab(*index) == Some(tab))
}
pub const STONECUTTER_CELLS: usize = 12;
/// The beacon level each selectable effect needs.
pub const BEACON_LEVEL_FOR: [(i32, u8); 6] = [(1, 1), (3, 1), (11, 2), (8, 2), (5, 3), (10, 4)];

/// Banner patterns the loom offers without a pattern item, as wire names
/// (provisional list, no vanilla evidence yet).
pub const LOOM_PATTERNS: [&str; 36] = [
    "bs", "ts", "ls", "rs", "cs", "ms", "drs", "dls", "ss", "cr", "sc", "ld", "rud", "lud", "rd",
    "vh", "vhr", "hh", "hhb", "bl", "br", "tl", "tr", "bt", "tt", "bts", "tts", "mc", "mr", "bo",
    "gra", "gru", "cbo", "bri", "flo", "cre",
];
