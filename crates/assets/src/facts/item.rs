/// Canonical identity used to request the totem's presentation assets.
pub const TOTEM_IDENTIFIER: &str = "minecraft:totem_of_undying";
/// Native filled-map identity used for session admission outside the retail census.
pub const FILLED_MAP_IDENTIFIER: &str = "minecraft:filled_map";

use crate::{
    item::{
        armour::{ItemArmour, ItemArmourSlot},
        durability::ItemDurability,
        fuel::ItemFuel,
        glint::ItemGlint,
        grip::ItemGrip,
    },
    item_use::ItemUseProfile,
};
use serde::{Deserialize, Serialize};

macro_rules! flags {
    ($name:ident, $($flag:ident = $bit:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
        impl $name {
            $(pub const $flag: Self = Self(1 << $bit);)+
            pub const ALL: Self = Self(0 $(| Self::$flag.0)+);
            /// Tests a compiled flag without examining an identifier.
            pub const fn contains(self, flag: Self) -> bool { self.0 & flag.0 == flag.0 }
            /// Combines independently admitted flags.
            pub fn insert(&mut self, flag: Self) { self.0 |= flag.0; }
        }
    };
}

flags!(
    ItemTraits,
    GLIDER = 0,
    POWDER_SNOW_WALKER = 1,
    CREATIVE_NO_BREAK = 2,
    COSMETIC_HEAD = 3,
    BOOK = 4,
    ENCHANTED_BOOK = 5,
    SWORD = 6,
    AXE = 7,
    PICKAXE = 8,
    SHOVEL = 9,
    HOE = 10,
    SHEARS = 11,
    SPEAR = 12,
    TRIDENT = 13,
    BOW = 14,
    CROSSBOW = 15,
    SHIELD = 16,
    FISHING_ROD = 17,
    FLINT_AND_STEEL = 18,
    CARROT_STICK = 19,
    FUNGUS_STICK = 20,
    COMPASS = 21,
    BRUSH = 22,
    MACE = 23,
    SPYGLASS = 24,
);
flags!(
    ContainerRoles,
    BREWING_FUEL = 0,
    BREWING_BOTTLE = 1,
    LOOM_BANNER = 2,
    LOOM_DYE = 3,
    LOOM_PATTERN = 4,
    SMITHING_INPUT = 5,
    SMITHING_MATERIAL = 6,
    SMITHING_TEMPLATE = 7,
    HORSE_SADDLE = 8,
    HORSE_ARMOUR = 9,
    HORSE_DECOR = 10,
    BEACON_PAYMENT = 11,
    CARTOGRAPHY_MAP = 12,
    ENCHANTING_LAPIS = 13,
);

/// A tag index in this carrier's sorted, version-bound vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemTag(pub(super) u8);

/// All pinned tag memberships in one numeric mask.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ItemTagSet(pub u128);
impl ItemTagSet {
    /// Tests membership using a tag admitted by this carrier.
    pub const fn contains(self, tag: ItemTag) -> bool {
        self.0 & (1 << tag.0) != 0
    }
    /// Adds a tag during compilation or session admission.
    pub fn insert(&mut self, tag: ItemTag) {
        self.0 |= 1 << tag.0;
    }
}

/// A declared use animation, resolved once from server component text.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemUseAnimationFact {
    None,
    Eat,
    Drink,
    Block,
    Bow,
    Camera,
    Spear,
    Crossbow,
    Spyglass,
    Brush,
}

impl ItemUseAnimationFact {
    /// Resolves supported animation contract values; unknown strings stay unavailable.
    pub fn from_component(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "eat" => Self::Eat,
            "drink" => Self::Drink,
            "block" => Self::Block,
            "bow" => Self::Bow,
            "camera" => Self::Camera,
            "spear" => Self::Spear,
            "crossbow" => Self::Crossbow,
            "spyglass" => Self::Spyglass,
            "brush" => Self::Brush,
            _ => return None,
        })
    }
}

/// Tool material admitted independently from the item's family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemTier {
    Wood,
    Stone,
    Copper,
    Iron,
    Gold,
    Diamond,
    Netherite,
}

/// Native armour texture materials that are absent from tool-tier tags.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmourMaterial {
    Leather,
    Iron,
    Gold,
    Diamond,
    Copper,
    Netherite,
}

impl ArmourMaterial {
    /// Maps supported tool-tier materials to their armour texture domain.
    const fn from_tier(tier: ItemTier) -> Option<Self> {
        Some(match tier {
            ItemTier::Wood | ItemTier::Stone => return None,
            ItemTier::Copper => Self::Copper,
            ItemTier::Iron => Self::Iron,
            ItemTier::Gold => Self::Gold,
            ItemTier::Diamond => Self::Diamond,
            ItemTier::Netherite => Self::Netherite,
        })
    }

