//! Client-facing facts a server item's registry components declare.

use std::sync::Arc;

use crate::nbt_tree::{Nbt, read_root};

pub(super) const MAX_TEXT_BYTES: usize = 256;

/// Converts component seconds to the protocol's integral simulation duration.
pub(super) fn duration_ticks(seconds: f64) -> Option<u32> {
    (seconds.is_finite() && seconds >= 0.0).then(|| {
        (seconds * f64::from(simulation_clock::TICKS_PER_SECOND))
            .round()
            .min(f64::from(u32::MAX)) as u32
    })
}

/// What the client presents from one item's components; absent facts stay `None`/`false`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemComponents {
    /// Food permission retained independently from use duration.
    pub food_can_always_eat: Option<bool>,
    /// Whether a valid `minecraft:use_modifiers` compound was declared.
    pub use_modifiers_present: bool,
    /// Finite nonnegative movement multiplier, retained by its IEEE bits.
    pub use_movement_modifier_bits: Option<u64>,
    /// Authored use-category timer, excluding attack cooldowns.
    pub use_cooldown: Option<ItemUseCooldown>,
    /// Parsed fuel duration; missing or malformed declarations stay absent.
    pub fuel_duration_ticks: Option<u32>,
    /// Whether fuel was declared, including malformed component values.
    pub fuel_declared: bool,
    /// Both storage components required to register a dynamic item container are present.
    pub storage_container: bool,
    /// Attack and kinetic presentation facts, when this item declares them.
    pub attack: Option<super::ItemAttackTiming>,
    /// `minecraft:shooter` draw facts, when declared.
    pub shooter: Option<super::ItemShooter>,
    /// `item_texture.json` key from `minecraft:icon`.
    pub icon: Option<Arc<str>>,
    /// `minecraft:display_name` value: a localization key or literal text.
    pub display_name: Option<Arc<str>>,
    /// `foil` / `minecraft:glint`.
    pub glint: Option<bool>,
    /// Presence of an intrinsic-shine declaration, including malformed values.
    pub glint_declared: bool,
    /// Explicit finite hand flag; absence or malformed data remains unresolved.
    pub hand_equipped: Option<bool>,
    /// Distinguishes a malformed declaration from an absent hand component.
    pub hand_equipped_declared: bool,
    /// Explicit offhand permission; absent or malformed values remain unresolved.
    pub allow_off_hand: Option<bool>,
    /// Distinguishes a malformed offhand declaration from an absent one.
    pub allow_off_hand_declared: bool,
    /// Category declared by the legacy Int property, narrowed to its low byte.
    /// Missing or ambiguous item-properties containers stay unresolved.
    pub creative_category: Option<u8>,
    /// Nonnegative maximum from a typed Int; missing or invalid values remain unresolved.
    pub max_durability: Option<u32>,
    /// Retains declaration presence when its maximum is absent or invalid.
    pub durability_declared: bool,
    pub max_stack_size: Option<u8>,
    /// The top-level stack component, independent from item_properties precedence.
    pub max_stack_size_component: Option<u8>,
    /// Duplicate capacity fields or ancestors make the stack capacity unresolved.
    pub stack_capacity_ambiguous: bool,
    /// The components carry an `item_properties` entry, whose stack size a component item takes.
    pub item_properties_declared: bool,
    /// The components carry a top-level `minecraft:max_stack_size` entry.
    pub max_stack_size_component_declared: bool,
    /// `use_animation` by name (`eat`, `drink`, `bow`...); the legacy enum maps 1 and 2.
    pub use_animation: Option<Arc<str>>,
    pub use_duration_ticks: Option<u32>,
    /// Whether `minecraft:food` is declared.
    pub food: bool,
    /// Present only when a food component declares finite numeric nutrition facts.
    pub food_properties: Option<FoodProperties>,
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
    /// `minecraft:digger` destroy speeds, when declared.
    pub digger: Option<Arc<super::ItemDigger>>,
}

