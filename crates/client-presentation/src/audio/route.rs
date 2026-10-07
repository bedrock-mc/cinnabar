//! Packet sound events to sound requests, resolved through `sounds.json` routing.

use assets::{FloatRange, RouteLookup, SoundEventTables, SoundRoute};
use protocol::{LevelAudioEvent, LevelEventSound, PlayAudioEvent};

use super::engine::SoundRequest;

/// `PlaySoundPacket` positions are fixed-point in eighths of a block.
const PLAY_POSITION_SCALE: f32 = 0.125;
/// Pitch multiplier for baby actors; needs native measurement.
const BABY_PITCH: f32 = 1.5;
const DESTROY_BLOCK_EVENT: i32 = 2001;
/// Legacy door sound, which picks opening or closing at random.
const DOOR_EVENT: i32 = 1003;

/// Sound-only level events: `(id, individual event name, sound definition fallback)`.
const LEVEL_EVENT_SOUNDS: &[(i32, &str, &str)] = &[
    (1000, "block.click", "random.click"),
    (1001, "block.click.fail", "random.click"),
    (1002, "launch", "random.bow"),
    (1004, "fizz", "random.fizz"),
    (1005, "", "random.fuse"),
    (1007, "", "mob.ghast.charge"),
    (1008, "", "mob.ghast.fireball"),
    (1009, "", "mob.blaze.shoot"),
    (1010, "", "mob.zombie.wood"),
    (1012, "", "mob.zombie.woodbreak"),
    (1016, "unfect", "mob.zombie.unfect"),
    (1017, "remedy", "mob.zombie.remedy"),
    (1018, "", "mob.endermen.portal"),
    (1020, "", "random.anvil_break"),
    (1021, "", "random.anvil_use"),
    (1022, "", "random.anvil_land"),
    (1030, "", "random.pop"),
    (1032, "", "mob.endermen.portal"),
    (1040, "", "block.itemframe.add_item"),
    (1041, "", "block.itemframe.break"),
    (1042, "", "block.itemframe.place"),
    (1043, "", "block.itemframe.remove_item"),
    (1044, "", "block.itemframe.rotate_item"),
    (1051, "", "random.orb"),
    (1052, "random.totem", "random.totem"),
    (1060, "", "mob.armor_stand.break"),
    (1061, "", "mob.armor_stand.hit"),
    (1062, "", "mob.armor_stand.land"),
    (1063, "", "mob.armor_stand.place"),
];

/// Note block instruments by the id a `note` event packs above its low data byte.
const NOTE_INSTRUMENTS: [&str; 26] = [
    "note.harp",
    "note.bd",
    "note.snare",
    "note.hat",
    "note.bassattack",
    "note.flute",
    "note.bell",
    "note.guitar",
    "note.chime",
    "note.xylophone",
    "note.iron_xylophone",
    "note.cow_bell",
    "note.didgeridoo",
    "note.bit",
    "note.banjo",
    "note.pling",
    "note.trumpet",
    "note.trumpet_exposed",
    "note.trumpet_weathered",
    "note.trumpet_oxidized",
    "note.zombie",
    "note.skeleton",
    "note.creeper",
    "note.enderdragon",
    "note.witherskeleton",
    "note.piglin",
];
const NOTE_EVENT: &str = "note";

#[cfg(test)]
#[path = "route/block_tests.rs"]
mod block_tests;

fn from_route(route: SoundRoute, position: Option<[f32; 3]>) -> SoundRequest {
    let mut request = SoundRequest::new(route.sound).with_ranges(route.volume, route.pitch);
    request.position = position;
    request
}

/// Explicit `PlaySound`: the server-named definition at the packet's volume and pitch.
pub(super) fn play_request(play: &PlayAudioEvent) -> SoundRequest {
    let pitch = if play.pitch > 0.0 { play.pitch } else { 1.0 };
    let position = play.position.map(|axis| axis as f32 * PLAY_POSITION_SCALE);
    let mut request = SoundRequest::new(play.name.as_ref()).with_ranges(
        FloatRange {
            min: play.volume.max(0.0),
            max: play.volume.max(0.0),
        },
        FloatRange {
            min: pitch,
            max: pitch,
        },
    );
    request.position = Some(position);
    request
}

