//! Local inventory gesture feedback, resolved through the active pack's sound routes.

use assets::SoundEventTables;
use bevy::prelude::{Res, ResMut};
use inventory::PlayerInventoryLedger;

use super::engine::{AudioEngine, SoundRequest};
use crate::{local_player::LocalViewPose, observations::WorldObservation};

fn take_drop_requests(
    ledger: &mut PlayerInventoryLedger,
    tables: Option<&SoundEventTables>,
    position: Option<[f32; 3]>,
) -> impl Iterator<Item = SoundRequest> + use<> {
    let count = ledger.take_world_drops();
    let request = if count == 0 {
        None
    } else {
        position
            .filter(|position| position.iter().all(|axis| axis.is_finite()))
            .zip(tables.and_then(|tables| tables.individual("drop.slot")))
            .map(|(position, route)| {
                SoundRequest::new(route.sound.as_ref())
                    .with_ranges(route.volume, route.pitch)
                    .at(position)
            })
    };
    (0..count).filter_map(move |_| request.clone())
}

/// Voices each admitted world drop immediately; server replies do not repeat its feedback.
pub fn drive_inventory_audio(
    player: &mut player_state::PlayerState,
    world: WorldObservation<'_>,
    view: Res<LocalViewPose>,
    mut engine: ResMut<AudioEngine>,
) {
    let position = world.stream.map(|_| view.eye_translation().to_array());
    let requests = take_drop_requests(
        player.inventory.ledger_mut(),
        engine.bank().map(|bank| bank.tables()),
        position,
    );
    for request in requests {
        engine.enqueue(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{
        InventoryAuthority, InventoryContentEvent, InventoryEvent, NetworkItemStack,
        PLAYER_INVENTORY_WINDOW_ID,
    };

    fn ledger() -> PlayerInventoryLedger {
        let mut ledger = PlayerInventoryLedger::default();
        ledger.begin_session(1);
        ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: protocol::ContainerIdentity::window(PLAYER_INVENTORY_WINDOW_ID),
            slots: vec![NetworkItemStack {
                network_id: 1,
                stack_network_id: 100,
                count: 3,
                ..NetworkItemStack::default()
            }]
            .into(),
            storage_item: NetworkItemStack::empty(),
        }));
        ledger
    }

    fn tables(sound: &str) -> SoundEventTables {
        SoundEventTables::from_json(
            &serde_json::json!({
                "individual_event_sounds": {"events": {
                    "drop.slot": {"sound": sound, "volume": 0.3, "pitch": [0.55, 0.75]},
                    "pop": {"sound": "pickup", "volume": 0.25, "pitch": [0.6, 2.2]}
                }}
            }),
            &serde_json::json!({}),
        )
    }

    #[test]
    fn admitted_single_and_stack_drops_use_the_drop_route_once() {
        let mut ledger = ledger();
        ledger.begin_world_drop(0, Some(1)).unwrap();
        ledger.begin_world_drop(0, None).unwrap();
        let position = [12.0, 64.0, -8.0];
        let tables = tables("random.pop");
        let requests: Vec<_> =
            take_drop_requests(&mut ledger, Some(&tables), Some(position)).collect();
        assert_eq!(requests.len(), 2);
        for request in requests {
            assert_eq!(request.name.as_ref(), "random.pop");
            assert_eq!(request.position, Some(position));
            assert_eq!(request.volume, assets::FloatRange { min: 0.3, max: 0.3 });
            assert_eq!(
                request.pitch,
                assets::FloatRange {
                    min: 0.55,
                    max: 0.75
                }
            );
            assert!(!request.looping);
        }
        assert_eq!(
            take_drop_requests(&mut ledger, Some(&tables), Some(position)).count(),
            0
        );
    }

    #[test]
    fn drop_feedback_respects_pack_overrides_and_explicit_silence() {
        for (sound, count) in [("server.drop", 1), ("", 0)] {
            let mut ledger = ledger();
            ledger.begin_world_drop(0, Some(1)).unwrap();
            let requests: Vec<_> =
                take_drop_requests(&mut ledger, Some(&tables(sound)), Some([0.0; 3])).collect();
            assert_eq!(requests.len(), count);
            if let Some(request) = requests.first() {
                assert_eq!(request.name.as_ref(), sound);
            }
        }
    }

    #[test]
    fn unavailable_world_or_audio_does_not_replay_drops_later() {
        let tables = tables("random.pop");
        for (tables, position) in [
            (Some(&tables), None),
            (None, Some([0.0; 3])),
            (Some(&tables), Some([f32::NAN; 3])),
        ] {
            let mut ledger = ledger();
            ledger.begin_world_drop(0, Some(1)).unwrap();
            assert_eq!(take_drop_requests(&mut ledger, tables, position).count(), 0);
            assert_eq!(ledger.take_world_drops(), 0);
        }
    }

    #[test]
    fn local_pinned_drop_route_reaches_the_mixer_when_bank_is_present() {
        use super::super::{
            bank::SOUND_BANK_FILENAME, engine::Listener, settings::AudioSettings,
            voice::OUTPUT_RATE,
        };
        use crate::named_audio::AudioDevice;
        use std::path::{Path, PathBuf};

        let path = std::env::var_os("CINNABAR_TEST_SOUND_BANK")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/assets/compiled")
                    .join(SOUND_BANK_FILENAME)
            });
        if !path.is_file() {
            eprintln!(
                "skipping local_pinned_drop_route_reaches_the_mixer_when_bank_is_present: missing sound bank {}; run make audio-bank-assets",
                path.display()
            );
            return;
        }
        let Some(catalog_path) = std::env::var_os("CINNABAR_TEST_AUDIO_CATALOG") else {
            eprintln!(
                "skipping local_pinned_drop_route_reaches_the_mixer_when_bank_is_present: missing CINNABAR_TEST_AUDIO_CATALOG fixture path"
            );
            return;
        };
        let catalog_path = PathBuf::from(catalog_path);
        if !catalog_path.is_file() {
            eprintln!(
                "skipping local_pinned_drop_route_reaches_the_mixer_when_bank_is_present: missing audio catalog {}",
                catalog_path.display()
            );
            return;
        }
        let catalog =
            assets::RuntimeAudioCatalog::decode(&std::fs::read(catalog_path).unwrap()).unwrap();
        let mut bank = super::super::SoundBank::open(&path, Some(std::sync::Arc::new(catalog)))
            .unwrap()
            .expect("local sound bank");
        let route = bank.tables().individual("drop.slot").expect("drop route");
        assert_eq!(route.sound.as_ref(), "random.pop");
        assert_eq!(route.volume, assets::FloatRange { min: 0.3, max: 0.3 });
        assert_eq!(
            route.pitch,
            assets::FloatRange {
                min: 0.55,
                max: 0.75
            }
        );
        let definition = bank.definition(&route.sound).expect("drop definition");
        assert_eq!(definition.category.as_deref(), Some("player"));
        let alternatives = definition.alternatives.clone();
        for alternative in &alternatives {
            let pcm = bank
                .pcm(&alternative.name, false)
                .expect("decoded drop sound");
            assert!(pcm.frames() > 0);
            assert!(pcm.samples.iter().any(|sample| *sample != 0));
        }
        let position = [0.0; 3];
        let listener = Some(Listener {
            position,
            right: [1.0, 0.0, 0.0],
        });
        let mut ledger = ledger();
        ledger.begin_world_drop(0, Some(1)).unwrap();
        ledger.begin_world_drop(0, None).unwrap();
        let mut engine = AudioEngine::new(Some(bank));
        for request in take_drop_requests(
            &mut ledger,
            engine.bank().map(|bank| bank.tables()),
            Some(position),
        ) {
            engine.enqueue(request);
        }
        let settings = AudioSettings::default();
        let sources = engine.pump(listener, 0.0, &settings);
        assert_eq!(sources.len(), 2);
        let (mut device, mixer) = AudioDevice::memory_mixer();
        for source in sources {
            assert!(device.play_source(source));
        }
        assert!(
            mixer
                .take(OUTPUT_RATE as usize)
                .any(|sample| sample.abs() > 0.0)
        );
        assert!(engine.pump(listener, 0.0, &settings).is_empty());
        assert_eq!(engine.stats.started, 2);

        if std::env::var_os("CINNABAR_TEST_AUDIO_DEVICE").is_some() {
            let mut hardware = AudioDevice::open_default_once();
            assert!(hardware.available(), "default audio output unavailable");
            ledger = self::ledger();
            ledger.begin_world_drop(0, Some(1)).unwrap();
            for request in take_drop_requests(
                &mut ledger,
                engine.bank().map(|bank| bank.tables()),
                Some(position),
            ) {
                engine.enqueue(request);
            }
            let sources = engine.pump(listener, 0.0, &settings);
            assert_eq!(sources.len(), 1);
            assert!(hardware.play_source(sources.into_iter().next().unwrap()));
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
