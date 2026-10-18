//! What the container screens read from the client: inventory authority, the open screen's
//! interaction state, and this frame's resolved icons and names.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use assets::entity::mount::MountEquipment;
use protocol::{NetworkItemStack, ScreenRecipe};
use ui::IconRef;

use crate::{furnace_book::Entries, recipe_book::BookEntry};

/// The predicted station result and its resolved item decorations.
#[derive(Clone, Debug)]
pub struct StationOutput {
    pub stack: NetworkItemStack,
    pub icon: Option<IconRef>,
    pub durability: Option<f32>,
}

/// The client's authority as the container screens read it; the client's adapter answers from
/// its runtime, so screens never hold it.
pub trait ContainerSource {
    fn player(&self) -> &player_state::PlayerState;
    /// The open screen's interaction state.
    fn screen(&self) -> &ScreenView<'_>;
    fn translation(&self, key: &str) -> Option<Arc<str>>;
    /// The local player's experience level; `0` before the server sends one.
    fn experience_level(&self) -> u32;
    /// An enchantment's localized name followed by its level.
    fn enchantment_name(&self, id: i16, level: u8) -> String;
    /// Whether the recipe book lists only recipes the inventory can supply.
    fn recipe_filtering(&self) -> bool;
    /// Changes whenever displayed item names change.
    fn item_name_generation(&self) -> u64;
    /// The recipe book's entries on the current tab, in list order.
    fn book_entries(&self) -> Vec<BookEntry<'_>>;
    /// The furnace recipes the current tab and search list.
    fn furnace_entries(&self) -> Entries<'_>;
    /// The positions [`Self::furnace_entries`] lists, shared until their inputs change.
    fn furnace_projection(&self) -> Arc<[usize]>;
    /// The furnace tab filing `recipe`: `1` food, `2` items, `3` blocks.
    fn furnace_category(&self, recipe: &ScreenRecipe) -> u8;
    /// The sources furnace rows name their items from.
    fn name_sources(&self) -> NameSources;
}

/// The open container screen's interaction state, borrowed from the client's screen state.
pub struct ScreenView<'a> {
    /// Admitted equipment facts of the open mount.
    pub mount_equipment: MountEquipment,
    pub anvil_name: &'a str,
    /// The predicted anvil cost, when no server output takes precedence.
    pub anvil_cost: Option<AnvilCost>,
    /// The stonecutter recipe the player picked.
    pub recipe_choice: Option<u32>,
    /// Chosen beacon effects; `0` means none.
    pub beacon: (i32, i32),
    /// The beacon's pyramid level, from its block entity.
    pub beacon_level: Option<u8>,
    pub loom_pattern: Option<&'a str>,
    /// Whether the crafter screen shows each slot disabled.
    pub crafter_disabled: [bool; 9],
    /// The crafter block's `triggered_bit`.
    pub crafter_powered: Option<bool>,
    pub furnace_book_open: bool,
    pub furnace_tab: Option<i32>,
    /// The Classic controller snapshot read by all inventory bindings.
    pub inventory: inventory::InventoryController,
    /// Creative catalog groups the player expanded, by group index.
    pub creative_expanded: &'a BTreeSet<u32>,
    pub search: &'a str,
    /// Scroll offsets of the screen's scroll views, by view key.
    pub container_scroll: &'a BTreeMap<String, f64>,
    /// Whether a text field owns the keyboard.
    pub text_focused: bool,
    /// The book, sign-off or lectern reader being shown.
    pub book: Option<BookView<'a>>,
}

/// What taking the open anvil's predicted result costs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnvilCost {
    /// Experience levels.
    pub levels: i32,
    /// Survival players cannot take it at any level count.
    pub too_expensive: bool,
    /// The player can take it now.
    pub affordable: bool,
}

/// A book reader or editor's state, borrowed from the client's book state.
pub struct BookView<'a> {
    pub pages: &'a [String],
    /// The page typing reaches.
    pub page: usize,
    /// The left page of the spread showing `page`.
    pub spread: usize,
    pub editable: bool,
    /// The title prompt shown before a book is signed.
    pub signing: bool,
    pub title: &'a str,
    pub author: &'a str,
    /// The page whose edit controls the screen shows.
    pub editing: Option<usize>,
}

/// This frame's resolved icons, durabilities and names, borrowed from the client's frame.
pub struct ContainerFrame<'a> {
    /// Tooltip names by `(network id, metadata)`.
    pub item_names: &'a HashMap<(i32, u32), Arc<str>>,
    pub inventory_icons: &'a [Option<IconRef>; 36],
    pub player_durability: &'a [Option<f32>; 36],
    pub hotbar_durability: &'a [Option<f32>; 9],
    pub storage_icons: &'a [Option<IconRef>],
    pub storage_durability: &'a [Option<f32>],
    /// UI-inventory slot icons and durabilities, by slot.
    pub ui_icons: &'a [Option<IconRef>],
    pub ui_durability: &'a [Option<f32>],
    pub crafting_icons: &'a [Option<IconRef>; 9],
    pub crafting_output: Option<&'a (Option<IconRef>, NetworkItemStack)>,
    pub armor_icons: &'a [Option<IconRef>; 4],
    pub armor_durability: &'a [Option<f32>; 4],
    pub offhand_icon: Option<IconRef>,
    pub offhand_durability: Option<f32>,
    /// The held stack's icon.
    pub cursor_icon: Option<IconRef>,
    /// Remaining durability of the presented cursor stack, including server corrections.
    pub cursor_durability: Option<f32>,
    /// Output icons of the stonecutter's recipe cells.
    pub recipe_icons: &'a [Option<IconRef>],
    /// The result a recipe screen would produce now.
    pub recipe_output: Option<&'a StationOutput>,
    /// Icons of the recipe book's entries, in list order.
    pub book_entries: &'a [Option<IconRef>],
    pub furnace_entries: &'a Arc<[Option<IconRef>]>,
}

/// The item-name sources rows were built under; two are the same while each source is the same
/// allocation.
#[derive(Clone, Default)]
pub struct NameSources {
    /// The session's item components.
    pub components: Option<Arc<dyn Any + Send + Sync>>,
    pub lang: Option<Arc<assets::RuntimeLangCatalog>>,
}

impl NameSources {
    pub fn same(&self, other: &Self) -> bool {
        fn same<T: ?Sized>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
            match (left, right) {
                (Some(left), Some(right)) => {
                    std::ptr::addr_eq(Arc::as_ptr(left), Arc::as_ptr(right))
                }
                (None, None) => true,
                _ => false,
            }
        }
        same(&self.components, &other.components) && same(&self.lang, &other.lang)
    }
}
