//! Inventory preferences travel independently of stack transactions.

use bedrock_protocol_1_26_51::{enums, packets, types};

use super::InventoryEvent;
use crate::Packet;

/// Defines lossless domain enums using the generated wire discriminants.
macro_rules! option_enum {
    ($(#[$meta:meta])* $name:ident, $wire:ident; $($variant:ident => $generated:ident),+ $(,)?) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant,)+ Unknown(i32) }
        impl $name {
            /// Retains known and unfamiliar signed wire values.
            pub fn from_index(index: i32) -> Self {
                match enums::$wire::from(index) {
                    $(enums::$wire::$generated => Self::$variant,)+
                    enums::$wire::Unknown(index) => Self::Unknown(index),
                }
            }
            /// Returns the protocol and radio-control discriminator.
            pub fn index(self) -> i32 {
                match self {
                    $(Self::$variant => enums::$wire::$generated,)+
                    Self::Unknown(index) => enums::$wire::Unknown(index),
                }.to_raw()
            }
        }
    };
}

option_enum!(
    /// The selected left navigation tab, independent of the layout.
    InventoryTab, InventoryLeftTabIndex;
    None => None, Construction => Recipeconstruction, Equipment => Recipeequipment,
    Items => Recipeitems, Nature => Recipenature, Search => Recipesearch, Survival => Survival,
);
option_enum!(
    /// The selected right panel of the inventory.
    InventoryRightTab, InventoryRightTabIndex;
    None => None, Fullscreen => Fullscreen, Crafting => Crafting, Armor => Armor,
);
option_enum!(
    /// The layout retained for a personal inventory or crafting table.
    InventoryLayout, InventoryLayout;
    None => None, InventoryOnly => Inventoryonly, RecipeBook => Default,
    RecipeBookOnly => Recipebookonly,
);

/// Player preferences retained as one complete tuple, including the inactive layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventoryOptions {
    pub left_tab: InventoryTab,
    pub right_tab: InventoryRightTab,
    /// The saved toggle, before the creative-mode filter bypass.
    pub filtering: bool,
    pub inventory_layout: InventoryLayout,
    pub crafting_layout: InventoryLayout,
}

impl Default for InventoryOptions {
    fn default() -> Self {
        Self {
            left_tab: InventoryTab::Search,
            right_tab: InventoryRightTab::Crafting,
            filtering: false,
            inventory_layout: InventoryLayout::None,
            crafting_layout: InventoryLayout::None,
        }
    }
}

/// Builds a complete preference update without altering inventory stacks.
pub fn inventory_options_packet(options: InventoryOptions) -> Packet {
    Packet::new(packets::SetPlayerInventoryOptions {
        inventory_options: types::InventoryOptions {
            left_inventory_tab: options.left_tab.index().into(),
            right_inventory_tab: options.right_tab.index().into(),
            filtering: options.filtering,
            layout_inv: options.inventory_layout.index().into(),
            layout_craft: options.crafting_layout.index().into(),
        },
    })
}

/// Admits the full server tuple without applying screen-specific defaults.
pub(crate) fn normalize_inventory_options(
    packet: packets::SetPlayerInventoryOptions,
) -> InventoryEvent {
    let options = packet.inventory_options;
    InventoryEvent::Options(InventoryOptions {
        left_tab: InventoryTab::from_index(options.left_inventory_tab.to_raw()),
        right_tab: InventoryRightTab::from_index(options.right_inventory_tab.to_raw()),
        filtering: options.filtering,
        inventory_layout: InventoryLayout::from_index(options.layout_inv.to_raw()),
        crafting_layout: InventoryLayout::from_index(options.layout_craft.to_raw()),
    })
}
