//! Enchantment facts by Bedrock id:
//! language key, rarity (frequency), item slots, level cap and exclusivity group.

use assets::{
    facts::{ItemFact, ItemTraits},
    item::armour::ItemArmourSlot,
};
use protocol_ids::Enchantment as EnchantmentId;

/// Item slot bits used for enchantment applicability.
pub mod slot {
    pub const HEAD: u32 = 0x1;
    pub const TORSO: u32 = 0x2;
    pub const FEET: u32 = 0x4;
    pub const LEGS: u32 = 0x8;
    pub const SWORD: u32 = 0x10;
    pub const BOW: u32 = 0x20;
    pub const HOE: u32 = 0x40;
    pub const SHEARS: u32 = 0x80;
    pub const FLINT_AND_STEEL: u32 = 0x100;
    pub const AXE: u32 = 0x200;
    pub const PICKAXE: u32 = 0x400;
    pub const SHOVEL: u32 = 0x800;
    pub const FISHING_ROD: u32 = 0x1000;
    pub const CARROT_STICK: u32 = 0x2000;
    pub const ELYTRA: u32 = 0x4000;
    pub const TRIDENT: u32 = 0x8000;
    pub const CROSSBOW: u32 = 0x10000;
    pub const SHIELD: u32 = 0x20000;
    pub const COSMETIC_HEAD: u32 = 0x40000;
    pub const COMPASS: u32 = 0x80000;
    pub const FUNGUS_STICK: u32 = 0x100000;
    pub const BRUSH: u32 = 0x200000;
    pub const MACE: u32 = 0x400000;
    pub const SPEAR: u32 = 0x800000;
    /// Every slot, which enchanted books and Curse of Vanishing use.
    pub const ALL: u32 = u32::MAX;
}

/// One enchantment's facts.
#[derive(Clone, Copy, Debug)]
pub struct Enchantment {
    pub id: i16,
    /// The complete language key used to display this enchantment.
    pub lang_key: &'static str,
    /// Rarity frequency: 30 common, 10 uncommon, 3 rare, 1 very rare.
    pub frequency: u8,
    /// Item slots it applies to directly.
    pub primary_slots: u32,
    /// Item slots it reaches only through an anvil or a command.
    pub secondary_slots: u32,
    pub max_level: i16,
    /// Enchantments sharing a nonzero group exclude each other.
    group: u8,
}

impl Enchantment {
    /// The anvil cost per level, from its rarity; books cost half, at least 1.
    #[must_use]
    pub const fn anvil_multiplier(&self, from_book: bool) -> i32 {
        let item = match self.frequency {
            30.. => 1,
            10.. => 2,
            3.. => 4,
            _ => 8,
        };
        if from_book && item > 1 {
            item / 2
        } else {
            item
        }
    }
}

/// Builds one registry row.
const fn e(
    id: EnchantmentId,
    lang_key: &'static str,
    frequency: u8,
    primary_slots: u32,
    secondary_slots: u32,
    max_level: i16,
    group: u8,
) -> Enchantment {
    Enchantment {
        id: id.wire_value() as i16,
        lang_key,
        frequency,
        primary_slots,
        secondary_slots,
        max_level,
        group,
    }
}

use slot::*;

const ARMOR: u32 = HEAD | TORSO | FEET | LEGS;
const DIGGERS: u32 = HOE | AXE | PICKAXE | SHOVEL;
const DURABLE_PRIMARY: u32 = 0x81_1E7F;
const DURABLE_SECONDARY: u32 = 0x72_E1C0;

