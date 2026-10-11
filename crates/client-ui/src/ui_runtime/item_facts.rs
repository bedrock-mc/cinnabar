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

use std::{collections::HashMap, sync::Arc};

use protocol::{ItemComponents, NetworkItemStack, item_stack_damage};

/// The session's server item components by identifier.
#[derive(Debug, Default)]
pub struct SessionItemComponents(HashMap<Arc<str>, SessionItem>);

/// Components and the UI facts resolved once when the session admits an item.
#[derive(Debug)]
struct SessionItem {
    components: ItemComponents,
    name_color: Option<ui::BedrockColor>,
}

impl SessionItemComponents {
    /// `None` when no StartGame item declares components.
    pub fn from_game_data(game_data: &protocol::GameData) -> Option<Arc<Self>> {
        let items = Self::from_iter(protocol::item_components(game_data));
        (!items.0.is_empty()).then(|| Arc::new(items))
    }

    pub fn get(&self, identifier: &str) -> Option<&ItemComponents> {
        self.0.get(identifier).map(|item| &item.components)
    }

    /// The semantic name colour already resolved for this session item.
    pub fn name_color(&self, identifier: &str) -> Option<ui::BedrockColor> {
        self.0.get(identifier)?.name_color
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Arc<str>, &ItemComponents)> {
        self.0.iter().map(|(id, item)| (id, &item.components))
    }
}

impl FromIterator<(Arc<str>, ItemComponents)> for SessionItemComponents {
    fn from_iter<I: IntoIterator<Item = (Arc<str>, ItemComponents)>>(items: I) -> Self {
        Self(
            items
                .into_iter()
                .map(|(id, components)| {
                    let name_color = name_format(&components);
                    (
                        id,
                        SessionItem {
                            components,
                            name_color,
                        },
                    )
                })
                .collect(),
        )
    }
}

/// Armor points for one equipped vanilla armor identifier.
#[must_use]
pub fn armor_points(identifier: &str) -> u16 {
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
pub fn total_armor_points<'a>(identifiers: impl Iterator<Item = Option<&'a str>>) -> u16 {
    identifiers
        .flatten()
        .map(armor_points)
        .fold(0u16, u16::saturating_add)
        .min(20)
}

/// Remaining durability in `0.0..=1.0` for a damageable stack, or `None` when
/// the item is untracked, undamaged, or carries no readable damage tag.
/// The reference hides the bar at full durability, so zero damage is `None`.
#[must_use]
pub fn durability_fraction(stack: &NetworkItemStack, maximum: Option<u32>) -> Option<f32> {
    durability_fraction_for_damage(maximum, item_stack_damage(stack)?)
}

/// The bar fraction for a server-corrected damage value.
///
/// Response corrections carry the same maximum-minus-remaining quantity as
/// the stack's NBT `Damage` tag, so both paths share one fraction contract;
/// an unknown maximum fails closed exactly like the derived path.
#[must_use]
pub fn durability_fraction_for_damage(maximum: Option<u32>, damage: u32) -> Option<f32> {
    fraction_from_damage(maximum?, damage)
}

/// Remaining durability for one HUD cell, preferring an authoritative server
/// durability correction over locally derived NBT damage. A negative
/// correction is semantically odd wire data and falls back to local
/// derivation. Reading the wire correction as maximum-minus-remaining damage,
/// including presenting a stated zero as a hidden pristine bar, is a
/// provisional domain pinned from the reference server implementation, not a
/// natively measured Bedrock contract; presentation changes only after that
/// measurement exists.
#[must_use]
pub fn cell_durability_fraction(
    stack: &NetworkItemStack,
    maximum: Option<u32>,
    durability_correction: Option<i32>,
) -> Option<f32> {
    match durability_correction {
        Some(damage) if damage >= 0 => {
            fraction_from_damage(maximum?, u32::try_from(damage).unwrap_or(u32::MAX))
        }
        _ => durability_fraction(stack, maximum),
    }
}

/// The bar fraction for a known damage value; the wire decoding itself is
/// covered by the protocol crate's `item_stack_damage` tests.
#[must_use]
fn fraction_from_damage(maximum: u32, damage: u32) -> Option<f32> {
    if damage == 0 || maximum == 0 {
        return None;
    }
    let remaining = maximum.saturating_sub(damage.min(maximum));
    Some(remaining as f32 / maximum as f32)
}

