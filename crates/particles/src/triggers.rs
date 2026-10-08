//! Maps protocol particle triggers to effect spawns: legacy particle ids, level events,
//! block break/crack parameters and server-supplied Molang variable maps.

use serde_json::Value;

use super::{
    emitter::{SpawnRequest, TileRequest},
    molang::{Queries, variable_key},
};

/// Level events at or above this bit carry a legacy particle type in the low bits.
pub const LEVEL_EVENT_PARTICLE_FLAG: i32 = 0x4000;

/// Compiled block-destruction definition used by native trigger adapters.
pub const BLOCK_BREAK_EFFECT: &str = "minecraft:block_destruct";

/// Vanilla's default block destruction particle count.
pub const BLOCK_BREAK_PARTICLES: f32 = 100.0;
/// One piece per vanilla hit-particle event.
pub const BLOCK_CRACK_PARTICLES: f32 = 1.0;
/// Default item crumb count for existing eating and icon-crack presentation.
pub const ITEM_ICON_PARTICLES: u32 = 6;
/// Vanilla keeps hits this far from the edge and outside the selected face.
const CRACK_FACE_INSET: f32 = 0.1;
/// Huge explosions use the finite emitter despite its dragon-specific name.
const HUGE_EXPLOSION_EFFECT: &str = "dragon_death_explosion_emitter";

/// Effect identifier for a legacy particle type (`LevelEventParticleLegacyEvent | type`).
#[must_use]
pub fn legacy_particle_effect(particle_type: i32) -> Option<&'static str> {
    Some(match particle_type {
        1 => "basic_bubble_particle",           // Bubble
        2 => "basic_bubble_particle_manual",    // BubbleManual
        3 => "basic_crit_particle",             // Crit
        5 => "basic_smoke_particle",            // Smoke
        6 => "explosion_particle",              // Explode
        7 => "water_evaporation_actor_emitter", // Evaporation
        8 => "basic_flame_particle",            // Flame
        9 => "lava_particle",                   // Lava
        10 => "basic_smoke_particle",           // LargeSmoke
        11 => "redstone_wire_dust_particle",    // RedDust
        12 => "rising_border_dust_particle",    // RisingBorderDust
        16 | 17 => HUGE_EXPLOSION_EFFECT,       // Huge explosions
        18 => "heart_particle",                 // Heart
        20 => "mycelium_dust_particle",         // TownAura
        21 => "basic_portal_particle",          // Portal
        22 => "mob_portal",                     // MobPortal
        23 => "water_splash_particle",          // WaterSplash
        24 => "water_splash_particle_manual",   // WaterSplashManual
        25 => "water_wake_particle",            // WaterWake
        26 => "water_drip_particle",            // DripWater
        27 => "lava_drip_particle",             // DripLava
        28 => "honey_drip_particle",            // DripHoney
        29 => "stalactite_water_drip_particle", // StalactiteDripWater
        30 => "stalactite_lava_drip_particle",  // StalactiteDripLava
        31 => "falling_dust",                   // FallingDust
        32 => "mobspell_emitter",               // MobSpell
        33 => "mobspell_ambient",               // MobSpellAmbient
        34 => "mobspell_emitter",               // MobSpellInstantaneous
        35 => "ink_emitter",                    // Ink
        37 => "rain_splash_particle",           // RainSplash
        38 => "villager_angry",                 // VillagerAngry
        39 => "villager_happy",                 // VillagerHappy
        40 => "enchanting_table_particle",      // EnchantingTable
        42 => "note_particle",                  // Note
        43 => "witchspell_emitter",             // WitchSpell
        44 => "crop_growth_emitter",            // CarrotBoost
        46 => "endrod",                         // EndRod
        47 => "dragon_breath_fire",             // DragonBreath
        48 => "llama_spit_smoke",               // Spit
        49 => "totem_particle",                 // Totem
        54 => "balloon_gas_particle",           // BalloonGas
        55 => "colored_flame_particle",         // ColouredFlame
        56 => "sparkler_emitter",               // Sparkler
        57 => "conduit_particle",               // Conduit
        58 => "bubble_column_up_particle",      // BubbleColumnUp
        59 => "bubble_column_down_particle",    // BubbleColumnDown
        60 => "sneeze",                         // Sneeze
        61 => "shulker_bullet",                 // ShulkerBullet
        62 => "bleach",                         // Bleach
        63 => "dragon_destroy_block",           // DragonDestroyBlock
        64 => "mycelium_dust_particle",         // MyceliumDust
        65 => "falling_border_dust_particle",   // FallingBorderDust
        66 => "campfire_smoke_particle",        // CampfireSmoke
        67 => "campfire_tall_smoke_particle",   // CampfireSmokeTall
        68 => "dragon_breath_fire",             // DragonBreathFire
        69 => "dragon_breath_trail",            // DragonBreathTrail
        70 => "blue_flame_particle",            // BlueFlame
        71 => "soul_particle",                  // Soul
        72 => "obsidian_tear_particle",         // ObsidianTear
        73 => "portal_reverse_particle",        // PortalReverse
        74 => "snowflake_particle",             // Snowflake
        75 => "vibration_signal",               // VibrationSignal
        76 => "sculk_sensor_redstone_particle", // SculkSensorRedstone
        77 => "spore_blossom_shower_particle",  // SporeBlossomShower
        78 => "spore_blossom_ambient_particle", // SporeBlossomAmbient
        79 => "wax_particle",                   // Wax
        80 => "electric_spark_particle",        // ElectricSpark
        81 => "candle_flame_particle",          // CandleFlame
        82 => "shriek_particle",                // Shriek
        83 => "sculk_soul_particle",            // SculkSoul
        84 => "sonic_explosion",                // SonicExplosion
        86 => "cherry_leaves_particle",         // CherryLeaves
        87 => "dust_plume",                     // DustPlume
        88 => "white_smoke_particle",           // WhiteSmoke
        89 => "wind_explosion_emitter",         // WindExplosion
        90 => "breeze_wind_explosion_emitter",  // BreezeWindExplosion
        91 => "vault_connection_particle",      // VaultConnection
        94 => "creaking_crumble_body",          // CreakingCrumble
        95 => "pale_oak_leaves_particle",       // PaleOakLeaves
        96 => "eyeblossom_open",                // EyeblossomOpen
        97 => "eyeblossom_close",               // EyeblossomClose
        98 => "green_flame_particle",           // GreenFlame
        99 => "pause_mob_growth",               // PauseMobGrowth
        100 => "reset_mob_growth",              // ResetMobGrowth
        101 => "sulfur_cube_goo",               // SulfurCube
        _ => return None,
    })
}