/// Every enchantment, indexed by id.
pub const ENCHANTMENTS: [Enchantment; EnchantmentId::Numenchantments.wire_value() as usize] = [
    e(
        EnchantmentId::Protection,
        "enchantment.protect.all",
        30,
        ARMOR,
        0,
        4,
        3,
    ),
    e(
        EnchantmentId::Fireprotection,
        "enchantment.protect.fire",
        10,
        ARMOR,
        0,
        4,
        3,
    ),
    e(
        EnchantmentId::Featherfalling,
        "enchantment.protect.fall",
        10,
        FEET,
        0,
        4,
        0,
    ),
    e(
        EnchantmentId::Blastprotection,
        "enchantment.protect.explosion",
        3,
        ARMOR,
        0,
        4,
        3,
    ),
    e(
        EnchantmentId::Projectileprotection,
        "enchantment.protect.projectile",
        10,
        ARMOR,
        0,
        4,
        3,
    ),
    e(
        EnchantmentId::Thorns,
        "enchantment.thorns",
        1,
        TORSO,
        HEAD | FEET | LEGS,
        3,
        0,
    ),
    e(
        EnchantmentId::Respiration,
        "enchantment.oxygen",
        3,
        HEAD,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Depthstrider,
        "enchantment.waterWalker",
        3,
        FEET,
        0,
        3,
        4,
    ),
    e(
        EnchantmentId::Aquaaffinity,
        "enchantment.waterWorker",
        3,
        HEAD,
        0,
        1,
        0,
    ),
    e(
        EnchantmentId::Sharpness,
        "enchantment.damage.all",
        30,
        SWORD | SPEAR,
        AXE,
        5,
        1,
    ),
    e(
        EnchantmentId::Smite,
        "enchantment.damage.undead",
        10,
        SWORD | MACE | SPEAR,
        AXE,
        5,
        1,
    ),
    e(
        EnchantmentId::Baneofarthropods,
        "enchantment.damage.arthropods",
        10,
        SWORD | MACE | SPEAR,
        AXE,
        5,
        1,
    ),
    e(
        EnchantmentId::Knockback,
        "enchantment.knockback",
        10,
        SWORD | SPEAR,
        0,
        2,
        0,
    ),
    e(
        EnchantmentId::Fireaspect,
        "enchantment.fire",
        3,
        SWORD | MACE | SPEAR,
        0,
        2,
        0,
    ),
    e(
        EnchantmentId::Looting,
        "enchantment.lootBonus",
        3,
        SWORD | SPEAR,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Efficiency,
        "enchantment.digging",
        30,
        DIGGERS,
        SHEARS,
        5,
        0,
    ),
    e(
        EnchantmentId::Silktouch,
        "enchantment.untouching",
        1,
        DIGGERS,
        SHEARS,
        1,
        2,
    ),
    e(
        EnchantmentId::Unbreaking,
        "enchantment.durability",
        10,
        DURABLE_PRIMARY,
        DURABLE_SECONDARY,
        3,
        0,
    ),
    e(
        EnchantmentId::Fortune,
        "enchantment.lootBonusDigger",
        3,
        DIGGERS,
        0,
        3,
        2,
    ),
    e(
        EnchantmentId::Power,
        "enchantment.arrowDamage",
        30,
        BOW,
        0,
        5,
        0,
    ),
    e(
        EnchantmentId::Punch,
        "enchantment.arrowKnockback",
        3,
        BOW,
        0,
        2,
        0,
    ),
    e(
        EnchantmentId::Flame,
        "enchantment.arrowFire",
        3,
        BOW,
        0,
        1,
        0,
    ),
    e(
        EnchantmentId::Infinity,
        "enchantment.arrowInfinite",
        1,
        BOW,
        0,
        1,
        5,
    ),
    e(
        EnchantmentId::Luckofthesea,
        "enchantment.lootBonusFishing",
        3,
        FISHING_ROD,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Lure,
        "enchantment.fishingSpeed",
        3,
        FISHING_ROD,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Frostwalker,
        "enchantment.frostwalker",
        3,
        FEET,
        0,
        2,
        4,
    ),
    e(
        EnchantmentId::Mending,
        "enchantment.mending",
        3,
        DURABLE_PRIMARY,
        DURABLE_SECONDARY,
        1,
        5,
    ),
    e(
        EnchantmentId::Curseofbinding,
        "enchantment.curse.binding",
        1,
        ARMOR | ELYTRA | COSMETIC_HEAD,
        0,
        1,
        0,
    ),
    e(
        EnchantmentId::Curseofvanishing,
        "enchantment.curse.vanishing",
        1,
        ALL,
        0,
        1,
        0,
    ),
    e(
        EnchantmentId::Impaling,
        "enchantment.tridentImpaling",
        10,
        TRIDENT,
        0,
        5,
        0,
    ),
    e(
        EnchantmentId::Riptide,
        "enchantment.tridentRiptide",
        3,
        TRIDENT,
        0,
        3,
        6,
    ),
    e(
        EnchantmentId::Loyalty,
        "enchantment.tridentLoyalty",
        30,
        TRIDENT,
        0,
        3,
        6,
    ),
    e(
        EnchantmentId::Channeling,
        "enchantment.tridentChanneling",
        1,
        TRIDENT,
        0,
        1,
        0,
    ),
    e(
        EnchantmentId::Multishot,
        "enchantment.crossbowMultishot",
        3,
        CROSSBOW,
        0,
        1,
        0,
    ),
    e(
        EnchantmentId::Piercing,
        "enchantment.crossbowPiercing",
        30,
        CROSSBOW,
        0,
        4,
        0,
    ),
    e(
        EnchantmentId::Quickcharge,
        "enchantment.crossbowQuickCharge",
        10,
        CROSSBOW,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Soulspeed,
        "enchantment.soul_speed",
        1,
        FEET,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Swiftsneak,
        "enchantment.swift_sneak",
        1,
        LEGS,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Windburst,
        "enchantment.heavy_weapon.windburst",
        3,
        MACE,
        0,
        3,
        0,
    ),
    e(
        EnchantmentId::Density,
        "enchantment.heavy_weapon.density",
        10,
        MACE,
        0,
        5,
        1,
    ),
    e(
        EnchantmentId::Breach,
        "enchantment.heavy_weapon.breach",
        3,
        MACE,
        0,
        4,
        1,
    ),
    e(
        EnchantmentId::Lunge,
        "enchantment.lunge",
        10,
        SPEAR,
        0,
        3,
        0,
    ),
];

