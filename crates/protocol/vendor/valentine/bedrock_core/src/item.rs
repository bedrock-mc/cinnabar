//! Core item traits for the Flyweight-Static Registry Pattern.
//!
//! These traits are defined here in bedrock_core.
//! The actual implementations (ZST items) are GENERATED
//! by valentine_gen into the version crates.

use std::fmt::Debug;

/// Trait for item definitions. Implemented by zero-sized marker types.
///
/// Each item (Stone, DiamondSword, etc.) is a ZST implementing this trait.
/// All data is const for zero-cost access.
pub trait ItemDef: 'static + Send + Sync + Sized {
    const ID: i32;
    /// Raw item string ID from protocol (e.g., "item.hopper" or "minecraft:hopper")
    const STRING_ID: &'static str;
    /// Display name (e.g., "Hopper", "Stone")
    const NAME: &'static str;
    const STACK_SIZE: u8;

    /// Legacy metadata value (0 for most items, used for variants)
    const METADATA: u32 = 0;
}

/// Extension trait for items with durability (tools, armor, etc.).
pub trait DurableItem: ItemDef {
    const MAX_DURABILITY: u16;
}

/// Extension trait for items that can be repaired with specific materials.
pub trait RepairableItem: DurableItem {
    /// Item IDs that can repair this item (e.g., diamond for diamond tools)
    fn repair_items() -> &'static [i32];
}

/// Extension trait for items that can be enchanted.
pub trait EnchantableItem: ItemDef {
    /// Enchantment categories this item accepts
    fn enchant_categories() -> &'static [EnchantmentCategory];
}

/// Extension trait for items with multiple variants (e.g., beds with different colors).
pub trait VariantItem: ItemDef {
    /// Item variants (different metadata values)
    fn variants() -> &'static [ItemVariant];
}

/// Enchantment categories for items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnchantmentCategory {
    Weapon,
    Sword,
    Axe,
    Pickaxe,
    Shovel,
    Hoe,
    HeadArmor,
    ChestArmor,
    LegsArmor,
    FeetArmor,
    Armor,
    Equippable,
    Bow,
    Crossbow,
    Trident,
    FishingRod,
    Shears,
    FlintAndSteel,
    Shield,
    Elytra,
    Durability,
    Vanishing,
    Mending,
}

impl EnchantmentCategory {
    /// Parse enchantment category from string (as found in JSON).
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "weapon" => Some(Self::Weapon),
            "sword" => Some(Self::Sword),
            "axe" => Some(Self::Axe),
            "pickaxe" => Some(Self::Pickaxe),
            "shovel" => Some(Self::Shovel),
            "hoe" => Some(Self::Hoe),
            "head_armor" => Some(Self::HeadArmor),
            "chest_armor" => Some(Self::ChestArmor),
            "legs_armor" => Some(Self::LegsArmor),
            "feet_armor" => Some(Self::FeetArmor),
            "armor" => Some(Self::Armor),
            "equippable" => Some(Self::Equippable),
            "bow" => Some(Self::Bow),
            "crossbow" => Some(Self::Crossbow),
            "trident" => Some(Self::Trident),
            "fishing_rod" => Some(Self::FishingRod),
            "shears" => Some(Self::Shears),
            "flint_and_steel" => Some(Self::FlintAndSteel),
            "shield" => Some(Self::Shield),
            "elytra" => Some(Self::Elytra),
            "durability" => Some(Self::Durability),
            "vanishing" => Some(Self::Vanishing),
            "mending" => Some(Self::Mending),
            _ => None,
        }
    }
}

/// Item variant data (for items with multiple metadata values).
#[derive(Debug, Clone, Copy)]
pub struct ItemVariant {
    pub id: i32,
    pub metadata: u32,
    pub name: &'static str,
    pub display_name: &'static str,
    pub stack_size: u8,
}

/// Object-safe version of ItemDef for dynamic dispatch.
///
/// Allows storing `&dyn ItemDefDyn` in arrays/vectors.
pub trait ItemDefDyn: Send + Sync {
    fn id(&self) -> i32;
    fn string_id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn stack_size(&self) -> u8;
    fn metadata(&self) -> u32;
}

impl<T: ItemDef> ItemDefDyn for T {
    fn id(&self) -> i32 {
        T::ID
    }
    fn string_id(&self) -> &'static str {
        T::STRING_ID
    }
    fn name(&self) -> &'static str {
        T::NAME
    }
    fn stack_size(&self) -> u8 {
        T::STACK_SIZE
    }
    fn metadata(&self) -> u32 {
        T::METADATA
    }
}
