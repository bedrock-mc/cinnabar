//! Pinned vanilla item presentation facts: armor points, maximum durability,
//! and mechanical display names.
//!
//! The vanilla client derives the armor bar and durability fractions from the
//! item identity; the identity itself is authoritative (the server's own item
//! registry maps network ids to identifiers). Values below are the vanilla
//! Bedrock item stats cross-checked against PocketMine-MP and Dragonfly's
//! item definitions; an identifier outside the table simply contributes
//! nothing rather than guessing. Custom component-based items declare their
//! own stats server-side and are intentionally not modeled here.

use protocol::{NetworkItemStack, item_stack_damage};

/// Armor points for one equipped vanilla armor identifier.
#[must_use]
pub(crate) fn armor_points(identifier: &str) -> u16 {
    match identifier.strip_prefix("minecraft:").unwrap_or(identifier) {
        "leather_helmet" | "golden_boots" | "chainmail_boots" => 1,
        "leather_boots" => 1,
        "leather_leggings" => 2,
        "leather_chestplate" => 3,
        "golden_helmet" | "chainmail_helmet" | "iron_helmet" | "iron_boots" | "turtle_helmet" => 2,
        "golden_leggings" => 3,
        "chainmail_leggings" => 4,
        "golden_chestplate" | "chainmail_chestplate" => 5,
        "iron_leggings" => 5,
        "iron_chestplate" => 6,
        "diamond_helmet" | "netherite_helmet" | "diamond_boots" | "netherite_boots" => 3,
        "diamond_leggings" | "netherite_leggings" => 6,
        "diamond_chestplate" | "netherite_chestplate" => 8,
        _ => 0,
    }
}

/// Total armor points across the local player's equipped armor identifiers,
/// clamped to the reference 20-point bar.
#[must_use]
pub(crate) fn total_armor_points<'a>(identifiers: impl Iterator<Item = Option<&'a str>>) -> u16 {
    identifiers
        .flatten()
        .map(armor_points)
        .fold(0u16, u16::saturating_add)
        .min(20)
}

/// Maximum durability for damageable vanilla items (Bedrock values).
#[must_use]
pub(crate) fn max_durability(identifier: &str) -> Option<u32> {
    let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    let value = match name {
        // Tools and weapons by material tier.
        "wooden_sword" | "wooden_pickaxe" | "wooden_axe" | "wooden_shovel" | "wooden_hoe" => 59,
        "stone_sword" | "stone_pickaxe" | "stone_axe" | "stone_shovel" | "stone_hoe" => 131,
        "iron_sword" | "iron_pickaxe" | "iron_axe" | "iron_shovel" | "iron_hoe" => 250,
        "golden_sword" | "golden_pickaxe" | "golden_axe" | "golden_shovel" | "golden_hoe" => 32,
        "diamond_sword" | "diamond_pickaxe" | "diamond_axe" | "diamond_shovel" | "diamond_hoe" => {
            1_561
        }
        "netherite_sword" | "netherite_pickaxe" | "netherite_axe" | "netherite_shovel"
        | "netherite_hoe" => 2_031,
        // Armor: material base durability times the per-piece multiplier
        // (helmet 11, chestplate 16, leggings 15, boots 13).
        "leather_helmet" => 55,
        "leather_chestplate" => 80,
        "leather_leggings" => 75,
        "leather_boots" => 65,
        "golden_helmet" => 77,
        "golden_chestplate" => 112,
        "golden_leggings" => 105,
        "golden_boots" => 91,
        "chainmail_helmet" | "iron_helmet" => 165,
        "chainmail_chestplate" | "iron_chestplate" => 240,
        "chainmail_leggings" | "iron_leggings" => 225,
        "chainmail_boots" | "iron_boots" => 195,
        "diamond_helmet" => 363,
        "diamond_chestplate" => 528,
        "diamond_leggings" => 495,
        "diamond_boots" => 429,
        "netherite_helmet" => 407,
        "netherite_chestplate" => 592,
        "netherite_leggings" => 555,
        "netherite_boots" => 481,
        "turtle_helmet" => 275,
        // Other damageable vanilla items (Bedrock maxima).
        "bow" => 384,
        "crossbow" => 464,
        "trident" => 250,
        "elytra" => 432,
        "shield" => 336,
        "fishing_rod" => 384,
        "carrot_on_a_stick" => 25,
        "warped_fungus_on_a_stick" => 100,
        "flint_and_steel" => 64,
        "shears" => 238,
        "brush" => 64,
        "mace" => 500,
        _ => return None,
    };
    Some(value)
}