/// What a level event asks the particle system to do.
#[derive(Clone, Debug, PartialEq)]
pub enum LevelParticle {
    Named {
        effect: &'static str,
        spell_color: Option<[f32; 4]>,
    },
    /// Block break pieces textured from the block with this network runtime id.
    BlockBreak { runtime_id: i32 },
    /// Block crack pieces from a face of the block being mined.
    BlockCrack { runtime_id: i32, face: u8 },
    /// A legacy `Terrain` particle: block-textured pieces.
    Terrain { runtime_id: i32 },
    /// Item-icon pieces for an item network id and aux value (item break, food crumbs).
    ItemIcon { network_id: i32, aux: i32 },
    /// Item-icon pieces for a fixed item (snowball and slime impacts).
    FixedItemIcon {
        identifier: &'static str,
        count: u32,
    },
}

fn argb(data: i32) -> [f32; 4] {
    let bits = data as u32;
    [
        ((bits >> 16) & 0xff) as f32 / 255.0,
        ((bits >> 8) & 0xff) as f32 / 255.0,
        (bits & 0xff) as f32 / 255.0,
        if bits >> 24 == 0 {
            1.0
        } else {
            (bits >> 24) as f32 / 255.0
        },
    ]
}

/// Classifies a `LevelEvent` id; `None` for events with no particle presentation.
#[must_use]
pub fn classify_level_event(event_id: i32, data: i32) -> Option<LevelParticle> {
    let named = |effect: &'static str| {
        Some(LevelParticle::Named {
            effect,
            spell_color: None,
        })
    };
    if event_id & LEVEL_EVENT_PARTICLE_FLAG != 0 {
        let particle_type = event_id & !LEVEL_EVENT_PARTICLE_FLAG;
        return match particle_type {
            19 => Some(LevelParticle::Terrain { runtime_id: data }),
            13 | 50 => Some(LevelParticle::ItemIcon {
                network_id: data >> 16,
                aux: data & 0xffff,
            }),
            14 => Some(LevelParticle::FixedItemIcon {
                identifier: "minecraft:snowball",
                count: ITEM_ICON_PARTICLES,
            }),
            15 => Some(LevelParticle::FixedItemIcon {
                identifier: "minecraft:snowball",
                count: 1,
            }),
            36 => Some(LevelParticle::FixedItemIcon {
                identifier: "minecraft:slime_ball",
                count: ITEM_ICON_PARTICLES,
            }),
            32..=34 => Some(LevelParticle::Named {
                effect: legacy_particle_effect(particle_type)?,
                spell_color: Some(argb(data)),
            }),
            _ => named(legacy_particle_effect(particle_type)?),
        };
    }
    match event_id {
        2001 | 2021 => Some(LevelParticle::BlockBreak { runtime_id: data }),
        2014 => Some(LevelParticle::BlockCrack {
            runtime_id: data & 0x00ff_ffff,
            face: ((data >> 24) & 0x7) as u8,
        }),
        2002 => Some(LevelParticle::Named {
            effect: "splash_spell_emitter",
            spell_color: Some(argb(data)),
        }),
        2003 => named("eyeofender_death_explode_particle"),
        2004 => named("mob_block_spawn_emitter"),
        2005 => named("crop_growth_emitter"),
        2007 => named("death_explosion_emitter"),
        2009 if data & 0xffff == 15 => Some(LevelParticle::FixedItemIcon {
            identifier: "minecraft:snowball",
            count: 1,
        }),
        2009 => named(legacy_particle_effect(data & 0xffff)?),
        2012 => named("critical_hit_emitter"),
        2013 => named("mob_portal"),
        2015 => named("basic_bubble_particle"),
        2016 => named("water_evaporation_bucket_emitter"),
        2018 | 2019 => named("egg_destroy_emitter"),
        2020 => named("water_evaporation_actor_emitter"),
        2022 => named("knockback_roar_particle"),
        2025 => named(legacy_particle_effect(if data < 2 { 16 } else { 17 })?),
        2027 => named("vibration_signal"),
        2029 => named("misc_fire_vapor_particle"),
        2030 => named("wax_particle"),
        2033 => named("electric_spark_particle"),
        2035 => named("shriek_particle"),
        2037 => named("sculk_charge_particle"),
        2038 => named("sculk_charge_pop_particle"),
        2039 => named("sonic_explosion"),
        2040 => named("dust_plume"),
        3609 => named("white_smoke_particle"),
        3610 => named("breeze_wind_explosion_emitter"),
        3611 => named("trial_spawner_detection"),
        3614 => named("wind_explosion_emitter"),
        _ => None,
    }
}