/// `LevelSoundEvent`: actor route first, then block material, then a named individual sound.
pub(super) fn level_sound_request(
    tables: &SoundEventTables,
    event: &LevelAudioEvent,
    block_identifier: &dyn Fn(u32) -> Option<String>,
) -> Option<SoundRequest> {
    let name = event.sound_event.as_ref();
    let position = (!event.is_global).then_some(event.position);
    let actor = event.actor_identifier.as_ref();
    if name == NOTE_EVENT {
        return note_request(tables, event, position);
    }
    if !actor.is_empty() || matches!(name, "splash" | "swim") {
        match tables.entity_lookup(actor, name, None) {
            RouteLookup::Route(route) => {
                return Some(from_level_route(route, event, position));
            }
            RouteLookup::Silent => return None,
            RouteLookup::Absent => {}
        }
    }
    let material = block_identifier(event.data as u32)
        .and_then(|identifier| tables.material_of(&identifier).map(str::to_owned));
    if let Some(material) = material.as_deref() {
        let mover = if actor.is_empty() { "player" } else { actor };
        let route = tables
            .block(material, name)
            .or_else(|| tables.interactive(mover, name, material));
        if let Some(route) = route {
            return Some(from_level_route(route, event, position));
        }
    }
    tables
        .individual(name)
        .cloned()
        .map(|route| from_level_route(route, event, position))
}

/// Water events carry their actor-computed volume and use the pack pitch without baby scaling.
fn from_level_route(
    route: SoundRoute,
    event: &LevelAudioEvent,
    position: Option<[f32; 3]>,
) -> SoundRequest {
    let mut request = from_route(route, position);
    if matches!(event.sound_event.as_ref(), "splash" | "swim") {
        let actor = event
            .actor_identifier
            .strip_prefix("minecraft:")
            .unwrap_or(&event.actor_identifier);
        if event.sound_event.as_ref() != "splash" || actor != "fishing_hook" {
            let volume = super::water::encoded_volume(event.data);
            request.volume = FloatRange {
                min: volume,
                max: volume,
            };
        }
        request
    } else {
        request.scaled(1.0, if event.is_baby { BABY_PITCH } else { 1.0 })
    }
}

/// A `note` event: data packs `note | instrument << 8`; the note sets pitch in semitones from 12.
fn note_request(
    tables: &SoundEventTables,
    event: &LevelAudioEvent,
    position: Option<[f32; 3]>,
) -> Option<SoundRequest> {
    let instrument = NOTE_INSTRUMENTS.get(usize::try_from(event.data >> 8).ok()?)?;
    let semitones = (event.data & 0xff) - 12;
    let pitch = (semitones as f32 / 12.0).exp2();
    let (volume, base_pitch) = match tables.individual_lookup(NOTE_EVENT) {
        RouteLookup::Route(route) => (route.volume, route.pitch),
        RouteLookup::Absent => (FloatRange::ONE, FloatRange::ONE),
        RouteLookup::Silent => return None,
    };
    let mut request = SoundRequest::new(*instrument)
        .with_ranges(volume, base_pitch)
        .scaled(1.0, pitch);
    request.position = position;
    Some(request)
}

/// Sound-range `LevelEvent`: the individual route when the pack defines one, else the fallback
/// definition; `roll` in `[0, 1)` makes the door event's open-or-close choice.
pub(super) fn level_event_request(
    tables: &SoundEventTables,
    event: &LevelEventSound,
    roll: f32,
) -> Option<SoundRequest> {
    if event.event_id == DOOR_EVENT {
        let name = if roll >= 0.5 {
            "random.door_close"
        } else {
            "random.door_open"
        };
        return Some(SoundRequest::new(name).at(event.position));
    }
    let &(_, individual, fallback) = LEVEL_EVENT_SOUNDS
        .iter()
        .find(|(id, _, _)| *id == event.event_id)?;
    let lookup = if individual.is_empty() {
        RouteLookup::Absent
    } else {
        tables.individual_lookup(individual)
    };
    let request = match lookup {
        RouteLookup::Route(route) => from_route(route, None),
        RouteLookup::Absent => SoundRequest::new(fallback),
        RouteLookup::Silent => return None,
    };
    Some(request.at(event.position))
}

