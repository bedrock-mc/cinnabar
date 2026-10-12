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
    /// Parsed fuel duration; missing or malformed declarations stay absent.
    pub fuel_duration_ticks: Option<u32>,
    /// Whether fuel was declared, including malformed component values.
    pub fuel_declared: bool,
    /// Attack and kinetic presentation facts, when this item declares them.
    pub attack: Option<super::ItemAttackTiming>,
    /// `item_texture.json` key from `minecraft:icon`.
    pub icon: Option<Arc<str>>,
    /// `minecraft:display_name` value: a localization key or literal text.
    pub display_name: Option<Arc<str>>,
    /// `foil` / `minecraft:glint`.
    pub glint: bool,
    /// Explicit finite hand flag; absence or malformed data remains unresolved.
    pub hand_equipped: Option<bool>,
    /// Distinguishes a malformed declaration from an absent hand component.
    pub hand_equipped_declared: bool,
    pub max_durability: Option<u32>,
    /// Retains declaration presence when its maximum is absent or invalid.
    pub durability_declared: bool,
    pub max_stack_size: Option<u8>,
    /// `use_animation` by name (`eat`, `drink`, `bow`...); the legacy enum maps 1 and 2.
    pub use_animation: Option<Arc<str>>,
    pub use_duration_ticks: Option<u32>,
    /// Whether `minecraft:food` is declared.
    pub food: bool,
    /// `minecraft:wearable` slot, e.g. `slot.armor.head`.
    pub wearable_slot: Option<Arc<str>>,
    /// Whether the payload declares `minecraft:wearable`, including malformed declarations.
    pub wearable_declared: bool,
    /// Exact non-negative integer protection; absent or invalid values stay unresolved.
    pub wearable_protection: Option<u32>,
    /// Block `minecraft:block_placer` places.
    pub block_placer: Option<Arc<str>>,
    /// `minecraft:rarity`: `common`, `uncommon`, `rare` or `epic`.
    pub rarity: Option<Arc<str>>,
    /// `minecraft:hover_text_color` format-code name, e.g. `gold`.
    pub hover_text_color: Option<Arc<str>>,
}

