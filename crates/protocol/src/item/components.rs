//! Client-facing facts a server item's registry components declare.

use std::sync::Arc;

use jolyne::GameData;

use crate::nbt_tree::{Nbt, read_root};

pub(super) const MAX_TEXT_BYTES: usize = 256;

/// Converts component seconds to the protocol's integral simulation duration.
pub(super) fn duration_ticks(seconds: f64) -> Option<u32> {
    (seconds.is_finite() && seconds >= 0.0)
        .then(|| (seconds * 20.0).round().min(f64::from(u32::MAX)) as u32)
}

/// What the client presents from one item's components; absent facts stay `None`/`false`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemComponents {
    /// Attack and kinetic presentation facts, when this item declares them.
    pub attack: Option<super::ItemAttackTiming>,
    /// `item_texture.json` key from `minecraft:icon`.
    pub icon: Option<Arc<str>>,
    /// `minecraft:display_name` value: a localization key or literal text.
    pub display_name: Option<Arc<str>>,
    /// `foil` / `minecraft:glint`.
    pub glint: bool,
    pub hand_equipped: bool,
    pub max_durability: Option<u32>,
    pub max_stack_size: Option<u8>,
    /// `use_animation` by name (`eat`, `drink`, `bow`...); the legacy enum maps 1 and 2.
    pub use_animation: Option<Arc<str>>,
    pub use_duration_ticks: Option<u32>,
    /// Whether `minecraft:food` is declared.
    pub food: bool,
    /// `minecraft:wearable` slot, e.g. `slot.armor.head`.
    pub wearable_slot: Option<Arc<str>>,
    /// Block `minecraft:block_placer` places.
    pub block_placer: Option<Arc<str>>,
    /// `minecraft:rarity`: `common`, `uncommon`, `rare` or `epic`.
    pub rarity: Option<Arc<str>>,
    /// `minecraft:hover_text_color` format-code name, e.g. `gold`.
    pub hover_text_color: Option<Arc<str>>,
}

/// Components for every StartGame registry item whose component data is non-empty.
#[must_use]
pub fn item_components(game_data: &GameData) -> Box<[(Arc<str>, ItemComponents)]> {
    game_data
        .item_registry
        .item_data
        .iter()
        .filter_map(|item| {
            let bytes = super::encode_extra(&item.item_component_data).ok()?;
            let components = parse_components(&bytes)?;
            Some((Arc::from(item.item_name.as_str()), components))
        })
        .collect()
}

