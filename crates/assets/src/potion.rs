//! Vanilla potion facts: item appearance, display names and granted effects.

use std::sync::LazyLock;

/// Effect names in the native projectile texture array.
#[derive(Clone, Copy)]
enum Appearance {
    Slowness = 0,
    Speed = 1,
    MiningFatigue = 2,
    Haste = 3,
    Strength = 4,
    Healing = 5,
    Harming = 6,
    JumpBoost = 7,
    Nausea = 8,
    Regeneration = 9,
    Resistance = 10,
    FireResistance = 11,
    WaterBreathing = 12,
    Invisibility = 13,
    Blindness = 14,
    NightVision = 15,
    Hunger = 16,
    Weakness = 17,
    Poison = 18,
    Wither = 19,
    HealthBoost = 20,
    Absorption = 21,
    Saturation = 22,
    TurtleMaster = 24,
    SlowFalling = 25,
    WindCharged = 26,
    Weaving = 27,
    Oozing = 28,
    Infested = 29,
    Default = 30,
}

// The item atlas has its own ordering and omits the projectile's levitation slot.
const ATLAS_APPEARANCES: [Appearance; 30] = {
    use Appearance::*;
    [
        Default,
        Speed,
        Slowness,
        Haste,
        MiningFatigue,
        Strength,
        Healing,
        Harming,
        JumpBoost,
        Nausea,
        Regeneration,
        Resistance,
        FireResistance,
        WaterBreathing,
        Invisibility,
        Blindness,
        NightVision,
        Hunger,
        Weakness,
        Poison,
        Wither,
        HealthBoost,
        Absorption,
        Saturation,
        TurtleMaster,
        SlowFalling,
        WindCharged,
        Weaving,
        Oozing,
        Infested,
    ]
};

static APPEARANCES: LazyLock<[Option<Appearance>; 64]> = LazyLock::new(|| {
    let mut appearances = [None; 64];
    for line in include_str!("../data/legacy-icon-routes-26.30.tsv").lines() {
        let mut columns = line.split('\t');
        if columns.next() != Some(crate::facts::VanillaItem::SplashPotion.identifier()) {
            continue;
        }
        let metadata: usize = columns.next().unwrap().parse().unwrap();
        assert_eq!(columns.next(), Some("potion_bottle_splash"));
        let atlas: usize = columns.next().unwrap().parse().unwrap();
        assert!(
            appearances[metadata].is_none(),
            "duplicate potion appearance route"
        );
        appearances[metadata] = Some(ATLAS_APPEARANCES[atlas]);
    }
    appearances
});

/// Returns the projectile effect index for a known potion auxiliary value.
/// Unknown values have no reviewed item route and return `None`.
pub fn vanilla_potion_variant(auxiliary: i16) -> Option<u8> {
    usize::try_from(auxiliary)
        .ok()
        .and_then(|index| APPEARANCES.get(index).copied().flatten())
        .map(|appearance| appearance as u8)
}

/// One status effect a potion grants when drunk or splashed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PotionEffect {
    /// The effect's registry identifier.
    pub effect: &'static str,
    /// The zero-based amplifier; zero is level I.
    pub amplifier: u8,
    /// Duration in ticks, or `None` for an instant effect.
    pub duration_ticks: Option<u32>,
}

/// A vanilla potion type, indexed by its item auxiliary value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PotionDescriptor {
    /// The potion type's registry identifier.
    pub identifier: &'static str,
    /// The language key stem shared by the drinkable, splash and lingering
    /// names, such as `potion.<stem>.splash.name`.
    pub name_key: &'static str,
    /// Effects in tooltip order; empty for base potions.
    pub effects: &'static [PotionEffect],
}