/// Remaining durability in `0.0..=1.0` for a damageable stack, or `None` when
/// the item is untracked, undamaged, or carries no readable damage tag.
/// The reference hides the bar at full durability, so zero damage is `None`.
#[must_use]
pub(crate) fn durability_fraction(
    stack: &NetworkItemStack,
    identifier: Option<&str>,
) -> Option<f32> {
    fraction_from_damage(identifier?, item_stack_damage(stack)?)
}

/// The bar fraction for a known damage value; the wire decoding itself is
/// covered by the protocol crate's `item_stack_damage` tests.
#[must_use]
fn fraction_from_damage(identifier: &str, damage: u32) -> Option<f32> {
    let maximum = max_durability(identifier)?;
    if damage == 0 {
        return None;
    }
    let remaining = maximum.saturating_sub(damage.min(maximum));
    Some(remaining as f32 / maximum as f32)
}

/// Mechanical display name from a vanilla identifier: the path segment in
/// title case ("minecraft:golden_apple" -> "Golden Apple"). This is a
/// recorded approximation until the localization carrier lands; the
/// authoritative identity is never altered, only presented.
#[must_use]
pub(crate) fn mechanical_display_name(identifier: &str) -> String {
    let tail = identifier
        .rsplit_once(':')
        .map_or(identifier, |(_, tail)| tail);
    let mut name = String::with_capacity(tail.len());
    for (index, word) in tail.split('_').enumerate() {
        if index > 0 {
            name.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            name.extend(first.to_uppercase());
            name.push_str(chars.as_str());
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use protocol::NetworkItemStack;

    use super::*;

    #[test]
    fn armor_points_follow_the_pinned_vanilla_table() {
        assert_eq!(armor_points("minecraft:diamond_chestplate"), 8);
        assert_eq!(armor_points("minecraft:leather_boots"), 1);
        assert_eq!(armor_points("minecraft:turtle_helmet"), 2);
        assert_eq!(armor_points("minecraft:elytra"), 0);
        assert_eq!(armor_points("custom:armor"), 0);
        let total = total_armor_points(
            [
                Some("minecraft:iron_helmet"),
                Some("minecraft:iron_chestplate"),
                Some("minecraft:iron_leggings"),
                Some("minecraft:iron_boots"),
                None,
            ]
            .into_iter(),
        );
        assert_eq!(total, 15);
        // A pathological modded sum clamps to the reference bar.
        let clamped = total_armor_points(
            [Some("minecraft:diamond_chestplate"); 4]
                .map(Some)
                .map(|value| value.flatten())
                .into_iter(),
        );
        assert_eq!(clamped, 20);
    }

    #[test]
    fn durability_fractions_follow_the_pinned_maxima_and_hide_pristine_bars() {
        let fraction = fraction_from_damage("minecraft:iron_sword", 125).unwrap();
        assert!((fraction - 0.5).abs() < 0.01);
        assert_eq!(fraction_from_damage("minecraft:iron_sword", 0), None);
        assert_eq!(fraction_from_damage("minecraft:stick", 125), None);
        // Over-damage clamps to an empty bar instead of wrapping.
        assert_eq!(
            fraction_from_damage("minecraft:iron_sword", 9_999),
            Some(0.0)
        );
        // A stack with no extra data reads as no bar at the public boundary.
        assert_eq!(
            durability_fraction(&NetworkItemStack::empty(), Some("minecraft:iron_sword")),
            None
        );
        assert_eq!(durability_fraction(&NetworkItemStack::empty(), None), None);
    }

    #[test]
    fn mechanical_names_title_case_the_identifier_tail() {
        assert_eq!(
            mechanical_display_name("minecraft:golden_apple"),
            "Golden Apple"
        );
        assert_eq!(mechanical_display_name("minecraft:tnt"), "Tnt");
        assert_eq!(mechanical_display_name("oddity"), "Oddity");
    }
}
