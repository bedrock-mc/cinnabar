//! What each item does on an air use beyond the click-air transaction every item sends.

/// Bow and trident maximum use duration.
const LONG_USE_TICKS: u32 = 72_000;
const SPYGLASS_USE_TICKS: u32 = 1_200;
/// Crossbow charge: 25 ticks less 5 per Quick Charge level.
const CROSSBOW_CHARGE_TICKS: u32 = 25;
const QUICK_CHARGE_TICKS_PER_LEVEL: u32 = 5;
/// Drink duration of potions, ominous bottles and milk buckets.
const DRINK_TICKS: u32 = 32;
/// Vanilla's movement factor for a use without `minecraft:use_modifiers`.
const DEFAULT_USE_SLOWDOWN: f64 = 0.35;
/// The vanilla spears' `use_modifiers.movement_modifier`.
const SPEAR_USE_SLOWDOWN: f64 = 1.0;
/// Ender pearl cooldown.
const ENDER_PEARL_COOLDOWN: Cooldown = Cooldown {
    category: "ender_pearl",
    ticks: 20,
};
/// The vanilla pack's `wind_charge` `minecraft:cooldown` (0.5 s).
const WIND_CHARGE_COOLDOWN: Cooldown = Cooldown {
    category: "wind_charge",
    ticks: 10,
};
/// Vanilla foods whose `minecraft:food` sets `can_always_eat`.
const ALWAYS_EDIBLE: &[&str] = &[
    "enchanted_golden_apple",
    "chorus_fruit",
    "golden_apple",
    "honey_bottle",
    "suspicious_stew",
];
/// Wire names whose vanilla behavior-pack file keeps a legacy identifier.
const PACK_ALIASES: &[(&str, &str)] = &[
    (
        "minecraft:enchanted_golden_apple",
        "minecraft:appleEnchanted",
    ),
    ("minecraft:cooked_mutton", "minecraft:muttonCooked"),
    ("minecraft:mutton", "minecraft:muttonRaw"),
    ("minecraft:tropical_fish", "minecraft:clownfish"),
    ("minecraft:cod", "minecraft:fish"),
    ("minecraft:cooked_cod", "minecraft:cooked_fish"),
];

/// What a use needs before it starts outside creative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    Arrow,
    /// Arrows anywhere, or a firework rocket in the offhand.
    ArrowOrOffhandRocket,
    /// Food points below full, as vanilla food requires.
    Appetite,
}

/// A use's shared cooldown category and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cooldown {
    pub category: &'static str,
    pub ticks: u32,
}

/// What pressing use in the air does with the selected item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AirUse {
    /// Starts a use that ends on release, or silently once `max_ticks` run out.
    Hold {
        max_ticks: u32,
        needs: Needs,
        slowdown: f64,
    },
    /// Consumes one item outside creative and swings, as a thrown projectile does.
    Throw { cooldown: Option<Cooldown> },
    /// Acts at once, as a loaded crossbow fires.
    Instant,
}

impl AirUse {
    const fn hold(max_ticks: u32, needs: Needs) -> Self {
        Self::Hold {
            max_ticks,
            needs,
            slowdown: DEFAULT_USE_SLOWDOWN,
        }
    }

    pub const fn cooldown(self) -> Option<Cooldown> {
        match self {
            Self::Throw { cooldown } => cooldown,
            Self::Hold { .. } | Self::Instant => None,
        }
    }

    /// A crossbow acts only on a fresh press; everything else repeats while use is held.
    pub const fn repeats_while_held(self) -> bool {
        !matches!(
            self,
            Self::Instant
                | Self::Hold {
                    needs: Needs::ArrowOrOffhandRocket,
                    ..
                }
        )
    }
}

/// Whether a vanilla hold use is eaten or drunk (eat/drink use animation), which the
/// first-person pass raises to the mouth.
pub fn is_consumed(identifier: &str) -> bool {
    identifier.strip_prefix("minecraft:").is_some_and(|name| {
        !matches!(name, "bow" | "trident" | "spyglass" | "crossbow" | "camera")
            && !name.ends_with("_spear")
    })
}

/// The behavior-pack identifier a wire identifier's use duration is filed under.
pub fn pack_identifier(identifier: &str) -> Option<&'static str> {
    PACK_ALIASES
        .iter()
        .find(|(wire, _)| *wire == identifier)
        .map(|(_, pack)| *pack)
}

/// The air use of `identifier`; `pack_ticks` is the use duration its pack or item components
/// state. `None` sends only the click-air transaction.
pub fn classify(
    identifier: &str,
    charged: bool,
    quick_charge: u8,
    pack_ticks: Option<u32>,
) -> Option<AirUse> {
    let Some(name) = identifier.strip_prefix("minecraft:") else {
        // A custom item with a stated use duration holds like vanilla `use_modifiers`.
        return pack_ticks.map(|ticks| AirUse::hold(ticks, Needs::Nothing));
    };
    Some(match name {
        "bow" => AirUse::hold(LONG_USE_TICKS, Needs::Arrow),
        "trident" => AirUse::hold(LONG_USE_TICKS, Needs::Nothing),
        "spyglass" => AirUse::hold(SPYGLASS_USE_TICKS, Needs::Nothing),
        "crossbow" if charged => AirUse::Instant,
        "crossbow" => AirUse::hold(
            CROSSBOW_CHARGE_TICKS
                .saturating_sub(u32::from(quick_charge) * QUICK_CHARGE_TICKS_PER_LEVEL),
            Needs::ArrowOrOffhandRocket,
        ),
        "potion" | "ominous_bottle" | "milk_bucket" => AirUse::hold(DRINK_TICKS, Needs::Nothing),
        "snowball" | "egg" | "blue_egg" | "brown_egg" | "experience_bottle" | "splash_potion"
        | "lingering_potion" => AirUse::Throw { cooldown: None },
        "ender_pearl" => AirUse::Throw {
            cooldown: Some(ENDER_PEARL_COOLDOWN),
        },
        "wind_charge" => AirUse::Throw {
            cooldown: Some(WIND_CHARGE_COOLDOWN),
        },
        // Placed with its block; its pack use duration is not an air use.
        "camera" => return None,
        _ if name.ends_with("_spear") => AirUse::Hold {
            max_ticks: pack_ticks?,
            needs: Needs::Nothing,
            slowdown: SPEAR_USE_SLOWDOWN,
        },
        // Every other vanilla item with a pack use duration is a food.
        _ => AirUse::hold(
            pack_ticks?,
            if ALWAYS_EDIBLE.contains(&name) {
                Needs::Nothing
            } else {
                Needs::Appetite
            },
        ),
    })
}