/// Vanilla potion types, indexed by item auxiliary value. Durations are the
/// drinkable and splash durations.
pub const POTION_DESCRIPTORS: [PotionDescriptor; 47] = [
    PotionDescriptor {
        identifier: "minecraft:water",
        name_key: "emptyPotion",
        effects: &[],
    },
    PotionDescriptor {
        identifier: "minecraft:mundane",
        name_key: "mundane",
        effects: &[],
    },
    PotionDescriptor {
        identifier: "minecraft:long_mundane",
        name_key: "mundane.extended",
        effects: &[],
    },
    PotionDescriptor {
        identifier: "minecraft:thick",
        name_key: "thick",
        effects: &[],
    },
    PotionDescriptor {
        identifier: "minecraft:awkward",
        name_key: "awkward",
        effects: &[],
    },
    PotionDescriptor {
        identifier: "minecraft:nightvision",
        name_key: "nightVision",
        effects: &[PotionEffect {
            effect: "minecraft:night_vision",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_nightvision",
        name_key: "nightVision",
        effects: &[PotionEffect {
            effect: "minecraft:night_vision",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:invisibility",
        name_key: "invisibility",
        effects: &[PotionEffect {
            effect: "minecraft:invisibility",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_invisibility",
        name_key: "invisibility",
        effects: &[PotionEffect {
            effect: "minecraft:invisibility",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:leaping",
        name_key: "jump",
        effects: &[PotionEffect {
            effect: "minecraft:jump_boost",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_leaping",
        name_key: "jump",
        effects: &[PotionEffect {
            effect: "minecraft:jump_boost",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_leaping",
        name_key: "jump",
        effects: &[PotionEffect {
            effect: "minecraft:jump_boost",
            amplifier: 1,
            duration_ticks: Some(1800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:fire_resistance",
        name_key: "fireResistance",
        effects: &[PotionEffect {
            effect: "minecraft:fire_resistance",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_fire_resistance",
        name_key: "fireResistance",
        effects: &[PotionEffect {
            effect: "minecraft:fire_resistance",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:swiftness",
        name_key: "moveSpeed",
        effects: &[PotionEffect {
            effect: "minecraft:speed",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_swiftness",
        name_key: "moveSpeed",
        effects: &[PotionEffect {
            effect: "minecraft:speed",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_swiftness",
        name_key: "moveSpeed",
        effects: &[PotionEffect {
            effect: "minecraft:speed",
            amplifier: 1,
            duration_ticks: Some(1800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:slowness",
        name_key: "moveSlowdown",
        effects: &[PotionEffect {
            effect: "minecraft:slowness",
            amplifier: 0,
            duration_ticks: Some(1800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_slowness",
        name_key: "moveSlowdown",
        effects: &[PotionEffect {
            effect: "minecraft:slowness",
            amplifier: 0,
            duration_ticks: Some(4800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:water_breathing",
        name_key: "waterBreathing",
        effects: &[PotionEffect {
            effect: "minecraft:water_breathing",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_water_breathing",
        name_key: "waterBreathing",
        effects: &[PotionEffect {
            effect: "minecraft:water_breathing",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:healing",
        name_key: "heal",
        effects: &[PotionEffect {
            effect: "minecraft:instant_health",
            amplifier: 0,
            duration_ticks: None,
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_healing",
        name_key: "heal",
        effects: &[PotionEffect {
            effect: "minecraft:instant_health",
            amplifier: 1,
            duration_ticks: None,
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:harming",
        name_key: "harm",
        effects: &[PotionEffect {
            effect: "minecraft:instant_damage",
            amplifier: 0,
            duration_ticks: None,
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_harming",
        name_key: "harm",
        effects: &[PotionEffect {
            effect: "minecraft:instant_damage",
            amplifier: 1,
            duration_ticks: None,
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:poison",
        name_key: "poison",
        effects: &[PotionEffect {
            effect: "minecraft:poison",
            amplifier: 0,
            duration_ticks: Some(900),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_poison",
        name_key: "poison",
        effects: &[PotionEffect {
            effect: "minecraft:poison",
            amplifier: 0,
            duration_ticks: Some(2400),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_poison",
        name_key: "poison",
        effects: &[PotionEffect {
            effect: "minecraft:poison",
            amplifier: 1,
            duration_ticks: Some(450),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:regeneration",
        name_key: "regeneration",
        effects: &[PotionEffect {
            effect: "minecraft:regeneration",
            amplifier: 0,
            duration_ticks: Some(900),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_regeneration",
        name_key: "regeneration",
        effects: &[PotionEffect {
            effect: "minecraft:regeneration",
            amplifier: 0,
            duration_ticks: Some(2400),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_regeneration",
        name_key: "regeneration",
        effects: &[PotionEffect {
            effect: "minecraft:regeneration",
            amplifier: 1,
            duration_ticks: Some(450),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strength",
        name_key: "damageBoost",
        effects: &[PotionEffect {
            effect: "minecraft:strength",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_strength",
        name_key: "damageBoost",
        effects: &[PotionEffect {
            effect: "minecraft:strength",
            amplifier: 0,
            duration_ticks: Some(9600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_strength",
        name_key: "damageBoost",
        effects: &[PotionEffect {
            effect: "minecraft:strength",
            amplifier: 1,
            duration_ticks: Some(1800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:weakness",
        name_key: "weakness",
        effects: &[PotionEffect {
            effect: "minecraft:weakness",
            amplifier: 0,
            duration_ticks: Some(1800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_weakness",
        name_key: "weakness",
        effects: &[PotionEffect {
            effect: "minecraft:weakness",
            amplifier: 0,
            duration_ticks: Some(4800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:wither",
        name_key: "wither",
        effects: &[PotionEffect {
            effect: "minecraft:wither",
            amplifier: 1,
            duration_ticks: Some(800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:turtle_master",
        name_key: "turtleMaster",
        effects: &[
            PotionEffect {
                effect: "minecraft:slowness",
                amplifier: 3,
                duration_ticks: Some(400),
            },
            PotionEffect {
                effect: "minecraft:resistance",
                amplifier: 2,
                duration_ticks: Some(400),
            },
        ],
    },
    PotionDescriptor {
        identifier: "minecraft:long_turtle_master",
        name_key: "turtleMaster",
        effects: &[
            PotionEffect {
                effect: "minecraft:slowness",
                amplifier: 3,
                duration_ticks: Some(800),
            },
            PotionEffect {
                effect: "minecraft:resistance",
                amplifier: 2,
                duration_ticks: Some(800),
            },
        ],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_turtle_master",
        name_key: "turtleMaster",
        effects: &[
            PotionEffect {
                effect: "minecraft:slowness",
                amplifier: 5,
                duration_ticks: Some(400),
            },
            PotionEffect {
                effect: "minecraft:resistance",
                amplifier: 3,
                duration_ticks: Some(400),
            },
        ],
    },
    PotionDescriptor {
        identifier: "minecraft:slow_falling",
        name_key: "slowFalling",
        effects: &[PotionEffect {
            effect: "minecraft:slow_falling",
            amplifier: 0,
            duration_ticks: Some(1800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:long_slow_falling",
        name_key: "slowFalling",
        effects: &[PotionEffect {
            effect: "minecraft:slow_falling",
            amplifier: 0,
            duration_ticks: Some(4800),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:strong_slowness",
        name_key: "moveSlowdown",
        effects: &[PotionEffect {
            effect: "minecraft:slowness",
            amplifier: 3,
            duration_ticks: Some(400),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:wind_charged",
        name_key: "windCharged",
        effects: &[PotionEffect {
            effect: "minecraft:wind_charged",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:weaving",
        name_key: "weaving",
        effects: &[PotionEffect {
            effect: "minecraft:weaving",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:oozing",
        name_key: "oozing",
        effects: &[PotionEffect {
            effect: "minecraft:oozing",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
    PotionDescriptor {
        identifier: "minecraft:infested",
        name_key: "infested",
        effects: &[PotionEffect {
            effect: "minecraft:infested",
            amplifier: 0,
            duration_ticks: Some(3600),
        }],
    },
];

/// Returns the vanilla potion type for an item auxiliary value.
pub fn potion_descriptor(auxiliary: i16) -> Option<&'static PotionDescriptor> {
    usize::try_from(auxiliary)
        .ok()
        .and_then(|index| POTION_DESCRIPTORS.get(index))
}

/// The language key naming a tipped arrow: its aux is one more than the potion it carries, and
/// aux 0 is the plain arrow. A water arrow is "Splashing"; a long mundane one is mundane.
#[must_use]
pub fn tipped_arrow_name_key(auxiliary: u32) -> Option<String> {
    let potion = i16::try_from(auxiliary.checked_sub(1)?).ok()?;
    let stem = match potion_descriptor(potion)?.name_key {
        "emptyPotion" => "water",
        "mundane.extended" => "mundane",
        key => key,
    };
    Some(format!("tipped_arrow.effect.{stem}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::*;

    #[test]
    fn tipped_arrows_name_the_potion_they_carry() {
        assert_eq!(tipped_arrow_name_key(0), None);
        assert_eq!(
            tipped_arrow_name_key(1).as_deref(),
            Some("tipped_arrow.effect.water")
        );
        assert_eq!(
            tipped_arrow_name_key(3).as_deref(),
            Some("tipped_arrow.effect.mundane")
        );
        // Potion 14 is swiftness.
        assert_eq!(
            tipped_arrow_name_key(15).as_deref(),
            Some("tipped_arrow.effect.moveSpeed")
        );
        assert_eq!(
            tipped_arrow_name_key(46).as_deref(),
            Some("tipped_arrow.effect.oozing")
        );
        assert_eq!(tipped_arrow_name_key(999), None);
    }

    fn pinned_pack_file(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(crate::vanilla_source().cache_dir.as_ref())
            .join(relative)
    }

    #[test]
    fn potion_effects_name_admitted_effects() {
        for potion in &POTION_DESCRIPTORS {
            for effect in potion.effects {
                assert!(
                    crate::effect_descriptor_by_identifier(effect.effect).is_some(),
                    "{} grants unknown effect {}",
                    potion.identifier,
                    effect.effect
                );
            }
        }
        assert!(potion_descriptor(-1).is_none());
        assert!(potion_descriptor(POTION_DESCRIPTORS.len() as i16).is_none());
    }

    #[test]
    fn potion_table_covers_the_pinned_potion_inventory() {
        let path = pinned_pack_file("metadata/vanilladata_modules/mojang-potion-effects.json");
        if !path.is_file() {
            eprintln!(
                "skipping potion_table_covers_the_pinned_potion_inventory: missing mojang-potion-effects.json fixture at {}",
                path.display()
            );
            return;
        }
        let inventory: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let names: BTreeSet<_> = inventory["data_items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect();
        let admitted: BTreeSet<_> = POTION_DESCRIPTORS
            .iter()
            .map(|potion| potion.identifier)
            .collect();
        assert_eq!(names, admitted);
    }

    #[test]
    fn potion_and_effect_names_exist_in_the_pinned_language() {
        let path = pinned_pack_file("resource_pack/texts/en_US.lang");
        if !path.is_file() {
            eprintln!(
                "skipping potion_and_effect_names_exist_in_the_pinned_language: missing en_US.lang fixture at {}",
                path.display()
            );
            return;
        }
        let text = std::fs::read_to_string(path).unwrap();
        let keys: BTreeSet<_> = text
            .lines()
            .filter_map(|line| line.split_once('=').map(|(key, _)| key))
            .collect();
        for potion in &POTION_DESCRIPTORS {
            for form in ["", ".splash", ".linger"] {
                let key = format!("potion.{}{form}.name", potion.name_key);
                assert!(keys.contains(key.as_str()), "missing {key}");
            }
        }
        for effect in &crate::EFFECT_DESCRIPTORS {
            assert!(
                keys.contains(effect.name_key),
                "missing {}",
                effect.name_key
            );
        }
    }
}