    /// Gives the native armour arrays one material-to-index mapping.
    const fn texture_index(self) -> u8 {
        match self {
            Self::Leather => 1,
            Self::Iron => 2,
            Self::Gold => 3,
            Self::Diamond => 4,
            Self::Copper => 5,
            Self::Netherite => 6,
        }
    }
}

/// An exact material identity or a pinned material tag used for anvil repair.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemRepairMaterial {
    Item(i32),
    Tag(u8),
}

/// Small item facts copied into the negotiated numeric registry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFact {
    pub traits: ItemTraits,
    pub roles: ContainerRoles,
    /// Legacy UI renderer key for a pinned beacon payment icon.
    pub beacon_payment_key: Option<i64>,
    /// Rules still awaiting complete behavior evidence stay marked provisional.
    pub provisional_traits: ItemTraits,
    pub provisional_roles: ContainerRoles,
    pub tags: ItemTagSet,
    pub armour: ItemArmour,
    /// Equipment admission can be known even when protection is unresolved.
    pub armour_slot: Option<ItemArmourSlot>,
    pub tier: Option<ItemTier>,
    #[serde(default)]
    pub armour_material: Option<ArmourMaterial>,
    pub repair: Option<ItemRepairMaterial>,
    pub repair_provisional: bool,
    pub durability: ItemDurability,
    pub fuel: ItemFuel,
    pub grip: ItemGrip,
    pub glint: ItemGlint,
    pub max_stack_size: Option<u8>,
    pub allow_off_hand: Option<bool>,
    pub offhand_provisional: bool,
    pub use_profile: Option<u16>,
    pub use_animation: Option<ItemUseAnimationFact>,
    pub use_duration_ticks: Option<u32>,
}
impl ItemFact {
    /// Reads the shared leather tag without guessing a material from its identifier.
    pub fn is_leather(&self) -> bool {
        static TAG: std::sync::OnceLock<ItemTag> = std::sync::OnceLock::new();
        self.armour_material == Some(ArmourMaterial::Leather)
            || self.tags.contains(*TAG.get_or_init(|| {
                super::ItemFacts::builtin()
                    .expect("valid pinned item facts")
                    .tag("minecraft:leather_tier")
                    .expect("pinned leather tag")
            }))
    }

    /// Preserves the native armour texture order; unresolved materials use slot zero.
    pub fn armor_texture_index(&self) -> u8 {
        let material = if self.is_leather() {
            Some(ArmourMaterial::Leather)
        } else {
            self.armour_material
                .or_else(|| self.tier.and_then(ArmourMaterial::from_tier))
        };
        material.map_or(0, ArmourMaterial::texture_index)
    }

    /// Tests an exact retail material or an admitted repair tag without parsing names.
    pub fn repairs_with(&self, material: &Self, retail_id: Option<i32>) -> bool {
        match self.repair {
            Some(ItemRepairMaterial::Item(id)) => retail_id == Some(id),
            Some(ItemRepairMaterial::Tag(index)) => material.tags.contains(ItemTag(index)),
            None => false,
        }
    }
}

impl Default for ItemFact {
    /// Unknown facts grant no equipment or predicted behavior.
    fn default() -> Self {
        Self {
            traits: ItemTraits::default(),
            roles: ContainerRoles::default(),
            beacon_payment_key: None,
            provisional_traits: ItemTraits::default(),
            provisional_roles: ContainerRoles::default(),
            tags: ItemTagSet::default(),
            armour: ItemArmour::Unresolved,
            armour_slot: None,
            tier: None,
            armour_material: None,
            repair: None,
            repair_provisional: false,
            durability: ItemDurability::Unresolved,
            fuel: ItemFuel::Unresolved,
            grip: ItemGrip::Unresolved,
            glint: ItemGlint::Unresolved,
            max_stack_size: None,
            allow_off_hand: None,
            offhand_provisional: false,
            use_profile: None,
            use_animation: None,
            use_duration_ticks: None,
        }
    }
}

/// One independently pinned item identity and its compiled facts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFactRecord {
    pub network_id: i32,
    pub identifier: Box<str>,
    pub fact: ItemFact,
}

/// Facts contributed by a declared tag, including server-defined items.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemTagDefinition {
    pub name: Box<str>,
    pub traits: ItemTraits,
    pub roles: ContainerRoles,
    pub provisional_traits: ItemTraits,
    pub provisional_roles: ContainerRoles,
    pub tier: Option<ItemTier>,
    pub repair: Option<ItemRepairMaterial>,
    pub repair_provisional: bool,
}

/// The deterministic payload shared by the compiler and bounded runtime decoder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFactsPayload {
    pub protocol: u32,
    pub retail_sha256: [u8; 32],
    pub tags: Box<[ItemTagDefinition]>,
    pub profiles: Box<[ItemUseProfile]>,
    pub records: Box<[ItemFactRecord]>,
}