/// Reads both the `item_properties` and the top-level component layouts; `None` when the
/// data carries no components compound.
pub(super) fn parse_components(bytes: &[u8]) -> Option<ItemComponents> {
    let root = read_root(bytes)?;
    let components = root.field("components")?;
    let properties = components.field("item_properties");
    let property = |name: &str| properties.and_then(|properties| properties.field(name));
    let component = |name: &str| components.field(name);
    let value_of = |nbt: &Nbt| nbt.field("value").or(Some(nbt)).and_then(Nbt::number);
    let text = |nbt: Option<&Nbt>| {
        nbt.and_then(Nbt::as_str)
            .filter(|text| !text.is_empty() && text.len() <= MAX_TEXT_BYTES)
            .map(Arc::from)
    };
    let flag = |nbt: Option<&Nbt>| nbt.and_then(value_of).is_some_and(|value| value != 0.0);
    let count = |nbt: Option<&Nbt>| {
        nbt.and_then(value_of)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value.min(f64::from(u32::MAX)) as u32)
    };
    let use_duration_ticks = count(property("use_duration")).or_else(|| {
        component("minecraft:use_modifiers")
            .and_then(|modifiers| modifiers.field("use_duration"))
            .and_then(Nbt::number)
            .and_then(duration_ticks)
    });
    Some(ItemComponents {
        attack: super::attack::parse_attack(components),
        icon: super::icons::icon_key(bytes),
        display_name: text(
            component("minecraft:display_name").and_then(|name| name.field("value")),
        ),
        glint: flag(property("foil")) || flag(component("minecraft:glint")),
        hand_equipped: flag(property("hand_equipped"))
            || flag(component("minecraft:hand_equipped")),
        max_durability: count(
            component("minecraft:durability")
                .and_then(|durability| durability.field("max_durability")),
        )
        .filter(|maximum| *maximum > 0),
        max_stack_size: count(property("max_stack_size").or(component("minecraft:max_stack_size")))
            .filter(|size| (1..=255).contains(size))
            .map(|size| size as u8),
        use_animation: text(
            component("minecraft:use_animation").and_then(|animation| animation.field("value")),
        )
        .or_else(|| {
            match property("use_animation")
                .and_then(Nbt::number)
                .map(|value| value as i32)
            {
                Some(1) => Some("eat".into()),
                Some(2) => Some("drink".into()),
                _ => None,
            }
        }),
        use_duration_ticks,
        food: component("minecraft:food").is_some(),
        wearable_slot: text(
            component("minecraft:wearable").and_then(|wearable| wearable.field("slot")),
        ),
        block_placer: component("minecraft:block_placer").and_then(|placer| {
            let block = placer.field("block")?;
            text(Some(block)).or_else(|| text(block.field("name")))
        }),
        rarity: text(component("minecraft:rarity").and_then(|rarity| rarity.field("value"))),
        hover_text_color: text(
            component("minecraft:hover_text_color").and_then(|color| color.field("value")),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_components;

    fn name(tag: u8, name: &str) -> Vec<u8> {
        let mut bytes = vec![tag, name.len() as u8];
        bytes.extend_from_slice(name.as_bytes());
        bytes
    }

    fn string(key: &str, value: &str) -> Vec<u8> {
        let mut bytes = name(8, key);
        bytes.push(value.len() as u8);
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn int(key: &str, value: i32) -> Vec<u8> {
        let mut bytes = name(3, key);
        let mut zigzag = ((value << 1) ^ (value >> 31)) as u32;
        loop {
            let byte = (zigzag & 0x7f) as u8;
            zigzag >>= 7;
            if zigzag == 0 {
                bytes.push(byte);
                break;
            }
            bytes.push(byte | 0x80);
        }
        bytes
    }

    fn byte(key: &str, value: u8) -> Vec<u8> {
        let mut bytes = name(1, key);
        bytes.push(value);
        bytes
    }

    fn compound(key: &str, entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = name(10, key);
        entries.iter().for_each(|entry| bytes.extend(entry));
        bytes.push(0);
        bytes
    }

    // Dragonfly's custom-item layout: properties nested under item_properties.
    #[test]
    fn food_classification_uses_the_food_component() {
        let food = compound(
            "",
            &[compound("components", &[compound("minecraft:food", &[])])],
        );
        assert!(parse_components(&food).unwrap().food);
        let animation = compound(
            "",
            &[compound(
                "components",
                &[compound(
                    "minecraft:use_animation",
                    &[string("value", "eat")],
                )],
            )],
        );
        assert!(!parse_components(&animation).unwrap().food);
    }

    #[test]
    fn reads_the_item_properties_layout() {
        let nbt = compound(
            "",
            &[compound(
                "components",
                &[
                    compound(
                        "item_properties",
                        &[
                            compound(
                                "minecraft:icon",
                                &[compound("textures", &[string("default", "zeqa:sword")])],
                            ),
                            int("max_stack_size", 16),
                            byte("foil", 1),
                            byte("hand_equipped", 1),
                            int("use_animation", 1),
                            int("use_duration", 32),
                        ],
                    ),
                    compound(
                        "minecraft:display_name",
                        &[string("value", "item.zeqa.sword.name")],
                    ),
                    compound("minecraft:durability", &[int("max_durability", 250)]),
                    compound("minecraft:wearable", &[string("slot", "slot.armor.head")]),
                    compound("minecraft:block_placer", &[string("block", "zeqa:crate")]),
                ],
            )],
        );
        let components = parse_components(&nbt).unwrap();
        assert_eq!(components.icon.as_deref(), Some("zeqa:sword"));
        assert_eq!(
            components.display_name.as_deref(),
            Some("item.zeqa.sword.name")
        );
        assert!(components.glint && components.hand_equipped);
        assert_eq!(components.max_durability, Some(250));
        assert_eq!(components.max_stack_size, Some(16));
        assert_eq!(components.use_animation.as_deref(), Some("eat"));
        assert_eq!(components.use_duration_ticks, Some(32));
        assert_eq!(components.wearable_slot.as_deref(), Some("slot.armor.head"));
        assert_eq!(components.block_placer.as_deref(), Some("zeqa:crate"));
    }

    // Current add-on layout: top-level value components, use_modifiers in seconds.
    #[test]
    fn reads_top_level_value_components() {
        let nbt = compound(
            "",
            &[compound(
                "components",
                &[
                    compound("minecraft:glint", &[byte("value", 1)]),
                    compound("minecraft:use_animation", &[string("value", "drink")]),
                    compound("minecraft:rarity", &[string("value", "epic")]),
                    compound("minecraft:hover_text_color", &[string("value", "gold")]),
                    compound("minecraft:hand_equipped", &[byte("value", 0)]),
                    compound("minecraft:max_stack_size", &[byte("value", 1)]),
                    compound(
                        "minecraft:use_modifiers",
                        &[{
                            let mut seconds = name(5, "use_duration");
                            seconds.extend(1.6f32.to_le_bytes());
                            seconds
                        }],
                    ),
                    compound(
                        "minecraft:block_placer",
                        &[compound("block", &[string("name", "test:ore")])],
                    ),
                ],
            )],
        );
        let components = parse_components(&nbt).unwrap();
        assert!(components.glint && !components.hand_equipped);
        assert_eq!(components.max_stack_size, Some(1));
        assert_eq!(components.use_duration_ticks, Some(32));
        assert_eq!(components.use_animation.as_deref(), Some("drink"));
        assert_eq!(components.rarity.as_deref(), Some("epic"));
        assert_eq!(components.hover_text_color.as_deref(), Some("gold"));
        assert_eq!(components.block_placer.as_deref(), Some("test:ore"));
        assert!(parse_components(&compound("", &[])).is_none());
    }
}