/// Whether a level event id can produce a particle; lets the protocol layer pre-filter.
#[must_use]
pub fn is_particle_level_event(event_id: i32) -> bool {
    classify_level_event(event_id, 0).is_some()
        || matches!(event_id, 2009)
        || event_id & LEVEL_EVENT_PARTICLE_FLAG != 0
}

fn vec3_members(prefix: &str, value: &Value, out: &mut Vec<(String, f32)>) {
    match value {
        Value::Number(number) => {
            if let Some(number) = number.as_f64() {
                out.push((prefix.to_owned(), number as f32));
            }
        }
        Value::Object(map) => {
            for (member, inner) in map {
                vec3_members(
                    &format!("{prefix}.{}", member.to_ascii_lowercase()),
                    inner,
                    out,
                );
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                let member = ["x", "y", "z", "w"].get(index).copied().unwrap_or("x");
                vec3_members(&format!("{prefix}.{member}"), inner, out);
            }
        }
        _ => {}
    }
}

/// Flattens a server Molang variable map into `(name, value)` pairs without the
/// `variable.` prefix. Accepts `{ "variable.x": value }` and `[ {"name", "value"} ]`
/// layouts; struct values flatten to `name.member`. The wire layout needs native confirmation.
#[must_use]
pub fn parse_molang_variables(json: &str) -> Vec<(String, f32)> {
    const MAX_VARIABLES: usize = 64;
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let entries: Vec<(&str, &Value)> = match &root {
        Value::Object(map) => map.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| Some((item.get("name")?.as_str()?, item.get("value")?)))
            .collect(),
        _ => Vec::new(),
    };
    for (name, value) in entries.into_iter().take(MAX_VARIABLES) {
        let Some(key) = variable_key(name) else {
            continue;
        };
        // Typed wrappers such as {"type": "float", "value": 1.0} unwrap to their value.
        let inner = value.get("value").filter(|_| value.get("type").is_some());
        vec3_members(&key, inner.unwrap_or(value), &mut out);
    }
    out
}