/// Sound definition of a music disc item (`minecraft:music_disc_13` -> `record.13`).
pub(super) fn record_sound_name(item_identifier: &str) -> Option<String> {
    let disc = item_identifier.strip_prefix("minecraft:music_disc_")?;
    (!disc.is_empty()).then(|| format!("record.{disc}"))
}

/// Break sound of the block in `data` for a destroy-block level event; other ids yield `None`.
pub fn destroy_block_request(
    tables: &SoundEventTables,
    event_id: i32,
    position: [f32; 3],
    data: i32,
    block_identifier: &dyn Fn(u32) -> Option<String>,
) -> Option<SoundRequest> {
    if event_id != DESTROY_BLOCK_EVENT {
        return None;
    }
    let identifier = block_identifier(data as u32)?;
    let route = tables.block(tables.material_of(&identifier)?, "break")?;
    Some(from_route(route, Some(position)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn tables() -> SoundEventTables {
        SoundEventTables::from_json(
            &json!({
                "block_sounds": {"stone": {"events": {
                    "break": {"sound": "dig.stone", "volume": 1.0}, "default": ""}}},
                "entity_sounds": {"defaults": {"events": {"hurt": "game.hurt"}},
                    "entities": {"zombie": {"events": {"step": "mob.zombie.step", "hurt": ""}}}},
                "individual_event_sounds": {"events": {"pop": {"sound": "random.pop"}}},
                "interactive_sounds": {"block_sounds": {"stone": {"events": {
                    "step": {"sound": "step.stone", "volume": 0.3}}}}}
            }),
            &json!({"minecraft:stone": "stone"}),
        )
    }

    fn stone(id: u32) -> Option<String> {
        (id == 7).then(|| "minecraft:stone".to_owned())
    }

    fn level(event: &str, actor: &str, data: i32) -> LevelAudioEvent {
        LevelAudioEvent {
            sound_event: Arc::from(event),
            position: [1.0, 2.0, 3.0],
            data,
            actor_identifier: Arc::from(actor),
            is_baby: false,
            is_global: false,
            actor_unique_id: 0,
            fire_at_position: None,
        }
    }

    /// Uses deliberately different route volumes to expose packet-volume replacement.
    fn water_tables() -> SoundEventTables {
        SoundEventTables::from_json(
            &json!({"entity_sounds": {"defaults": {"events": {
                "splash": {"sound": "entity.generic.splash", "volume": 0.8,
                    "pitch": [0.6, 1.4]},
                "swim": {"sound": "random.swim", "volume": 0.8,
                    "pitch": [0.6, 1.4]}}}}}),
            &json!({}),
        )
    }

    #[test]
    fn water_sounds_use_encoded_impact_volume_and_pack_pitch() {
        let tables = water_tables();
        for name in ["splash", "swim"] {
            for data in [
                0,
                838_860,
                8_388_607,
                super::super::water::VOLUME_DATA_SCALE as i32,
            ] {
                for actor in ["", "minecraft:player", "minecraft:zombie"] {
                    let request =
                        level_sound_request(&tables, &level(name, actor, data), &stone).unwrap();
                    let expected = data as f32 / super::super::water::VOLUME_DATA_SCALE;
                    assert_eq!(request.volume.min, expected);
                    assert_eq!(request.volume.max, expected);
                    assert_eq!(request.pitch.min, 0.6);
                    assert_eq!(request.pitch.max, 1.4);
                }
            }
        }
        let mut baby = level("splash", "minecraft:zombie", 0);
        baby.is_baby = true;
        let baby = level_sound_request(&tables, &baby, &stone).unwrap();
        assert_eq!(baby.pitch.min, 0.6);
        assert_eq!(baby.pitch.max, 1.4);
        let hook = level_sound_request(
            &tables,
            &level("splash", "minecraft:fishing_hook", 0),
            &stone,
        )
        .unwrap();
        assert_eq!(hook.volume.min, 0.8);
    }

    #[test]
    fn block_events_resolve_through_the_block_material() {
        let request = level_sound_request(&tables(), &level("break", "", 7), &stone).unwrap();
        assert_eq!(&*request.name, "dig.stone");
        assert_eq!(request.position, Some([1.0, 2.0, 3.0]));
        assert!(level_sound_request(&tables(), &level("break", "", 9), &stone).is_none());
    }

    #[test]
    fn player_steps_use_the_interactive_material_sound() {
        let request =
            level_sound_request(&tables(), &level("step", "minecraft:player", 7), &stone).unwrap();
        assert_eq!(&*request.name, "step.stone");
        assert!((request.volume.min - 0.3).abs() < 1e-6);
    }

    #[test]
    fn entity_routes_win_and_explicit_silence_suppresses_fallbacks() {
        let step =
            level_sound_request(&tables(), &level("step", "minecraft:zombie", 7), &stone).unwrap();
        assert_eq!(&*step.name, "mob.zombie.step");
        assert!(
            level_sound_request(&tables(), &level("hurt", "minecraft:zombie", 0), &stone).is_none()
        );
        let hurt =
            level_sound_request(&tables(), &level("hurt", "minecraft:pig", 0), &stone).unwrap();
        assert_eq!(&*hurt.name, "game.hurt");
    }

    #[test]
    fn global_and_baby_events_adjust_position_and_pitch() {
        let mut event = level("pop", "", 0);
        event.is_global = true;
        event.is_baby = true;
        let request = level_sound_request(&tables(), &event, &stone).unwrap();
        assert_eq!(request.position, None);
        assert_eq!(request.pitch.min, BABY_PITCH);
    }

    // Note data was read as a block runtime id, so instrument and pitch never resolved.
    #[test]
    fn note_events_unpack_instrument_and_semitone_pitch() {
        let request =
            level_sound_request(&tables(), &level("note", "", 24 | (4 << 8)), &stone).unwrap();
        assert_eq!(&*request.name, "note.bassattack");
        assert!((request.pitch.min - 2.0).abs() < 1e-6);
        let low = level_sound_request(&tables(), &level("note", "", 0), &stone).unwrap();
        assert_eq!(&*low.name, "note.harp");
        assert!((low.pitch.min - 0.5).abs() < 1e-6);
        assert!(level_sound_request(&tables(), &level("note", "", 26 << 8), &stone).is_none());
    }

    #[test]
    fn play_sound_positions_are_eighth_block_fixed_point() {
        let play = PlayAudioEvent {
            name: Arc::from("random.click"),
            position: [16, 8, -8],
            volume: 0.5,
            pitch: 0.0,
            loop_count: -1,
            server_sound_handle: None,
        };
        let request = play_request(&play);
        assert_eq!(request.position, Some([2.0, 1.0, -1.0]));
        assert_eq!(request.pitch.min, 1.0);
        assert_eq!(request.volume.max, 0.5);
    }

    #[test]
    fn music_discs_map_to_record_definitions() {
        assert_eq!(
            record_sound_name("minecraft:music_disc_13").as_deref(),
            Some("record.13")
        );
        assert_eq!(record_sound_name("minecraft:stick"), None);
    }

    #[test]
    fn level_events_and_destroy_block_map_to_sounds() {
        let event = LevelEventSound {
            event_id: 1003,
            position: [0.0; 3],
            data: 0,
        };
        assert_eq!(
            &*level_event_request(&tables(), &event, 0.2).unwrap().name,
            "random.door_open"
        );
        assert_eq!(
            &*level_event_request(&tables(), &event, 0.7).unwrap().name,
            "random.door_close",
            "the door event always opened"
        );
        let unknown = LevelEventSound {
            event_id: 1999,
            ..event
        };
        assert!(level_event_request(&tables(), &unknown, 0.0).is_none());
        let destroy = destroy_block_request(&tables(), 2001, [0.0; 3], 7, &stone).unwrap();
        assert_eq!(&*destroy.name, "dig.stone");
        assert!(destroy_block_request(&tables(), 2002, [0.0; 3], 7, &stone).is_none());
    }
}
