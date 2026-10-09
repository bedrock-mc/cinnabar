//! Protocol 5's player inventory and network scope, which a callback's snapshot may carry, and
//! the stacks a guest stages.

use serde::{Deserialize, Serialize};

use super::BlockPos;

/// An item stack; `data`, lowercase hex, is only on the Experience's own items, and `plain` is
/// set when the stack carries nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemStack {
    pub id: String,
    pub metadata: u16,
    pub count: u8,
    pub max_count: u8,
    pub data: Option<String>,
    pub plain: bool,
}

/// A stack the guest puts somewhere; `data` is lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewStack {
    pub id: String,
    pub metadata: u16,
    pub count: u8,
    pub data: Option<String>,
}

/// Slots of an inventory: 0 to 8 the hotbar, 9 to 35 the rest of the main inventory, 36 the
/// offhand.
pub const INVENTORY_SLOTS: usize = 37;
/// Slots of the hotbar, from which the selected slot is.
pub const HOTBAR_SLOTS: u8 = 9;

/// A registered variant with its stack limit, offhand eligibility and plain classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerItem {
    pub id: String,
    pub metadata: u16,
    pub max_count: u8,
    pub off_hand: bool,
    pub plain: bool,
}

/// The actor's inventory: slots 0 to 8 the hotbar, 9 to 35 the rest of the main inventory, 36
/// the offhand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub selected: u8,
    pub slots: Vec<Option<ItemStack>>,
}

/// The network around a callback's anchor: its loaded members, which the snapshot holds, and
/// whether its bounds cut it short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub blocks: Vec<BlockPos>,
    pub truncated: bool,
}