fn variables(pairs: &[(&str, f32)]) -> Vec<(String, f32)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect()
}

/// Spawn request for block break pieces textured from `tile`.
#[must_use]
pub fn block_break_request(
    effect: &str,
    block: [i32; 3],
    tile: TileRequest,
    tint: [f32; 4],
) -> SpawnRequest {
    SpawnRequest {
        effect: effect.to_owned(),
        position: block.map(|c| c as f32 + 0.5),
        variables: variables(&[
            ("emitter_particles_count", BLOCK_BREAK_PARTICLES),
            // Vanilla terrain effects use this exponent.
            ("emitter_intensity", BLOCK_BREAK_PARTICLES.powf(1.0 / 3.0)),
            ("emitter_radius", 0.5),
            ("velocity_scalar", 1.0),
            ("color.r", tint[0]),
            ("color.g", tint[1]),
            ("color.b", tint[2]),
            ("color.a", tint[3]),
        ]),
        tile: Some(tile),
        ..SpawnRequest::default()
    }
}

/// One legacy terrain fragment at the block centre, with no emitter radius.
#[must_use]
pub fn terrain_request(
    effect: &str,
    block: [i32; 3],
    tile: TileRequest,
    tint: [f32; 4],
) -> SpawnRequest {
    let mut request = block_break_request(effect, block, tile, tint);
    for (name, value) in &mut request.variables {
        match name.as_str() {
            "emitter_particles_count" => *value = 1.0,
            "emitter_radius" => *value = 0.0,
            _ => {}
        }
    }
    request
}

/// Item-icon pieces at `position` (item break, eating crumbs, snowball and egg impacts).
#[must_use]
pub fn item_icon_request(position: [f32; 3], tile: TileRequest, count: f32) -> SpawnRequest {
    SpawnRequest {
        effect: "minecraft:breaking_item_icon".to_owned(),
        position,
        variables: variables(&[
            ("num_particles", count),
            ("emitter_radius", 0.25),
            ("size_modifier", 1.0),
            ("speed_modifier", 1.0),
        ]),
        tile: Some(tile),
        ..SpawnRequest::default()
    }
}

/// Spawn request for crack pieces on `face` (0 down, 1 up, 2 north, 3 south, 4 west, 5 east).
#[must_use]
pub fn block_crack_request(
    effect: &str,
    block: [i32; 3],
    face: u8,
    tile: TileRequest,
    tint: [f32; 4],
) -> SpawnRequest {
    let normal: [f32; 3] = match face {
        0 => [0.0, -1.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        2 => [0.0, 0.0, -1.0],
        3 => [0.0, 0.0, 1.0],
        4 => [-1.0, 0.0, 0.0],
        _ => [1.0, 0.0, 0.0],
    };
    let mut request = block_break_request(effect, block, tile, tint);
    for (component, direction) in request.position.iter_mut().zip(normal) {
        *component += direction * (0.5 + CRACK_FACE_INSET);
    }
    request.position_spread = normal.map(|component| {
        if component == 0.0 {
            0.5 - CRACK_FACE_INSET
        } else {
            0.0
        }
    });
    for (name, value) in &mut request.variables {
        match name.as_str() {
            "emitter_particles_count" => *value = BLOCK_CRACK_PARTICLES,
            "emitter_radius" => *value = 0.0,
            "velocity_scalar" => *value = 0.7,
            _ => {}
        }
    }
    request
}

/// Spawn request for a named effect with potion-swirl colour queries.
#[must_use]
pub fn named_request(
    effect: &str,
    position: [f32; 3],
    spell_color: Option<[f32; 4]>,
) -> SpawnRequest {
    SpawnRequest {
        effect: effect.to_owned(),
        position,
        queries: Queries {
            spell_color: spell_color.unwrap_or([1.0; 4]),
            ..Queries::default()
        },
        variables: spell_color
            .map(|c| {
                variables(&[
                    ("color.r", c[0]),
                    ("color.g", c[1]),
                    ("color.b", c[2]),
                    ("color.a", c[3]),
                ])
            })
            .unwrap_or_default(),
        ..SpawnRequest::default()
    }
}

/// Seconds between hit-particle bursts on a block being mined; needs independent measurement.
const CRACK_INTERVAL_SECONDS: f32 = 0.2;
/// Largest server particle count a critical hit may request.
const MAX_CRITICAL_PARTICLES: i32 = 256;

/// Deterministic jitter in `[-0.4, 0.4]` per axis for a burst piece.
fn jitter(seed: u64) -> [f32; 3] {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    std::array::from_fn(|_| {
        state ^= state >> 29;
        state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.8
    })
}

/// One single-piece spawn per burst piece, each jittered around `origin`.
pub fn burst_requests(
    effect: &str,
    origin: [f32; 3],
    pieces: u64,
    seed: u64,
) -> impl Iterator<Item = SpawnRequest> + '_ {
    (0..pieces).map(move |index| {
        let offset = jitter(seed ^ (index << 32));
        SpawnRequest {
            effect: effect.to_owned(),
            position: std::array::from_fn(|i| origin[i] + offset[i]),
            seed: seed.wrapping_add(index),
            ..SpawnRequest::default()
        }
    })
}

