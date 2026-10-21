//! Player preferences synchronized separately for each furnace screen.

use bedrock_protocol_1_26_51::{enums, packets, types};

use super::{InventoryEvent, WindowKind};
use crate::Packet;

/// The furnace screen whose player preferences are being synchronized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FurnaceKind {
    Furnace,
    BlastFurnace,
    Smoker,
}

impl FurnaceKind {
    /// Identifies the preference owner for a furnace-like window.
    pub const fn from_window(kind: WindowKind) -> Option<Self> {
        match kind {
            WindowKind::Furnace => Some(Self::Furnace),
            WindowKind::BlastFurnace => Some(Self::BlastFurnace),
            WindowKind::Smoker => Some(Self::Smoker),
            _ => None,
        }
    }

    /// Whether a queued preference packet updates this furnace family's saved choices.
    pub fn owns_packet(self, packet: &Packet) -> bool {
        matches!(
            packet.data.as_ref(),
            packets::Packet::SetPlayerFurnaceOptions(options)
                if options.furnace_type == self.wire()
        )
    }

    /// Converts the owned screen kind to the generated packet discriminator.
    fn wire(self) -> enums::FurnaceType {
        match self {
            Self::Furnace => enums::FurnaceType::Furnace,
            Self::BlastFurnace => enums::FurnaceType::Blastfurnace,
            Self::Smoker => enums::FurnaceType::Smoker,
        }
    }
}

/// A furnace's selected left tab; unknown server values remain representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FurnaceTab {
    None,
    Food,
    Items,
    Blocks,
    Search,
    Inventory,
    Unknown(i32),
}

impl FurnaceTab {
    /// Admits a wire tab without discarding unfamiliar well-formed values.
    pub fn from_index(index: i32) -> Self {
        match enums::FurnaceLeftTabIndex::from(index) {
            enums::FurnaceLeftTabIndex::None => Self::None,
            enums::FurnaceLeftTabIndex::Recipefood => Self::Food,
            enums::FurnaceLeftTabIndex::Recipeitems => Self::Items,
            enums::FurnaceLeftTabIndex::Recipeblocks => Self::Blocks,
            enums::FurnaceLeftTabIndex::Recipesearch => Self::Search,
            enums::FurnaceLeftTabIndex::Inventory => Self::Inventory,
            enums::FurnaceLeftTabIndex::Unknown(index) => Self::Unknown(index),
        }
    }

    /// Returns the shared tab index used by the packet and furnace controls.
    pub fn index(self) -> i32 {
        let tab = match self {
            Self::None => enums::FurnaceLeftTabIndex::None,
            Self::Food => enums::FurnaceLeftTabIndex::Recipefood,
            Self::Items => enums::FurnaceLeftTabIndex::Recipeitems,
            Self::Blocks => enums::FurnaceLeftTabIndex::Recipeblocks,
            Self::Search => enums::FurnaceLeftTabIndex::Recipesearch,
            Self::Inventory => enums::FurnaceLeftTabIndex::Inventory,
            Self::Unknown(index) => enums::FurnaceLeftTabIndex::Unknown(index),
        };
        tab.to_raw()
    }
}

/// The player's furnace layout preference, including unrecognized server values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FurnaceLayout {
    None,
    InventoryOnly,
    RecipeBook,
    Unknown(i32),
}

impl FurnaceLayout {
    /// Admits a layout using the generated protocol's enum values.
    pub fn from_index(index: i32) -> Self {
        match enums::FurnaceLayout::from(index) {
            enums::FurnaceLayout::None => Self::None,
            enums::FurnaceLayout::Inventoryonly => Self::InventoryOnly,
            enums::FurnaceLayout::Default => Self::RecipeBook,
            enums::FurnaceLayout::Unknown(index) => Self::Unknown(index),
        }
    }

    /// Returns the shared layout index used by the packet and furnace controls.
    pub fn index(self) -> i32 {
        match self {
            Self::None => enums::FurnaceLayout::None,
            Self::InventoryOnly => enums::FurnaceLayout::Inventoryonly,
            Self::RecipeBook => enums::FurnaceLayout::Default,
            Self::Unknown(index) => enums::FurnaceLayout::Unknown(index),
        }
        .to_raw()
    }
}

/// One furnace screen's recipe tab, filter and layout preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FurnaceOptions {
    pub left_tab: FurnaceTab,
    pub filtering: bool,
    pub layout: FurnaceLayout,
}

/// A server update for one player's furnace, blast furnace or smoker preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FurnaceOptionsEvent {
    pub kind: FurnaceKind,
    pub options: FurnaceOptions,
}

/// Builds a preference update without changing any inventory stack state.
pub fn furnace_options_packet(kind: FurnaceKind, options: FurnaceOptions) -> Packet {
    Packet::new(packets::SetPlayerFurnaceOptions {
        furnace_type: kind.wire(),
        furnace_options: types::FurnaceOptions {
            left_furnace_tab: options.left_tab.index().into(),
            filtering: options.filtering,
            layout: options.layout.index().into(),
        },
    })
}

/// Routes supported furnace types while tolerating an unfamiliar screen discriminator.
pub(crate) fn normalize_furnace_options(
    packet: packets::SetPlayerFurnaceOptions,
) -> Option<InventoryEvent> {
    let kind = match packet.furnace_type {
        enums::FurnaceType::Furnace => FurnaceKind::Furnace,
        enums::FurnaceType::Blastfurnace => FurnaceKind::BlastFurnace,
        enums::FurnaceType::Smoker => FurnaceKind::Smoker,
        enums::FurnaceType::None | enums::FurnaceType::Unknown(_) => {
            tracing::warn!(target: "cinnabar::inventory_requests",
                furnace_type = packet.furnace_type.to_raw(),
                "skipped unsupported furnace preference type");
            return None;
        }
    };
    Some(InventoryEvent::FurnaceOptions(FurnaceOptionsEvent {
        kind,
        options: FurnaceOptions {
            left_tab: FurnaceTab::from_index(packet.furnace_options.left_furnace_tab.to_raw()),
            filtering: packet.furnace_options.filtering,
            layout: FurnaceLayout::from_index(packet.furnace_options.layout.to_raw()),
        },
    }))
}