/// The facts for enchantment `id`.
#[must_use]
pub fn enchantment(id: i16) -> Option<&'static Enchantment> {
    usize::try_from(id)
        .ok()
        .and_then(|index| ENCHANTMENTS.get(index))
}

/// Whether two different enchantments may sit on one item: they share no exclusivity group,
/// and none of the native special cases applies (riptide excludes channeling, and multishot
/// excludes piercing).
#[must_use]
pub fn compatible(a: i16, b: i16) -> bool {
    if a == b {
        return true;
    }
    let (Some(first), Some(second)) = (enchantment(a), enchantment(b)) else {
        return true;
    };
    if first.group != 0 && first.group == second.group {
        return false;
    }
    !matches!(
        (
            u8::try_from(a).ok().and_then(EnchantmentId::from_wire),
            u8::try_from(b).ok().and_then(EnchantmentId::from_wire)
        ),
        (
            Some(EnchantmentId::Riptide),
            Some(EnchantmentId::Channeling)
        ) | (
            Some(EnchantmentId::Channeling),
            Some(EnchantmentId::Riptide)
        ) | (
            Some(EnchantmentId::Multishot),
            Some(EnchantmentId::Piercing)
        ) | (
            Some(EnchantmentId::Piercing),
            Some(EnchantmentId::Multishot)
        )
    )
}

/// Special item classes take precedence over general tool and armor tags.
const ITEM_SLOTS: &[(ItemTraits, u32)] = &[
    (ItemTraits::BOOK, ALL),
    (ItemTraits::ENCHANTED_BOOK, ALL),
    (ItemTraits::COSMETIC_HEAD, COSMETIC_HEAD),
    (ItemTraits::GLIDER, ELYTRA),
    (ItemTraits::MACE, MACE),
    (ItemTraits::BOW, BOW),
    (ItemTraits::SHEARS, SHEARS),
    (ItemTraits::FLINT_AND_STEEL, FLINT_AND_STEEL),
    (ItemTraits::FISHING_ROD, FISHING_ROD),
    (ItemTraits::CARROT_STICK, CARROT_STICK),
    (ItemTraits::FUNGUS_STICK, FUNGUS_STICK),
    (ItemTraits::TRIDENT, TRIDENT),
    (ItemTraits::CROSSBOW, CROSSBOW),
    (ItemTraits::SHIELD, SHIELD),
    (ItemTraits::BRUSH, BRUSH),
    (ItemTraits::COMPASS, COMPASS),
    (ItemTraits::SWORD, SWORD),
    (ItemTraits::SPEAR, SPEAR),
    (ItemTraits::PICKAXE, PICKAXE),
    (ItemTraits::AXE, AXE),
    (ItemTraits::SHOVEL, SHOVEL),
    (ItemTraits::HOE, HOE),
];

// New traits must explicitly choose an enchantment slot or stay outside this domain.
const _: () = {
    let mut covered =
        ItemTraits::POWDER_SNOW_WALKER.0 | ItemTraits::CREATIVE_NO_BREAK.0 | ItemTraits::SPYGLASS.0;
    let mut index = 0;
    while index < ITEM_SLOTS.len() {
        covered |= ITEM_SLOTS[index].0.0;
        index += 1;
    }
    assert!(covered == ItemTraits::ALL.0);
};

/// Resolves enchantment applicability from admitted item classes and wearable slots.
#[must_use]
pub fn item_slot(fact: &ItemFact) -> u32 {
    if let Some((_, slot)) = ITEM_SLOTS
        .iter()
        .find(|(trait_, _)| fact.traits.contains(*trait_))
    {
        return *slot;
    }
    match fact.armour_slot {
        Some(ItemArmourSlot::Head) => HEAD,
        Some(ItemArmourSlot::Chest) => TORSO,
        Some(ItemArmourSlot::Legs) => LEGS,
        Some(ItemArmourSlot::Feet) => FEET,
        None => 0,
    }
}