impl ItemComponents {
    /// Counts the retained component allocation and its owned text for admission budgets.
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>().saturating_add(32);
        for text in [
            self.icon.as_deref(),
            self.display_name.as_deref(),
            self.use_animation.as_deref(),
            self.wearable_slot.as_deref(),
            self.block_placer.as_deref(),
            self.rarity.as_deref(),
            self.hover_text_color.as_deref(),
            self.attack
                .as_ref()
                .and_then(|attack| attack.attack_cooldown.as_ref())
                .map(|cooldown| &*cooldown.category),
        ]
        .into_iter()
        .flatten()
        {
            bytes = bytes.saturating_add(text.len()).saturating_add(32);
        }
        bytes
    }
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
    // A non-compound payload carries no usable facts, not confirmed absence.
    let components = root
        .field("components")
        .filter(|components| matches!(components, Nbt::Compound(_)))?;
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
        fuel_duration_ticks: component("minecraft:fuel")
            .and_then(|fuel| fuel.field("duration"))
            .and_then(Nbt::number)
            .and_then(duration_ticks),
        fuel_declared: component("minecraft:fuel").is_some(),
        attack: super::attack::parse_attack(components),
        icon: super::icons::icon_key(bytes),
        display_name: text(
            component("minecraft:display_name").and_then(|name| name.field("value")),
        ),
        glint: flag(property("foil")) || flag(component("minecraft:glint")),
        hand_equipped_declared: property("hand_equipped").is_some()
            || component("minecraft:hand_equipped").is_some(),
        hand_equipped: {
            let property = property("hand_equipped");
            let component = component("minecraft:hand_equipped");
            let declared = |value: &Nbt| {
                value_of(value)
                    .filter(|value| value.is_finite())
                    .map(|value| value != 0.0)
            };
            match (property, component) {
                (None, None) => None,
                (Some(value), None) | (None, Some(value)) => declared(value),
                (Some(property), Some(component)) => declared(property)
                    .zip(declared(component))
                    .map(|(property, component)| property || component),
            }
        },
        durability_declared: component("minecraft:durability").is_some(),
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
        wearable_declared: component("minecraft:wearable").is_some(),
        wearable_protection: component("minecraft:wearable")
            .and_then(|wearable| wearable.field("protection"))
            .and_then(|value| match value {
                Nbt::Int(value) if (0..=i64::from(i32::MAX)).contains(value) => Some(*value as u32),
                _ => None,
            }),
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

    #[test]
    fn durability_declaration_retains_absent_invalid_and_positive_maxima() {
        for (fields, expected) in [
            (vec![], None),
            (vec![int("max_durability", 0)], None),
            (vec![int("max_durability", -1)], None),
            (vec![int("max_durability", 50)], Some(50)),
        ] {
            let root = compound(
                "",
                &[compound(
                    "components",
                    &[compound("minecraft:durability", &fields)],
                )],
            );
            let facts = parse_components(&root).unwrap();
            assert!(facts.durability_declared);
            assert_eq!(facts.max_durability, expected);
        }
        let root = compound("", &[compound("components", &[])]);
        assert!(!parse_components(&root).unwrap().durability_declared);
    }

    #[test]
    fn scalar_components_payload_is_not_admitted_as_empty_facts() {
        let root = compound("", &[int("components", 1)]);
        assert!(parse_components(&root).is_none());
    }

    // Dragonfly's custom-item layout: properties nested under item_properties.
    #[test]
    fn fuel_duration_distinguishes_zero_missing_and_malformed_data() {
        let missing = parse_components(&compound("", &[compound("components", &[])])).unwrap();
        assert_eq!(missing.fuel_duration_ticks, None);
        assert!(!missing.fuel_declared);
        for (seconds, expected) in [
            (2.0f32, Some(40)),
            (0.0, Some(0)),
            (-1.0, None),
            (f32::NAN, None),
        ] {
            let mut duration = name(5, "duration");
            duration.extend(seconds.to_le_bytes());
            let components = parse_components(&compound(
                "",
                &[compound(
                    "components",
                    &[compound("minecraft:fuel", &[duration])],
                )],
            ))
            .unwrap();
            assert_eq!(components.fuel_duration_ticks, expected);
            assert!(components.fuel_declared);
        }
        let malformed = parse_components(&compound(
            "",
            &[compound(
                "components",
                &[compound("minecraft:fuel", &[string("duration", "wrong")])],
            )],
        ))
        .unwrap();
        assert_eq!(malformed.fuel_duration_ticks, None);
        assert!(malformed.fuel_declared);
    }

    #[test]
    fn wearable_protection_retains_zero_and_rejects_invalid_or_missing_values() {
        for (value, expected) in [
            (Some(int("protection", 6)), Some(6)),
            (Some(int("protection", 0)), Some(0)),
            (Some(int("protection", -1)), None),
            (Some(string("protection", "6")), None),
            (None, None),
        ] {
            let mut fields = vec![string("slot", "slot.armor.chest")];
            fields.extend(value);
            let payload = compound(
                "",
                &[compound(
                    "components",
                    &[compound("minecraft:wearable", &fields)],
                )],
            );
            let facts = parse_components(&payload).unwrap();
            assert!(facts.wearable_declared);
            assert_eq!(facts.wearable_slot.as_deref(), Some("slot.armor.chest"));
            assert_eq!(facts.wearable_protection, expected);
        }
        let absent = compound("", &[compound("components", &[])]);
        assert!(!parse_components(&absent).unwrap().wearable_declared);
    }

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
        assert!(components.glint && components.hand_equipped == Some(true));
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
        assert!(components.glint && components.hand_equipped == Some(false));
        assert_eq!(components.max_stack_size, Some(1));
        assert_eq!(components.use_duration_ticks, Some(32));
        assert_eq!(components.use_animation.as_deref(), Some("drink"));
        assert_eq!(components.rarity.as_deref(), Some("epic"));
        assert_eq!(components.hover_text_color.as_deref(), Some("gold"));
        assert_eq!(components.block_placer.as_deref(), Some("test:ore"));
        assert!(parse_components(&compound("", &[])).is_none());
    }

    #[test]
    fn hand_grip_flag_presence_keeps_false_distinct_from_missing_or_malformed() {
        let payload = |flag: Option<Vec<u8>>| {
            compound(
                "",
                &[compound(
                    "components",
                    &flag.into_iter().collect::<Vec<_>>(),
                )],
            )
        };
        let missing = parse_components(&payload(None)).unwrap();
        assert_eq!(missing.hand_equipped, None);
        assert!(!missing.hand_equipped_declared);
        let flat = parse_components(&payload(Some(compound(
            "minecraft:hand_equipped",
            &[byte("value", 0)],
        ))))
        .unwrap();
        assert_eq!(flat.hand_equipped, Some(false));
        assert!(flat.hand_equipped_declared);
        let malformed =
            parse_components(&payload(Some(compound("minecraft:hand_equipped", &[])))).unwrap();
        assert_eq!(malformed.hand_equipped, None);
        assert!(malformed.hand_equipped_declared);
    }
}
