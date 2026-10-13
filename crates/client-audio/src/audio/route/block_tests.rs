use std::sync::Arc;

use serde_json::json;

use super::{DESTROY_BLOCK_EVENT, destroy_block_request, level_sound_request};
use assets::{FloatRange, SoundEventTables};

fn block_identifier(id: u32) -> Option<String> {
    match id {
        7 => Some("minecraft:grass_block".to_owned()),
        8 => Some("minecraft:dirt".to_owned()),
        _ => None,
    }
}

#[test]
fn grass_and_dirt_break_events_resolve_the_pack_sound_and_dynamics() {
    // Pinned blocks.json uses the legacy grass key, but the world registry
    // supplies grass_block. Both packet families must reach the same route.
    let tables = SoundEventTables::from_json(
        &json!({"block_sounds": {
            "grass": {"events": {
                "break": {"sound": "dig.grass", "volume": 0.7, "pitch": [0.8, 1.0]},
                "default": ""
            }},
            "gravel": {"events": {
                "break": {"sound": "dig.gravel", "volume": 1.0, "pitch": [0.8, 1.0]},
                "default": ""
            }}
        }}),
        &json!({"grass": "grass", "dirt": "gravel"}),
    );
    for (id, sound, volume) in [(7, "dig.grass", 0.7), (8, "dig.gravel", 1.0)] {
        let event = protocol::LevelAudioEvent {
            sound_event: Arc::from("break"),
            position: [1.5, 2.5, 3.5],
            data: id,
            actor_identifier: Arc::from(""),
            is_baby: false,
            is_global: false,
            actor_unique_id: 0,
            fire_at_position: None,
        };
        let level = level_sound_request(&tables, &event, &block_identifier).expect("block route");
        let destroy = destroy_block_request(
            &tables,
            DESTROY_BLOCK_EVENT,
            event.position,
            id,
            &block_identifier,
        )
        .expect("destroy block route");
        for request in [level, destroy] {
            assert_eq!(&*request.name, sound);
            assert_eq!(request.position, Some(event.position));
            assert_eq!(
                request.volume,
                FloatRange {
                    min: volume,
                    max: volume
                }
            );
            assert_eq!(request.pitch, FloatRange { min: 0.8, max: 1.0 });
        }
    }
    assert!(
        destroy_block_request(&tables, DESTROY_BLOCK_EVENT, [0.0; 3], 9, &block_identifier,)
            .is_none()
    );
}

#[test]
fn dead_bush_break_packets_produce_audible_offline_sources() {
    let tables = SoundEventTables::from_json(
        &json!({"block_sounds": {
            "deadbush": {"base": "grass", "events": {"ambient": "bush.rustle"}},
            "grass": {"events": {"break": {
                "sound": "dig.grass", "volume": 0.7, "pitch": [0.8, 1.0]
            }}}
        }}),
        &json!({"deadbush": "deadbush"}),
    );
    assert_audible_dead_bush_breaks(
        super::super::engine::tests::engine(&[("dig.grass", "block")]),
        &tables,
    );
}

/// Checks both server break routes through the actual mixer source without opening a device.
fn assert_audible_dead_bush_breaks(
    mut engine: super::super::engine::AudioEngine,
    tables: &SoundEventTables,
) -> Vec<f32> {
    use super::super::{engine::Listener, settings::AudioSettings};

    let position = [1.5, 2.5, 3.5];
    let event = protocol::LevelAudioEvent {
        sound_event: Arc::from("break"),
        position,
        data: 7,
        actor_identifier: Arc::from(""),
        is_baby: false,
        is_global: false,
        actor_unique_id: 0,
        fire_at_position: None,
    };
    let identifier = |id| (id == 7).then(|| "minecraft:deadbush".to_owned());
    let requests = [
        level_sound_request(tables, &event, &identifier).expect("dead bush break sound"),
        destroy_block_request(tables, DESTROY_BLOCK_EVENT, position, 7, &identifier)
            .expect("dead bush destroy sound"),
    ];
    let grass = tables.block("grass", "break").expect("grass break route");
    let listener = Listener {
        position,
        right: [1.0, 0.0, 0.0],
    };
    let mut captured = Vec::new();
    for request in requests {
        assert_eq!(&*request.name, &*grass.sound);
        assert_eq!(request.volume, grass.volume);
        assert_eq!(request.pitch, grass.pitch);
        assert_eq!(request.position, Some(position));
        engine.enqueue(request);
        let mut sources = Vec::new();
        test_time::eventually("dead bush sound decoded and started", || {
            sources = engine.pump(Some(listener), 0.0, &AudioSettings::default());
            !sources.is_empty()
        });
        assert_eq!(sources.len(), 1);
        let samples: Vec<_> = sources.pop().unwrap().collect();
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().any(|sample| sample.abs() > 0.0001));
        captured.extend(samples);
    }
    captured
}

#[test]
fn dead_bush_break_uses_loaded_audio_carriers_offline() {
    use super::super::{SoundBank, engine::AudioEngine};

    let (Some(bank_path), Some(catalog_path)) = (
        std::env::var_os("CINNABAR_TEST_AUDIO_BANK"),
        std::env::var_os("CINNABAR_TEST_AUDIO_CATALOG"),
    ) else {
        eprintln!("missing-fixture: CINNABAR_TEST_AUDIO_BANK and CINNABAR_TEST_AUDIO_CATALOG");
        return;
    };
    let catalog = assets::RuntimeAudioCatalog::decode(
        &std::fs::read(catalog_path).expect("audio catalog fixture"),
    )
    .expect("valid audio catalog fixture");
    let bank = SoundBank::open(std::path::Path::new(&bank_path), Some(Arc::new(catalog)))
        .expect("valid audio bank fixture")
        .expect("audio bank fixture present");
    let tables = bank.tables().clone();
    let samples = assert_audible_dead_bush_breaks(AudioEngine::new(Some(bank)), &tables);
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    eprintln!(
        "dead bush offline output: {} stereo samples, peak {peak}",
        samples.len()
    );
    if let Some(path) = std::env::var_os("CINNABAR_TEST_AUDIO_CAPTURE") {
        let bytes: Vec<_> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        std::fs::write(path, bytes).expect("offline f32 stereo capture");
    }
}