/// Whether a stack glints as in vanilla: an `ench` list, the item's glint
/// component, or a vanilla item that always glints.
#[must_use]
pub fn is_glint(
    stack: &NetworkItemStack,
    identifier: &str,
    components: Option<&ItemComponents>,
) -> bool {
    protocol::item_has_enchantment_list(&stack.extra_data)
        || components.is_some_and(|components| components.glint)
        || matches!(
            identifier,
            "minecraft:enchanted_book"
                | "minecraft:experience_bottle"
                | "minecraft:written_book"
                | "minecraft:end_crystal"
                | "minecraft:enchanted_golden_apple"
        )
}

/// Resolve a component item's semantic colour at admission: explicit hover colour
/// first, then the supported rarity colour. Unknown names stay unresolved.
#[must_use]
pub fn name_format(components: &ItemComponents) -> Option<ui::BedrockColor> {
    let named = components
        .hover_text_color
        .as_deref()
        .or(match components.rarity.as_deref() {
            Some("uncommon") => Some("yellow"),
            Some("rare") => Some("aqua"),
            Some("epic") => Some("light_purple"),
            _ => None,
        })?;
    ui::BedrockColor::from_name(named)
}

/// Mechanical display name from a vanilla identifier: the path segment in
/// title case ("minecraft:golden_apple" -> "Golden Apple"). This is a
/// recorded approximation until the localization carrier lands; the
/// authoritative identity is never altered, only presented.
#[must_use]
pub fn mechanical_display_name(identifier: &str) -> String {
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
    fn durability_fractions_follow_the_pinned_maxima_and_hide_pristine_bars() {
        let fraction = fraction_from_damage(250, 125).unwrap();
        assert!((fraction - 0.5).abs() < 0.01);
        assert_eq!(fraction_from_damage(250, 0), None);
        assert_eq!(
            durability_fraction_for_damage(
                client_world::vanilla_max_durability("minecraft:stick"),
                125
            ),
            None
        );
        // Over-damage clamps to an empty bar instead of wrapping.
        assert_eq!(fraction_from_damage(250, 9_999), Some(0.0));
        // A stack with no extra data reads as no bar at the public boundary.
        assert_eq!(
            durability_fraction(
                &NetworkItemStack::empty(),
                client_world::vanilla_max_durability("minecraft:iron_sword")
            ),
            None
        );
        assert_eq!(durability_fraction(&NetworkItemStack::empty(), None), None);
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
        // A correction wins over conflicting damage carried by the local stack.
        let fraction = cell_durability_fraction(
            &stack_with_damage(125),
            client_world::vanilla_max_durability("minecraft:iron_sword"),
            Some(50),
        )
        .unwrap();
        assert!((fraction - 0.8).abs() < 0.01);
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                client_world::vanilla_max_durability("minecraft:iron_sword"),
                Some(9_999),
            ),
            Some(0.0),
            "over-damage clamps instead of wrapping"
        );
        // Zero correction keeps the bar hidden exactly like a pristine stack.
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                client_world::vanilla_max_durability("minecraft:iron_sword"),
                Some(0)
            ),
            None
        );
        // A negative correction is semantically odd wire data: local derivation stands.
        let damaged = stack_with_damage(125);
        assert_eq!(
            cell_durability_fraction(
                &damaged,
                client_world::vanilla_max_durability("minecraft:iron_sword"),
                Some(-3)
            ),
            durability_fraction(
                &damaged,
                client_world::vanilla_max_durability("minecraft:iron_sword")
            )
        );
        // Unknown maxima stay hidden under correction too.
        assert_eq!(
            cell_durability_fraction(
                &NetworkItemStack::empty(),
                client_world::vanilla_max_durability("minecraft:stick"),
                Some(5)
            ),
            None
        );
        // Without a correction the existing derivation is reproduced exactly.
        assert_eq!(
            cell_durability_fraction(
                &damaged,
                client_world::vanilla_max_durability("minecraft:iron_sword"),
                None
            ),
            durability_fraction(
                &damaged,
                client_world::vanilla_max_durability("minecraft:iron_sword")
            )
        );
        assert_eq!(
            cell_durability_fraction(&NetworkItemStack::empty(), None, Some(125)),
            None
        );
    }

    // Component names translate as keys or show literally; custom maxima drive the bar.
    #[test]
    fn session_components_name_and_bound_custom_items() {
        let mut player_runtime = player_state::PlayerState::new(1);

        let mut runtime = crate::ui_runtime::UiRuntime::new(1);
        let input = b"item.zeqa.blade.name=Zeqa Blade\n";
        runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
            target.copy_from_slice(input);
            true
        }));
        let components = |name: &str, max_durability| ItemComponents {
            display_name: Some(name.into()),
            max_durability,
            ..ItemComponents::default()
        };
        runtime.set_session_items(Some(Arc::new(SessionItemComponents::from_iter([
            (
                Arc::from("zeqa:blade"),
                components("item.zeqa.blade.name", Some(100)),
            ),
            (Arc::from("zeqa:gem"), components("Shiny Gem", None)),
        ]))));
        assert_eq!(runtime.localized_item_name("zeqa:blade"), "Zeqa Blade");
        assert_eq!(runtime.localized_item_name("zeqa:gem"), "Shiny Gem");
        assert_eq!(runtime.item_max_durability(Some("zeqa:blade")), Some(100));
        assert_eq!(runtime.item_max_durability(Some("zeqa:gem")), None);
        assert_eq!(
            runtime.item_max_durability(Some("minecraft:iron_sword")),
            Some(250)
        );
        player_runtime.begin_session(2);
        runtime.begin_session(2);
        assert_eq!(runtime.localized_item_name("zeqa:gem"), "Gem");
    }

    // Glint follows an ench list (even empty), the glint component, or an always-glinting item.
    #[test]
    fn glint_follows_enchantments_components_and_vanilla_items() {
        let plain = NetworkItemStack::empty();
        assert!(!is_glint(&plain, "minecraft:stick", None));
        assert!(is_glint(&plain, "minecraft:enchanted_book", None));
        let foil = ItemComponents {
            glint: true,
            ..ItemComponents::default()
        };
        assert!(is_glint(&plain, "zeqa:blade", Some(&foil)));
        let mut enchanted = stack_with_damage(0);
        let mut extra = enchanted.extra_data.to_vec();
        let end = extra.len() - 1;
        extra.splice(end..end, [9, 4, 0, b'e', b'n', b'c', b'h', 10, 0, 0, 0, 0]);
        enchanted.extra_data = Arc::from(extra);
        assert!(is_glint(&enchanted, "minecraft:stick", None));
    }

    // hover_text_color outranks rarity; unknown names and common rarity keep the default.
    #[test]
    fn name_format_follows_hover_colour_then_rarity() {
        let mut components = ItemComponents {
            rarity: Some("epic".into()),
            ..ItemComponents::default()
        };
        assert_eq!(
            name_format(&components),
            Some(ui::BedrockColor::LightPurple)
        );
        components.hover_text_color = Some("gold".into());
        assert_eq!(name_format(&components), Some(ui::BedrockColor::Gold));
        components.hover_text_color = Some("chartreuse".into());
        assert_eq!(name_format(&components), None);
        assert_eq!(name_format(&ItemComponents::default()), None);
    }

    #[test]
    fn session_name_colours_share_text_semantics_and_refresh_on_replacement() {
        let id: Arc<str> = "example:custom_item".into();
        let components = |name: &str| ItemComponents {
            hover_text_color: Some(Arc::from(name)),
            ..Default::default()
        };
        for (name, expected) in [
            ("material_emerald", ui::BedrockColor::MaterialEmerald),
            ("material_lapis", ui::BedrockColor::MaterialLapis),
            ("material_resin", ui::BedrockColor::MaterialResin),
        ] {
            let items = SessionItemComponents::from_iter([(Arc::clone(&id), components(name))]);
            let color = items.name_color(&id).unwrap();
            assert_eq!(color, expected);
            let descriptor = color.descriptor().unwrap();
            let text = format!("§{}Name", descriptor.code);
            let spans = ui::parse_bedrock_text(&text, text.len()).unwrap();
            assert_eq!(color.rgb(), spans[0].style.color.rgb());
        }
        let previous = SessionItemComponents::from_iter([(Arc::clone(&id), components("aqua"))]);
        let replacement = SessionItemComponents::from_iter([(Arc::clone(&id), components("gold"))]);
        assert_eq!(previous.name_color(&id), Some(ui::BedrockColor::Aqua));
        assert_eq!(replacement.name_color(&id), Some(ui::BedrockColor::Gold));
        assert_eq!(previous.iter().count(), 1);
        assert_eq!(
            previous.get(&id).unwrap().hover_text_color.as_deref(),
            Some("aqua")
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