/// One authored use cooldown with bounded owned category text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemUseCooldown {
    pub category: Arc<str>,
    pub ticks: u32,
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
            self.use_cooldown
                .as_ref()
                .map(|cooldown| &*cooldown.category),
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
        if let Some(digger) = &self.digger {
            bytes = bytes.saturating_add(digger.retained_bytes());
        }
        bytes
    }
}
/// Retained finite component-defined food values, with no guessed vanilla catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodProperties {
    nutrition: u32,
    saturation_modifier_bits: u32,
}
impl FoodProperties {
    /// Constructs validated component or version-pinned carrier values.
    pub fn new(nutrition: u32, saturation_modifier: f32) -> Option<Self> {
        (saturation_modifier.is_finite() && saturation_modifier >= 0.0).then_some(Self {
            nutrition,
            saturation_modifier_bits: saturation_modifier.to_bits(),
        })
    }
    /// Nutrition points granted when consuming this item.
    pub const fn nutrition(&self) -> u32 {
        self.nutrition
    }
    /// Component multiplier; saturation gained is nutrition × multiplier × 2.
    pub fn saturation_modifier(&self) -> f32 {
        f32::from_bits(self.saturation_modifier_bits)
    }
}
/// Reads only explicit component values; absent or malformed fields remain unavailable.
fn food_properties(component: Option<&Nbt>) -> Option<FoodProperties> {
    let component = component?;
    let nutrition = component.field("nutrition")?.number()?;
    let modifier = component.field("saturation_modifier")?.number()? as f32;
    if !nutrition.is_finite()
        || !(0.0..=u32::MAX as f64).contains(&nutrition)
        || nutrition.fract() != 0.0
        || !modifier.is_finite()
        || modifier < 0.0
    {
        return None;
    }
    Some(FoodProperties {
        nutrition: nutrition as u32,
        saturation_modifier_bits: modifier.to_bits(),
    })
}