/// Whether enchantment `id` can go on an item in `item_slot` through an anvil: the slot meets
/// its primary or secondary slots.
#[must_use]
pub fn applies_to(id: i16, item_slot: u32) -> bool {
    enchantment(id).is_some_and(|enchantment| {
        item_slot & (enchantment.primary_slots | enchantment.secondary_slots) != 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a real pinned record for applicability fixtures.
    fn facts(identifier: &str) -> &'static ItemFact {
        assets::facts::ItemFacts::builtin()
            .unwrap()
            .by_identifier(identifier)
            .unwrap()
    }

    #[test]
    fn rows_sit_at_their_id() {
        for (index, enchantment) in ENCHANTMENTS.iter().enumerate() {
            assert_eq!(usize::try_from(enchantment.id).unwrap(), index);
        }
        for id in EnchantmentId::ALL {
            if id.wire_value() < EnchantmentId::Numenchantments.wire_value() {
                assert_eq!(
                    enchantment(i16::from(id.wire_value())).unwrap().id,
                    i16::from(id.wire_value())
                );
            } else {
                assert!(enchantment(i16::from(id.wire_value())).is_none());
            }
        }
        assert!(enchantment(-1).is_none());
        assert!(
            ENCHANTMENTS
                .iter()
                .all(|enchantment| !enchantment.lang_key.is_empty()),
            "every enchantment names its language entry"
        );
    }

    #[test]
    fn every_enchantment_name_exists_in_the_pinned_language() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(assets::vanilla_source().resource_pack_dir())
            .join("texts/en_US.lang");
        if !path.is_file() {
            eprintln!(
                "skipping every_enchantment_name_exists_in_the_pinned_language: missing en_US.lang fixture at {}",
                path.display()
            );
            return;
        }
        let text = std::fs::read_to_string(path).unwrap();
        let keys: std::collections::BTreeSet<_> = text
            .lines()
            .filter_map(|line| line.split_once('=').map(|(key, _)| key))
            .collect();
        for enchantment in &ENCHANTMENTS {
            assert!(
                keys.contains(enchantment.lang_key),
                "enchantment {} names missing language key {}",
                enchantment.id,
                enchantment.lang_key
            );
        }
    }

    #[test]
    fn registered_skulls_support_cosmetic_head_curses() {
        let registry = protocol::vanilla_item_registry();
        let mut skulls = 0;
        for entry in registry
            .iter()
            .filter(|entry| assets::vanilla_skull_type(&entry.identifier).is_some())
        {
            skulls += 1;
            let slot = item_slot(facts(&entry.identifier));
            assert_eq!(slot, slot::COSMETIC_HEAD, "{}", entry.identifier);
            for curse in ENCHANTMENTS.iter().filter(|facts| {
                matches!(
                    facts.lang_key,
                    "enchantment.curse.binding" | "enchantment.curse.vanishing"
                )
            }) {
                assert!(
                    applies_to(curse.id, slot),
                    "{} {}",
                    entry.identifier,
                    curse.lang_key
                );
            }
        }
        assert!(skulls > 0, "the pinned registry contains skull items");
    }

    #[test]
    fn rarity_sets_the_anvil_multiplier_and_books_halve_it() {
        let sharpness = enchantment(9).unwrap();
        assert_eq!(sharpness.anvil_multiplier(false), 1);
        assert_eq!(sharpness.anvil_multiplier(true), 1);
        let thorns = enchantment(5).unwrap();
        assert_eq!(thorns.anvil_multiplier(false), 8);
        assert_eq!(thorns.anvil_multiplier(true), 4);
        let impaling = enchantment(29).unwrap();
        assert_eq!(impaling.anvil_multiplier(false), 2);
        assert_eq!(impaling.anvil_multiplier(true), 1);
    }

    #[test]
    fn exclusivity_follows_native_groups_and_special_cases() {
        assert!(!compatible(9, 10), "sharpness and smite");
        assert!(!compatible(9, 40), "sharpness and breach");
        assert!(!compatible(16, 18), "silk touch and fortune");
        assert!(!compatible(22, 26), "infinity and mending");
        assert!(!compatible(30, 31), "riptide and loyalty");
        assert!(!compatible(30, 32), "riptide and channeling");
        assert!(compatible(31, 32), "loyalty and channeling");
        assert!(!compatible(33, 34), "multishot and piercing");
        assert!(compatible(0, 2), "protection and feather falling");
    }

    #[test]
    fn slots_include_secondary_reach() {
        assert!(
            applies_to(5, item_slot(facts("minecraft:diamond_helmet"))),
            "thorns"
        );
        assert!(
            applies_to(9, item_slot(facts("minecraft:iron_axe"))),
            "sharpness"
        );
        assert!(
            !applies_to(12, item_slot(facts("minecraft:iron_axe"))),
            "knockback"
        );
        assert!(
            applies_to(17, item_slot(facts("minecraft:shield"))),
            "unbreaking"
        );
        assert!(!applies_to(0, item_slot(facts("minecraft:stick"))));
        assert!(applies_to(41, item_slot(facts("minecraft:enchanted_book"))));
    }
}
