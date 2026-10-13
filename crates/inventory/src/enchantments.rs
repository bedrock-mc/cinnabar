//! Enchantment facts by Bedrock id:
//! language key, rarity (frequency), item slots, level cap and exclusivity group.

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
    /// The suffix of its `enchantment.<key>` language entry.
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
    id: i16,
    lang_key: &'static str,
    frequency: u8,
    primary_slots: u32,
    secondary_slots: u32,
    max_level: i16,
    group: u8,
) -> Enchantment {
    Enchantment {
        id,
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
pub const ENCHANTMENTS: [Enchantment; 42] = [
    e(0, "protect.all", 30, ARMOR, 0, 4, 3),
    e(1, "protect.fire", 10, ARMOR, 0, 4, 3),
    e(2, "protect.fall", 10, FEET, 0, 4, 0),
    e(3, "protect.explosion", 3, ARMOR, 0, 4, 3),
    e(4, "protect.projectile", 10, ARMOR, 0, 4, 3),
    e(5, "thorns", 1, TORSO, HEAD | FEET | LEGS, 3, 0),
    e(6, "oxygen", 3, HEAD, 0, 3, 0),
    e(7, "waterWalker", 3, FEET, 0, 3, 4),
    e(8, "waterWorker", 3, HEAD, 0, 1, 0),
    e(9, "damage.all", 30, SWORD | SPEAR, AXE, 5, 1),
    e(10, "damage.undead", 10, SWORD | MACE | SPEAR, AXE, 5, 1),
    e(11, "damage.arthropods", 10, SWORD | MACE | SPEAR, AXE, 5, 1),
    e(12, "knockback", 10, SWORD | SPEAR, 0, 2, 0),
    e(13, "fire", 3, SWORD | MACE | SPEAR, 0, 2, 0),
    e(14, "lootBonus", 3, SWORD | SPEAR, 0, 3, 0),
    e(15, "digging", 30, DIGGERS, SHEARS, 5, 0),
    e(16, "untouching", 1, DIGGERS, SHEARS, 1, 2),
    e(
        17,
        "durability",
        10,
        DURABLE_PRIMARY,
        DURABLE_SECONDARY,
        3,
        0,
    ),
    e(18, "lootBonusDigger", 3, DIGGERS, 0, 3, 2),
    e(19, "arrowDamage", 30, BOW, 0, 5, 0),
    e(20, "arrowKnockback", 3, BOW, 0, 2, 0),
    e(21, "arrowFire", 3, BOW, 0, 1, 0),
    e(22, "arrowInfinite", 1, BOW, 0, 1, 5),
    e(23, "lootBonusFishing", 3, FISHING_ROD, 0, 3, 0),
    e(24, "fishingSpeed", 3, FISHING_ROD, 0, 3, 0),
    e(25, "frostwalker", 3, FEET, 0, 2, 4),
    e(26, "mending", 3, DURABLE_PRIMARY, DURABLE_SECONDARY, 1, 5),
    e(
        27,
        "curse.binding",
        1,
        ARMOR | ELYTRA | COSMETIC_HEAD,
        0,
        1,
        0,
    ),
    e(28, "curse.vanishing", 1, ALL, 0, 1, 0),
    e(29, "tridentImpaling", 10, TRIDENT, 0, 5, 0),
    e(30, "tridentRiptide", 3, TRIDENT, 0, 3, 6),
    e(31, "tridentLoyalty", 30, TRIDENT, 0, 3, 6),
    e(32, "tridentChanneling", 1, TRIDENT, 0, 1, 0),
    e(33, "crossbowMultishot", 3, CROSSBOW, 0, 1, 0),
    e(34, "crossbowPiercing", 30, CROSSBOW, 0, 4, 0),
    e(35, "crossbowQuickCharge", 10, CROSSBOW, 0, 3, 0),
    e(36, "soul_speed", 1, FEET, 0, 3, 0),
    e(37, "swift_sneak", 1, LEGS, 0, 3, 0),
    e(38, "heavy_weapon.windburst", 3, MACE, 0, 3, 0),
    e(39, "heavy_weapon.density", 10, MACE, 0, 5, 1),
    e(40, "heavy_weapon.breach", 3, MACE, 0, 4, 1),
    e(41, "lunge", 10, SPEAR, 0, 3, 0),
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
    !matches!((a.min(b), a.max(b)), (30, 32) | (33, 34))
}

/// The enchantment slot of the item `identifier`, or `0` when it takes no enchantments.
///
/// Provisional: derived from vanilla item names, not read from each item's native
/// registration.
#[must_use]
pub fn item_slot(identifier: &str) -> u32 {
    let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    let suffix = |end: &str| name.ends_with(end);
    match name {
        "enchanted_book" | "book" => ALL,
        "bow" => BOW,
        "shears" => SHEARS,
        "flint_and_steel" => FLINT_AND_STEEL,
        "fishing_rod" => FISHING_ROD,
        "carrot_on_a_stick" => CARROT_STICK,
        "warped_fungus_on_a_stick" => FUNGUS_STICK,
        "elytra" => ELYTRA,
        "trident" => TRIDENT,
        "crossbow" => CROSSBOW,
        "shield" => SHIELD,
        "brush" => BRUSH,
        "mace" => MACE,
        "compass" | "recovery_compass" | "lodestone_compass" => COMPASS,
        "carved_pumpkin" | "skull" => COSMETIC_HEAD,
        _ if suffix("_helmet") => HEAD,
        _ if suffix("_chestplate") => TORSO,
        _ if suffix("_leggings") => LEGS,
        _ if suffix("_boots") => FEET,
        _ if suffix("_sword") => SWORD,
        _ if suffix("_spear") => SPEAR,
        _ if suffix("_pickaxe") => PICKAXE,
        _ if suffix("_axe") => AXE,
        _ if suffix("_shovel") => SHOVEL,
        _ if suffix("_hoe") => HOE,
        _ if suffix("_head") => COSMETIC_HEAD,
        _ => 0,
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

    #[test]
    fn rows_sit_at_their_id() {
        for (index, enchantment) in ENCHANTMENTS.iter().enumerate() {
            assert_eq!(usize::try_from(enchantment.id).unwrap(), index);
        }
        assert!(enchantment(42).is_none() && enchantment(-1).is_none());
        assert!(
            ENCHANTMENTS
                .iter()
                .all(|enchantment| !enchantment.lang_key.is_empty()),
            "every enchantment names its language entry"
        );
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
            applies_to(5, item_slot("minecraft:diamond_helmet")),
            "thorns"
        );
        assert!(applies_to(9, item_slot("minecraft:iron_axe")), "sharpness");
        assert!(
            !applies_to(12, item_slot("minecraft:iron_axe")),
            "knockback"
        );
        assert!(applies_to(17, item_slot("minecraft:shield")), "unbreaking");
        assert!(!applies_to(0, item_slot("minecraft:stick")));
        assert!(applies_to(41, item_slot("minecraft:enchanted_book")));
    }
}