/// Face index (0 down, 1 up, 2 north, 3 south, 4 west, 5 east) of `block` nearest the camera.
#[must_use]
pub fn face_toward(block: [i32; 3], camera: [f32; 3]) -> u8 {
    let delta: [f32; 3] = std::array::from_fn(|i| camera[i] - (block[i] as f32 + 0.5));
    let axis = (0..3)
        .max_by(|&a, &b| delta[a].abs().total_cmp(&delta[b].abs()))
        .unwrap_or(1);
    match (axis, delta[axis] >= 0.0) {
        (0, true) => 5,
        (0, false) => 4,
        (1, true) => 1,
        (1, false) => 0,
        (_, true) => 3,
        (_, false) => 2,
    }
}

/// `variable.particle_count` from a critical Animate's data, truncated as vanilla's `(int)` cast;
/// a non-finite value leaves the pack's fallback count in place.
fn critical_particle_variables(particle_count: f32) -> Vec<(String, f32)> {
    if !particle_count.is_finite() {
        return Vec::new();
    }
    let count = (particle_count as i32).clamp(0, MAX_CRITICAL_PARTICLES);
    vec![("particle_count".to_owned(), count as f32)]
}

/// Spawn request for a (magic) critical hit at `position` with the server's particle count.
#[must_use]
pub fn critical_hit_request(magic: bool, position: [f32; 3], particle_count: f32) -> SpawnRequest {
    let effect = if magic {
        "minecraft:magic_critical_hit_emitter"
    } else {
        "minecraft:critical_hit_emitter"
    };
    let mut request = named_request(effect, position, None);
    request.variables = critical_particle_variables(particle_count);
    request
}

