use std::{fs, path::Path, sync::Arc};

use serde_json::json;

use super::{DESTROY_BLOCK_EVENT, destroy_block_request, level_sound_request};
use crate::audio::{SoundBank, sound_bank_path};
use assets::{FloatRange, RuntimeAudioCatalog, SoundEventTables};

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
#[ignore = "requires installed local carriers (make assets)"]
fn local_pinned_bank_has_audible_grass_and_dirt_break_alternatives_when_present() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let world = root.join(crate::asset_startup::DEFAULT_ASSET_PATH);
    let catalog_path = crate::asset_startup::audio_asset_path(&world);
    let bank_path = sound_bank_path(&world);
    let catalog = RuntimeAudioCatalog::decode(&fs::read(catalog_path).unwrap()).unwrap();
    let mut bank = SoundBank::open(&bank_path, Some(Arc::new(catalog)))
        .unwrap()
        .expect("local pinned bank");
    for identifier in ["minecraft:grass_block", "minecraft:dirt"] {
        let material = bank
            .tables()
            .material_of(identifier)
            .expect("block material");
        let route = bank.tables().block(material, "break").expect("break route");
        let alternatives = bank
            .definition(&route.sound)
            .expect("pack definition")
            .alternatives
            .clone();
        assert!(!alternatives.is_empty());
        for alternative in &alternatives {
            let pcm = bank
                .pcm(&alternative.name, false)
                .expect("decoded break sound");
            assert!(pcm.frames() > 0);
            assert!(pcm.samples.iter().any(|sample| *sample != 0));
        }
    }
}