/// Keeps absent, malformed and finite flags distinct while retaining the current OR combination.
fn declared_flag(property: Option<&Nbt>, component: Option<&Nbt>) -> (bool, Option<bool>) {
    let read = |value: &Nbt| {
        value
            .field("value")
            .or(Some(value))
            .and_then(Nbt::number)
            .filter(|value| value.is_finite())
            .map(|value| value != 0.0)
    };
    let value = match (property, component) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => read(value),
        (Some(property), Some(component)) => read(property)
            .zip(read(component))
            .map(|(left, right)| left || right),
    };
    (property.is_some() || component.is_some(), value)
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
    let stack_component = components.field("minecraft:max_stack_size");
    let stack_capacity_ambiguous = root.duplicate_field("components")
        || components.duplicate_field("item_properties")
        || components.duplicate_field("minecraft:max_stack_size")
        || properties.is_some_and(|properties| properties.duplicate_field("max_stack_size"))
        || stack_component.is_some_and(|component| component.duplicate_field("value"));
    let property = |name: &str| properties.and_then(|properties| properties.field(name));
    let component = |name: &str| components.field(name);
    let use_modifiers = component("minecraft:use_modifiers")
        .filter(|modifiers| matches!(modifiers, Nbt::Compound(_)));
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
    let use_duration_ticks =
        count(property("use_duration").or(component("minecraft:use_duration"))).or_else(|| {
            use_modifiers
                .and_then(|modifiers| modifiers.field("use_duration"))
                .and_then(Nbt::number)
                .map(|seconds| seconds as f32)
                .and_then(simulation_clock::whole_ticks_from_seconds)
        });
    let glint = declared_flag(
        property("foil").or(component("minecraft:foil")),
        component("minecraft:glint"),
    );
    let hand_equipped = declared_flag(
        property("hand_equipped"),
        component("minecraft:hand_equipped"),
    );
    Some(ItemComponents {
        food_can_always_eat: component("minecraft:food")
            .map(|food| flag(food.field("can_always_eat"))),
        use_modifiers_present: use_modifiers.is_some(),
        use_movement_modifier_bits: use_modifiers
            .and_then(|modifiers| modifiers.field("movement_modifier"))
            .and_then(Nbt::number)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(f64::to_bits),
        use_cooldown: component("minecraft:cooldown").and_then(|cooldown| {
            if cooldown.field("type").and_then(Nbt::as_str) == Some("attack") {
                return None;
            }
            Some(ItemUseCooldown {
                category: text(cooldown.field("category"))?,
                ticks: cooldown
                    .field("duration")
                    .and_then(Nbt::number)
                    .map(|seconds| seconds as f32)
                    .and_then(simulation_clock::whole_ticks_from_seconds)?,
            })
        }),
        fuel_duration_ticks: component("minecraft:fuel")
            .and_then(|fuel| fuel.field("duration"))
            .and_then(Nbt::number)
            .and_then(duration_ticks),
        fuel_declared: component("minecraft:fuel").is_some(),
        storage_container: ["minecraft:storage_item", "minecraft:storage_weight_limit"]
            .into_iter()
            .all(|name| component(name).is_some_and(|value| matches!(value, Nbt::Compound(_)))),
        attack: super::attack::parse_attack(components),
        shooter: super::shooter::parse_shooter(component("minecraft:shooter")),
        icon: super::icons::icon_key(bytes),
        display_name: text(
            component("minecraft:display_name").and_then(|name| name.field("value")),
        ),
        glint: glint.1,
        glint_declared: glint.0,
        hand_equipped: hand_equipped.1,
        hand_equipped_declared: hand_equipped.0,
        durability_declared: component("minecraft:durability").is_some()
            || component("minecraft:max_damage").is_some(),
        allow_off_hand_declared: property("allow_off_hand").is_some()
            || component("minecraft:allow_off_hand").is_some(),
        allow_off_hand: component("minecraft:allow_off_hand")
            .or(property("allow_off_hand"))
            .and_then(|value| match value.field("value").unwrap_or(value) {
                Nbt::Byte(value) => Some(*value != 0),
                Nbt::Short(value) | Nbt::Int(value) | Nbt::Long(value) => Some(*value != 0),
                _ => None,
            }),
        creative_category: super::registry_properties::negotiated_creative_category(bytes),
        max_durability: match component("minecraft:durability") {
            Some(durability) => durability
                .field("max_durability")
                .and_then(Nbt::int32)
                .and_then(|maximum| u32::try_from(maximum).ok()),
            None => count(component("minecraft:max_damage")).filter(|maximum| *maximum > 0),
        },
        max_stack_size: count(property("max_stack_size").or(component("minecraft:max_stack_size")))
            .filter(|size| (1..=255).contains(size))
            .map(|size| size as u8),
        max_stack_size_component: count(component("minecraft:max_stack_size"))
            .filter(|size| (1..=255).contains(size))
            .map(|size| size as u8),
        stack_capacity_ambiguous,
        item_properties_declared: properties.is_some(),
        max_stack_size_component_declared: component("minecraft:max_stack_size").is_some(),
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
        food_properties: food_properties(component("minecraft:food")),
        wearable_slot: text(
            component("minecraft:wearable").and_then(|wearable| wearable.field("slot")),
        ),
        wearable_declared: component("minecraft:wearable").is_some(),
        wearable_protection: component("minecraft:wearable")
            .and_then(|wearable| wearable.field("protection"))
            .and_then(|value| match value {
                Nbt::Short(value) | Nbt::Int(value) | Nbt::Long(value)
                    if (0..=i64::from(i32::MAX)).contains(value) =>
                {
                    Some(*value as u32)
                }
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
        digger: super::digger::parse_digger(component("minecraft:digger")).map(Arc::new),
    })
}

#[cfg(test)]
mod tests {
    mod creative_category;
    use super::parse_components;

    #[test]
    fn declared_use_cooldown_matches_authored_component_precision() {
        let nbt = compound(
            "",
            &[compound(
                "components",
                &[compound(
                    "minecraft:cooldown",
                    &[
                        string("category", "fixture:shared"),
                        float("duration", 0.549),
                    ],
                )],
            )],
        );
        let cooldown = parse_components(&nbt).unwrap().use_cooldown.unwrap();
        assert_eq!(cooldown.category.as_ref(), "fixture:shared");
        assert_eq!(cooldown.ticks, 10);
    }

    #[test]
    fn use_modifier_duration_discards_fractional_ticks() {
        for (seconds, expected) in [
            (0.049, Some(0)),
            (1.049, Some(20)),
            (1.6, Some(32)),
            (f32::NAN, None),
            (f32::INFINITY, None),
            (-1.0, None),
            (f32::MAX, None),
        ] {
            let nbt = compound(
                "",
                &[compound(
                    "components",
                    &[compound(
                        "minecraft:use_modifiers",
                        &[float("use_duration", seconds)],
                    )],
                )],
            );
            assert_eq!(
                parse_components(&nbt).unwrap().use_duration_ticks,
                expected,
                "{seconds}"
            );
        }
    }

    #[test]
    fn use_modifiers_presence_preserves_absent_and_invalid_movement_values() {
        for (declaration, present, movement) in [
            (None, false, None),
            (Some(compound("minecraft:use_modifiers", &[])), true, None),
            (
                Some(compound(
                    "minecraft:use_modifiers",
                    &[float("use_duration", 1.6)],
                )),
                true,
                None,
            ),
            (
                Some(compound(
                    "minecraft:use_modifiers",
                    &[float("movement_modifier", 0.0)],
                )),
                true,
                Some(0.0_f64),
            ),
            (
                Some(compound(
                    "minecraft:use_modifiers",
                    &[float("movement_modifier", 0.75)],
                )),
                true,
                Some(0.75_f64),
            ),
            (
                Some(string("minecraft:use_modifiers", "invalid")),
                false,
                None,
            ),
            (Some(int("minecraft:use_modifiers", 1)), false, None),
            (
                Some(compound(
                    "minecraft:use_modifiers",
                    &[string("movement_modifier", "0.75")],
                )),
                true,
                None,
            ),
            (
                Some(compound(
                    "minecraft:use_modifiers",
                    &[float("movement_modifier", -1.0)],
                )),
                true,
                None,
            ),
            (
                Some(compound(
                    "minecraft:use_modifiers",
                    &[float("movement_modifier", f32::NAN)],
                )),
                true,
                None,
            ),
        ] {
            let declarations: Vec<_> = declaration.into_iter().collect();
            let nbt = compound("", &[compound("components", &declarations)]);
            let facts = parse_components(&nbt).unwrap();
            assert_eq!(facts.use_modifiers_present, present);
            assert_eq!(facts.use_movement_modifier_bits, movement.map(f64::to_bits));
        }
    }

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

    /// Encodes one finite or malformed float component for admission tests.
    fn float(key: &str, value: f32) -> Vec<u8> {
        let mut bytes = name(5, key);
        bytes.extend(value.to_le_bytes());
        bytes
    }
    #[test]
    fn explicit_food_values_are_retained_without_guessing_missing_or_nonfinite_values() {
        let food = |modifier| {
            compound(
                "",
                &[compound(
                    "components",
                    &[compound(
                        "minecraft:food",
                        &[int("nutrition", 4), float("saturation_modifier", modifier)],
                    )],
                )],
            )
        };
        let components = parse_components(&food(0.6)).unwrap();
        let values = components.food_properties.unwrap();
        assert_eq!(values.nutrition(), 4);
        assert_eq!(values.saturation_modifier(), 0.6);
        assert!(
            parse_components(&food(f32::NAN))
                .unwrap()
                .food_properties
                .is_none()
        );
    }

    fn compound(key: &str, entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = name(10, key);
        entries.iter().for_each(|entry| bytes.extend(entry));
        bytes.push(0);
        bytes
    }

    #[test]
    fn custom_content_legacy_components_retain_tool_glint_and_use_facts() {
        let root = compound(
            "",
            &[compound(
                "components",
                &[
                    int("minecraft:max_stack_size", 1),
                    int("minecraft:max_damage", 1562),
                    byte("minecraft:foil", 1),
                    byte("minecraft:hand_equipped", 1),
                    int("minecraft:use_duration", 32),
                ],
            )],
        );
        let facts = parse_components(&root).unwrap();
        assert_eq!(facts.max_stack_size, Some(1));
        assert_eq!(facts.max_durability, Some(1562));
        assert!(facts.durability_declared && facts.max_stack_size_component_declared);
        assert_eq!(facts.glint, Some(true));
        assert_eq!(facts.hand_equipped, Some(true));
        assert_eq!(facts.use_duration_ticks, Some(32));
    }

    #[test]
    fn durability_declaration_retains_absent_invalid_and_nonnegative_int_maxima() {
        let scalar = |tag, payload: &[u8]| {
            let mut field = name(tag, "max_durability");
            field.extend(payload);
            vec![field]
        };
        for (fields, expected) in [
            (vec![], None),
            (scalar(1, &[50]), None),
            (scalar(2, &50_i16.to_le_bytes()), None),
            (scalar(4, &[100]), None),
            (scalar(5, &50_f32.to_le_bytes()), None),
            (scalar(6, &50_f64.to_le_bytes()), None),
            (vec![compound("max_durability", &[int("value", 50)])], None),
            (vec![int("max_durability", 0)], Some(0)),
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
    fn shooter_draw_facts_follow_the_declared_fields() {
        let shooter = |fields: &[Vec<u8>]| {
            let root = compound(
                "",
                &[compound(
                    "components",
                    &[compound("minecraft:shooter", fields)],
                )],
            );
            parse_components(&root).unwrap().shooter.unwrap()
        };
        let drawn = shooter(&[byte("charge_on_draw", 0), float("max_draw_duration", 1.0)]);
        assert!(drawn.starts_held_draw());
        assert!(!shooter(&[]).starts_held_draw());
        assert!(!shooter(&[float("max_draw_duration", f32::NAN)]).starts_held_draw());
        let charged = shooter(&[byte("charge_on_draw", 1), float("max_draw_duration", 1.0)]);
        assert!(charged.charge_on_draw && !charged.starts_held_draw());
        let root = compound("", &[compound("components", &[])]);
        assert_eq!(parse_components(&root).unwrap().shooter, None);
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
        assert!(components.glint == Some(true) && components.hand_equipped == Some(true));
        assert_eq!(components.max_durability, Some(250));
        assert_eq!(components.max_stack_size, Some(16));
        assert!(components.item_properties_declared);
        assert!(!components.max_stack_size_component_declared);
        assert_eq!(components.use_animation.as_deref(), Some("eat"));
        assert_eq!(components.use_duration_ticks, Some(32));
        assert_eq!(components.wearable_slot.as_deref(), Some("slot.armor.head"));
        assert_eq!(components.block_placer.as_deref(), Some("zeqa:crate"));
    }

    #[test]
    fn duplicate_capacity_declarations_remain_ambiguous_without_losing_other_facts() {
        let icon = string("minecraft:icon", "test:gem");
        for fields in [
            vec![
                compound("item_properties", &[]),
                compound("item_properties", &[int("max_stack_size", 1)]),
                icon.clone(),
            ],
            vec![
                compound(
                    "item_properties",
                    &[int("max_stack_size", 64), int("max_stack_size", 1)],
                ),
                icon.clone(),
            ],
            vec![
                compound("minecraft:max_stack_size", &[int("value", 64)]),
                compound("minecraft:max_stack_size", &[int("value", 1)]),
                icon.clone(),
            ],
            vec![
                compound(
                    "minecraft:max_stack_size",
                    &[int("value", 64), int("value", 1)],
                ),
                icon,
            ],
        ] {
            let nbt = compound("", &[compound("components", &fields)]);
            let facts = parse_components(&nbt).unwrap();
            assert!(facts.stack_capacity_ambiguous);
            assert_eq!(facts.icon.as_deref(), Some("test:gem"));
        }
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
        assert!(components.glint == Some(true) && components.hand_equipped == Some(false));
        assert_eq!(components.max_stack_size, Some(1));
        assert!(components.max_stack_size_component_declared);
        assert!(!components.item_properties_declared);
        assert_eq!(components.use_duration_ticks, Some(32));
        assert_eq!(components.use_animation.as_deref(), Some("drink"));
        assert_eq!(components.rarity.as_deref(), Some("epic"));
        assert_eq!(components.hover_text_color.as_deref(), Some("gold"));
        assert_eq!(components.block_placer.as_deref(), Some("test:ore"));
        assert!(parse_components(&compound("", &[])).is_none());
    }

    #[test]
    fn custom_content_stack_component_retains_its_own_value_with_property_precedence() {
        let payload = compound(
            "",
            &[compound(
                "components",
                &[
                    compound("item_properties", &[int("max_stack_size", 64)]),
                    int("minecraft:max_stack_size", 1),
                ],
            )],
        );
        let components = parse_components(&payload).unwrap();
        assert_eq!(components.max_stack_size, Some(64));
        assert_eq!(components.max_stack_size_component, Some(1));
        assert!(!components.stack_capacity_ambiguous);
    }

    #[test]
    fn intrinsic_glint_flags_distinguish_false_missing_and_malformed_data() {
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
        assert_eq!(missing.glint, None);
        assert!(!missing.glint_declared);
        let flat = parse_components(&payload(Some(compound(
            "minecraft:glint",
            &[byte("value", 0)],
        ))))
        .unwrap();
        assert_eq!(flat.glint, Some(false));
        assert!(flat.glint_declared);
        let malformed = parse_components(&payload(Some(compound("minecraft:glint", &[])))).unwrap();
        assert_eq!(malformed.glint, None);
        assert!(malformed.glint_declared);
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

    #[test]
    fn offhand_permission_keeps_denied_missing_and_malformed_distinct() {
        let payload = |entries: &[Vec<u8>]| compound("", &[compound("components", entries)]);
        let absent = parse_components(&payload(&[])).unwrap();
        assert!(!absent.allow_off_hand_declared);
        assert_eq!(absent.allow_off_hand, None);
        for (entry, expected) in [
            (byte("minecraft:allow_off_hand", 1), Some(true)),
            (
                compound("minecraft:allow_off_hand", &[byte("value", 0)]),
                Some(false),
            ),
            (
                compound("item_properties", &[int("allow_off_hand", 1)]),
                Some(true),
            ),
            (string("minecraft:allow_off_hand", "true"), None),
            (float("minecraft:allow_off_hand", 1.0), None),
            (compound("minecraft:allow_off_hand", &[]), None),
        ] {
            let components = parse_components(&payload(&[entry])).unwrap();
            assert!(components.allow_off_hand_declared);
            assert_eq!(components.allow_off_hand, expected);
        }
        let overridden = parse_components(&payload(&[
            compound("item_properties", &[byte("allow_off_hand", 1)]),
            compound("minecraft:allow_off_hand", &[byte("value", 0)]),
        ]))
        .unwrap();
        assert_eq!(overridden.allow_off_hand, Some(false));
    }
    #[test]
    fn dynamic_storage_requires_both_component_objects() {
        for storage in [false, true] {
            for weight in [false, true] {
                let mut fields = Vec::new();
                if storage {
                    fields.push(compound("minecraft:storage_item", &[]));
                }
                if weight {
                    fields.push(compound("minecraft:storage_weight_limit", &[]));
                }
                let payload = compound("", &[compound("components", &fields)]);
                assert_eq!(
                    parse_components(&payload).unwrap().storage_container,
                    storage && weight
                );
            }
        }
        let payload = compound(
            "",
            &[compound(
                "components",
                &[
                    string("minecraft:storage_item", "wrong"),
                    compound("minecraft:storage_weight_limit", &[]),
                ],
            )],
        );
        assert!(!parse_components(&payload).unwrap().storage_container);
    }
}
