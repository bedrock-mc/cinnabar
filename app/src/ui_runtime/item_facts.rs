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
        // The 1.26.30 copper set (base durability 11) protects between
        // leather and chainmail: 2/4/3/1.
        "copper_helmet" => 2,
        "copper_chestplate" => 4,
        "copper_leggings" | "golden_leggings" => 3,
        "copper_boots" => 1,
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
        "copper_sword" | "copper_pickaxe" | "copper_axe" | "copper_shovel" | "copper_hoe" => 190,
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
        "copper_helmet" => 121,
        "copper_chestplate" => 176,
        "copper_leggings" => 165,
        "copper_boots" => 143,
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
    durability_fraction_for_damage(identifier, item_stack_damage(stack)?)
}

/// The bar fraction for a server-corrected damage value.
///
/// Response corrections carry the same maximum-minus-remaining quantity as
/// the stack's NBT `Damage` tag, so both paths share one fraction contract;
/// an unknown identifier fails closed exactly like the derived path.
#[must_use]
pub(crate) fn durability_fraction_for_damage(identifier: Option<&str>, damage: u32) -> Option<f32> {
    fraction_from_damage(identifier?, damage)
}

/// Remaining durability for one HUD cell, preferring an authoritative server
/// durability correction over locally derived NBT damage. A negative
/// correction is semantically odd wire data and falls back to local
/// derivation; a zero correction keeps the bar hidden exactly like a pristine
/// stack.
#[must_use]
pub(crate) fn cell_durability_fraction(
    stack: &NetworkItemStack,
    identifier: Option<&str>,
    durability_correction: Option<i32>,
) -> Option<f32> {
    match durability_correction {
        Some(damage) if damage >= 0 => {
            fraction_from_damage(identifier?, u32::try_from(damage).unwrap_or(u32::MAX))
        }
        _ => durability_fraction(stack, identifier),
    }
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
        // The 1.26.30 copper set sits between leather and chainmail.
        assert_eq!(armor_points("minecraft:copper_helmet"), 2);
        assert_eq!(armor_points("minecraft:copper_chestplate"), 4);
        assert_eq!(armor_points("minecraft:copper_leggings"), 3);
        assert_eq!(armor_points("minecraft:copper_boots"), 1);
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
    fn copper_durabilities_follow_the_material_scheme() {
        // Copper tools share the 190 tier between stone (131) and iron (250);
        // copper armor is material base 11 times the per-piece multipliers.
        for tool in [
            "minecraft:copper_sword",
            "minecraft:copper_pickaxe",
            "minecraft:copper_axe",
            "minecraft:copper_shovel",
            "minecraft:copper_hoe",
        ] {
            assert_eq!(max_durability(tool), Some(190), "{tool}");
        }
        assert_eq!(max_durability("minecraft:copper_helmet"), Some(121));
        assert_eq!(max_durability("minecraft:copper_chestplate"), Some(176));
        assert_eq!(max_durability("minecraft:copper_leggings"), Some(165));
        assert_eq!(max_durability("minecraft:copper_boots"), Some(143));
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
    fn corrected_damage_drives_the_same_fraction_contract_as_nbt_damage() {
        // Iron sword maximum 250: a server-corrected damage of 125 is exactly
        // half, zero damage hides the bar, and unknown identifiers stay shut.
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                Some("minecraft:iron_sword"),
                Some(125)
            ),
            Some(0.5)
        );
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                Some("minecraft:iron_sword"),
                Some(0)
            ),
            None
        );
        assert_eq!(
            cell_durability_fraction(&NetworkItemStack::empty(), Some("minecraft:stick"), Some(5)),
            None
        );
        assert_eq!(
            cell_durability_fraction(&NetworkItemStack::empty(), None, Some(5)),
            None
        );
        // Over-damage clamps to an empty bar instead of wrapping.
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                Some("minecraft:iron_sword"),
                Some(9_999)
            ),
            Some(0.0)
        );
    }

    /// Builds one stack whose retained user data carries a vanilla `Damage`
    /// integer, exactly as the fixed little-endian wire encoding stores it.
    fn stack_with_damage(damage: i32) -> NetworkItemStack {
        use sha2::{Digest, Sha256};
        use std::sync::Arc;

        let mut extra = Vec::new();
        extra.extend_from_slice(&(-1_i16).to_le_bytes());
        extra.push(1);
        extra.push(10);
        extra.extend_from_slice(&0_u16.to_le_bytes());
        extra.push(3);
        extra.extend_from_slice(&6_u16.to_le_bytes());
        extra.extend_from_slice(b"Damage");
        extra.extend_from_slice(&damage.to_le_bytes());
        extra.push(0);
        NetworkItemStack {
            network_id: 7,
            metadata: 0,
            stack_network_id: -1,
            count: 1,
            nbt_digest: Sha256::digest(&extra).into(),
            block_runtime_id: 0,
            extra_data: Arc::from(extra),
        }
    }

    #[test]
    fn authoritative_corrections_take_precedence_over_derived_damage() {
        // A correction wins even when the local stack carries no readable damage.
        let fraction = cell_durability_fraction(
            &NetworkItemStack::empty(),
            Some("minecraft:iron_sword"),
            Some(125),
        )
        .unwrap();
        assert!((fraction - 0.5).abs() < 0.01);
        // Zero correction keeps the bar hidden exactly like a pristine stack.
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                Some("minecraft:iron_sword"),
                Some(0)
            ),
            None
        );
        // A negative correction is semantically odd wire data: local derivation stands.
        let damaged = stack_with_damage(125);
        assert_eq!(
            cell_durability_fraction(&damaged, Some("minecraft:iron_sword"), Some(-3)),
            durability_fraction(&damaged, Some("minecraft:iron_sword"))
        );
        // Unknown maxima stay hidden under correction too.
        assert_eq!(
            cell_durability_fraction(&NetworkItemStack::empty(), Some("minecraft:stick"), Some(5)),
            None
        );
        // Without a correction the existing derivation is reproduced exactly.
        assert_eq!(
            cell_durability_fraction(&damaged, Some("minecraft:iron_sword"), None),
            durability_fraction(&damaged, Some("minecraft:iron_sword"))
        );
        assert_eq!(
            cell_durability_fraction(&NetworkItemStack::empty(), None, Some(125)),
            None
        );
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