/// Advances the crack cadence, keeping the remainder so it does not drift with
/// the frame rate; a long stall yields one burst, not a backlog.
pub fn crack_cadence_due(timer: &mut f32, delta_seconds: f32) -> bool {
    *timer += delta_seconds;
    if *timer < CRACK_INTERVAL_SECONDS {
        return false;
    }
    *timer = (*timer - CRACK_INTERVAL_SECONDS).min(CRACK_INTERVAL_SECONDS);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ids_map_to_vanilla_effects() {
        assert_eq!(legacy_particle_effect(1), Some("basic_bubble_particle"));
        assert_eq!(legacy_particle_effect(8), Some("basic_flame_particle"));
        assert_eq!(legacy_particle_effect(60), Some("sneeze"));
        assert_eq!(legacy_particle_effect(0), None);
    }

    #[test]
    fn level_events_classify_by_kind() {
        assert_eq!(
            classify_level_event(2001, 77),
            Some(LevelParticle::BlockBreak { runtime_id: 77 })
        );
        assert_eq!(
            classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | 8, 0),
            Some(LevelParticle::Named {
                effect: "basic_flame_particle",
                spell_color: None
            })
        );
        assert!(classify_level_event(1000, 0).is_none());
        assert!(matches!(
            classify_level_event(2002, 0x00ff_0000u32 as i32),
            Some(LevelParticle::Named { spell_color: Some(color), .. }) if color[0] == 1.0
        ));
    }

    #[test]
    fn icon_crack_events_split_item_id_and_aux() {
        assert_eq!(
            classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | 13, (300 << 16) | 2),
            Some(LevelParticle::ItemIcon {
                network_id: 300,
                aux: 2
            })
        );
        assert!(matches!(
            classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | 14, 0),
            Some(LevelParticle::FixedItemIcon { .. })
        ));
    }

    #[test]
    fn crack_event_splits_runtime_id_and_face() {
        assert_eq!(
            classify_level_event(2014, (1 << 24) | 5),
            Some(LevelParticle::BlockCrack {
                runtime_id: 5,
                face: 1
            })
        );
    }

    #[test]
    fn molang_maps_flatten_in_both_layouts() {
        let object = parse_molang_variables(
            r#"{"variable.speed": 2.5, "variable.direction": {"x": 1, "y": 2, "z": 3}}"#,
        );
        assert!(object.contains(&("speed".to_owned(), 2.5)));
        assert!(object.contains(&("direction.y".to_owned(), 2.0)));
        let array = parse_molang_variables(
            r#"[{"name": "variable.size", "value": {"type": "float", "value": 4}}]"#,
        );
        assert_eq!(array, vec![("size".to_owned(), 4.0)]);
        assert!(parse_molang_variables("not json").is_empty());
    }

    #[test]
    fn crack_requests_sit_on_the_hit_face_with_fewer_pieces() {
        let tile = TileRequest {
            key: 1,
            size: 16,
            pixels: vec![0; 1024].into(),
        };
        let request = block_crack_request("block_destruct", [0, 0, 0], 1, tile, [1.0; 4]);
        assert!(request.position[1] > 1.0);
        assert!(
            request
                .variables
                .iter()
                .any(|(name, value)| name == "emitter_particles_count"
                    && *value == BLOCK_CRACK_PARTICLES)
        );
    }

    #[test]
    fn destruction_request_drives_the_native_default_burst() {
        use crate::{system::ParticleSystem, world::EmptyWorld};
        let mut system = ParticleSystem::default();
        // Synthetic emitter checks the real request through the live engine.
        assert!(system.register_effect(br#"{"particle_effect":{"description":{
          "identifier":"minecraft:test_terrain","basic_render_parameters":{"material":"particles_alpha","texture":"atlas.terrain"}},
          "components":{"minecraft:emitter_lifetime_once":{"active_time":1},
          "minecraft:emitter_rate_instant":{"num_particles":"variable.emitter_particles_count"},
          "minecraft:particle_appearance_billboard":{"size":[0.1,0.1]},
          "minecraft:particle_lifetime_expression":{"max_lifetime":1}}}}"#));
        let request = block_break_request(
            "minecraft:test_terrain",
            [-2, 3, 4],
            TileRequest {
                key: 1,
                size: 16,
                pixels: vec![255; 16 * 16 * 4].into(),
            },
            [1.0; 4],
        );
        assert_eq!(request.position, [-1.5, 3.5, 4.5]);
        assert!(request.variables.contains(&(
            "emitter_intensity".into(),
            BLOCK_BREAK_PARTICLES.powf(1.0 / 3.0)
        )));
        assert!(system.spawn_terrain(&request).is_some());
        system.tick(0.01, &EmptyWorld);
        assert_eq!(system.live_particles(), BLOCK_BREAK_PARTICLES as usize);
    }

    /// The server's critical count reaches the emitter; unusable data keeps the pack fallback.
    #[test]
    fn critical_hits_bind_the_server_particle_count() {
        let bound = |data| critical_particle_variables(data);
        assert_eq!(bound(12.7), [("particle_count".to_owned(), 12.0)]);
        assert_eq!(bound(0.0), [("particle_count".to_owned(), 0.0)]);
        assert_eq!(bound(1.0e9), [("particle_count".to_owned(), 256.0)]);
        assert!(bound(f32::NAN).is_empty());
    }

    /// Frame times that straddle the interval keep a steady five bursts per second.
    #[test]
    fn crack_cadence_keeps_the_remainder() {
        let mut timer = 0.0;
        let bursts = (0..61)
            .filter(|_| crack_cadence_due(&mut timer, 0.07))
            .count();
        assert_eq!(
            bursts, 21,
            "4.27 s at 0.2 s per burst, not one per three frames"
        );
    }
}
