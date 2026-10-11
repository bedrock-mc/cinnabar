//! Versioned status-effect facts shared by retention and presentation.

use crate::HudTextureRole;

/// The effect registry used by the pinned resource pack.
pub fn effect_registry_version() -> &'static str {
    crate::vanilla_source().tag.as_ref()
}

/// One admitted effect, including effects without a persistent HUD icon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectDescriptor {
    pub id: i32,
    pub identifier: &'static str,
    pub icon: Option<HudTextureRole>,
    pub icon_path: Option<&'static str>,
    pub harmful: bool,
}

pub const EFFECT_DESCRIPTORS: [EffectDescriptor; 37] = [
    EffectDescriptor {
        id: 1,
        identifier: "minecraft:speed",
        icon: Some(HudTextureRole::EffectIconSpeed),
        icon_path: Some("textures/ui/speed_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 2,
        identifier: "minecraft:slowness",
        icon: Some(HudTextureRole::EffectIconSlowness),
        icon_path: Some("textures/ui/slowness_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 3,
        identifier: "minecraft:haste",
        icon: Some(HudTextureRole::EffectIconHaste),
        icon_path: Some("textures/ui/haste_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 4,
        identifier: "minecraft:mining_fatigue",
        icon: Some(HudTextureRole::EffectIconMiningFatigue),
        icon_path: Some("textures/ui/mining_fatigue_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 5,
        identifier: "minecraft:strength",
        icon: Some(HudTextureRole::EffectIconStrength),
        icon_path: Some("textures/ui/strength_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 6,
        identifier: "minecraft:instant_health",
        icon: None,
        icon_path: None,
        harmful: false,
    },
    EffectDescriptor {
        id: 7,
        identifier: "minecraft:instant_damage",
        icon: None,
        icon_path: None,
        harmful: true,
    },
    EffectDescriptor {
        id: 8,
        identifier: "minecraft:jump_boost",
        icon: Some(HudTextureRole::EffectIconJumpBoost),
        icon_path: Some("textures/ui/jump_boost_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 9,
        identifier: "minecraft:nausea",
        icon: Some(HudTextureRole::EffectIconNausea),
        icon_path: Some("textures/ui/nausea_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 10,
        identifier: "minecraft:regeneration",
        icon: Some(HudTextureRole::EffectIconRegeneration),
        icon_path: Some("textures/ui/regeneration_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 11,
        identifier: "minecraft:resistance",
        icon: Some(HudTextureRole::EffectIconResistance),
        icon_path: Some("textures/ui/resistance_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 12,
        identifier: "minecraft:fire_resistance",
        icon: Some(HudTextureRole::EffectIconFireResistance),
        icon_path: Some("textures/ui/fire_resistance_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 13,
        identifier: "minecraft:water_breathing",
        icon: Some(HudTextureRole::EffectIconWaterBreathing),
        icon_path: Some("textures/ui/water_breathing_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 14,
        identifier: "minecraft:invisibility",
        icon: Some(HudTextureRole::EffectIconInvisibility),
        icon_path: Some("textures/ui/invisibility_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 15,
        identifier: "minecraft:blindness",
        icon: Some(HudTextureRole::EffectIconBlindness),
        icon_path: Some("textures/ui/blindness_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 16,
        identifier: "minecraft:night_vision",
        icon: Some(HudTextureRole::EffectIconNightVision),
        icon_path: Some("textures/ui/night_vision_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 17,
        identifier: "minecraft:hunger",
        icon: Some(HudTextureRole::EffectIconHunger),
        icon_path: Some("textures/ui/hunger_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 18,
        identifier: "minecraft:weakness",
        icon: Some(HudTextureRole::EffectIconWeakness),
        icon_path: Some("textures/ui/weakness_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 19,
        identifier: "minecraft:poison",
        icon: Some(HudTextureRole::EffectIconPoison),
        icon_path: Some("textures/ui/poison_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 20,
        identifier: "minecraft:wither",
        icon: Some(HudTextureRole::EffectIconWither),
        icon_path: Some("textures/ui/wither_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 21,
        identifier: "minecraft:health_boost",
        icon: Some(HudTextureRole::EffectIconHealthBoost),
        icon_path: Some("textures/ui/health_boost_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 22,
        identifier: "minecraft:absorption",
        icon: Some(HudTextureRole::EffectIconAbsorption),
        icon_path: Some("textures/ui/absorption_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 23,
        identifier: "minecraft:saturation",
        icon: None,
        icon_path: None,
        harmful: false,
    },
    EffectDescriptor {
        id: 24,
        identifier: "minecraft:levitation",
        icon: Some(HudTextureRole::EffectIconLevitation),
        icon_path: Some("textures/ui/levitation_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 25,
        identifier: "minecraft:fatal_poison",
        icon: Some(HudTextureRole::EffectIconPoison),
        icon_path: Some("textures/ui/poison_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 26,
        identifier: "minecraft:conduit_power",
        icon: Some(HudTextureRole::EffectIconConduitPower),
        icon_path: Some("textures/ui/conduit_power_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 27,
        identifier: "minecraft:slow_falling",
        icon: Some(HudTextureRole::EffectIconSlowFalling),
        icon_path: Some("textures/ui/slow_falling_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 28,
        identifier: "minecraft:bad_omen",
        icon: Some(HudTextureRole::EffectIconBadOmen),
        icon_path: Some("textures/ui/bad_omen_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 29,
        identifier: "minecraft:village_hero",
        icon: Some(HudTextureRole::EffectIconVillageHero),
        icon_path: Some("textures/ui/village_hero_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 30,
        identifier: "minecraft:darkness",
        icon: Some(HudTextureRole::EffectIconDarkness),
        icon_path: Some("textures/ui/darkness_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 31,
        identifier: "minecraft:trial_omen",
        icon: Some(HudTextureRole::EffectIconTrialOmen),
        icon_path: Some("textures/ui/trial_omen_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 32,
        identifier: "minecraft:wind_charged",
        icon: Some(HudTextureRole::EffectIconWindCharged),
        icon_path: Some("textures/ui/wind_charged_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 33,
        identifier: "minecraft:weaving",
        icon: Some(HudTextureRole::EffectIconWeaving),
        icon_path: Some("textures/ui/weaving_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 34,
        identifier: "minecraft:oozing",
        icon: Some(HudTextureRole::EffectIconOozing),
        icon_path: Some("textures/ui/oozing_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 35,
        identifier: "minecraft:infested",
        icon: Some(HudTextureRole::EffectIconInfested),
        icon_path: Some("textures/ui/infested_effect.png"),
        harmful: true,
    },
    EffectDescriptor {
        id: 36,
        identifier: "minecraft:raid_omen",
        icon: Some(HudTextureRole::EffectIconRaidOmen),
        icon_path: Some("textures/ui/raid_omen_effect.png"),
        harmful: false,
    },
    EffectDescriptor {
        id: 37,
        identifier: "minecraft:breath_of_the_nautilus",
        icon: Some(HudTextureRole::EffectIconBreathOfTheNautilus),
        icon_path: Some("textures/ui/breath_of_the_nautilus_effect.png"),
        harmful: false,
    },
];

/// Returns the pinned facts for an effect ID, or no facts for an unknown ID.
pub const fn effect_descriptor(id: i32) -> Option<&'static EffectDescriptor> {
    if id < 1 || id as usize > EFFECT_DESCRIPTORS.len() {
        None
    } else {
        Some(&EFFECT_DESCRIPTORS[id as usize - 1])
    }
}

/// Resolves an effect icon role to its sole source path.
pub(crate) const fn effect_icon_path(role: HudTextureRole) -> Option<&'static str> {
    let mut index = 0;
    while index < EFFECT_DESCRIPTORS.len() {
        let descriptor = &EFFECT_DESCRIPTORS[index];
        if let Some(icon) = descriptor.icon {
            if icon as u32 == role as u32 {
                return descriptor.icon_path;
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn effect_registry_covers_the_independent_pinned_inventory() {
        let path = std::env::var_os("PINNED_BEDROCK_EFFECT_INVENTORY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(".local/assets/bedrock-samples")
                    .join(effect_registry_version())
                    .join("full/metadata/vanilladata_modules/mojang-effects.json")
            });
        if !path.is_file() {
            eprintln!(
                "skipping effect_registry_covers_the_independent_pinned_inventory: missing mojang-effects.json fixture at {}",
                path.display()
            );
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "c08ae0f16edb9919eb7ed9165cd94c352883d6521227846cf183a90faed81d69"
        );
        let inventory: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let names: std::collections::BTreeSet<_> = inventory["data_items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect();
        let admitted: std::collections::BTreeSet<_> = EFFECT_DESCRIPTORS
            .iter()
            .map(|descriptor| descriptor.identifier)
            .collect();
        assert_eq!(names, admitted);
        for descriptor in &EFFECT_DESCRIPTORS {
            assert_eq!(effect_descriptor(descriptor.id), Some(descriptor));
            assert_eq!(descriptor.icon.is_some(), descriptor.icon_path.is_some());
            if let Some(icon) = descriptor.icon {
                assert_eq!(icon.source_path(), descriptor.icon_path.unwrap());
            }
        }
    }

    #[test]
    fn recent_effects_have_distinct_carried_icons() {
        let roles: std::collections::BTreeSet<_> = (31..=37)
            .map(|id| effect_descriptor(id).unwrap().icon.unwrap())
            .collect();
        assert_eq!(roles.len(), 7);
        assert!(effect_descriptor(0).is_none());
        assert!(effect_descriptor(38).is_none());
        for id in [6, 7, 23] {
            assert!(effect_descriptor(id).unwrap().icon.is_none());
        }
    }
}
