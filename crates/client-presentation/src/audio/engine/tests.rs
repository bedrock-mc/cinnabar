use super::*;
use crate::audio::voice::Pcm;
use assets::{
    AudioAlternative, AudioDefinition, RuntimeAudioCatalog, SoundBankIndex, SoundEventTables,
};

fn definition(name: &str, category: &str) -> AudioDefinition {
    AudioDefinition {
        identifier: name.into(),
        category: Some(category.into()),
        subtitle: None,
        min_distance: None,
        max_distance: None,
        volume: None,
        pitch: None,
        use_legacy_max_distance: None,
        alternatives: Box::new([AudioAlternative {
            object_form: false,
            name: format!("sounds/{name}").into(),
            weight: 1,
            volume: None,
            pitch: None,
            is_3d: None,
            stream: None,
            load_on_low_memory: None,
        }]),
    }
}

pub fn engine(names: &[(&str, &str)]) -> AudioEngine {
    let defs: Vec<_> = names
        .iter()
        .map(|(name, category)| definition(name, category))
        .collect();
    let catalog = RuntimeAudioCatalog::decode(
        &assets::encode_audio_catalog([0; 32], [0; 32], &defs).expect("catalog"),
    )
    .expect("decode");
    let bytes = assets::encode_sound_bank(b"{}", b"{}", b"{}", &[]).expect("bank");
    let prefix = assets::sound_bank_prefix_len(&bytes).expect("prefix");
    let index = SoundBankIndex::decode_prefix(&bytes[..prefix]).expect("index");
    let mut bank =
        SoundBank::from_parts(index, SoundEventTables::default(), Some(Arc::new(catalog)));
    for (name, _) in names {
        bank.insert_test_pcm(
            &format!("sounds/{name}"),
            Arc::new(Pcm {
                channels: 1,
                rate: 48_000,
                samples: vec![1000; 4800].into(),
            }),
        );
    }
    AudioEngine::new(Some(bank))
}

/// An engine whose bank holds `name` only as an encoded file, so its first play must decode.
fn engine_with_encoded(name: &str, category: &str) -> (AudioEngine, std::path::PathBuf) {
    engine_with_encoded_files(name, category, 0)
}

fn engine_with_encoded_files(
    name: &str,
    category: &str,
    others: usize,
) -> (AudioEngine, std::path::PathBuf) {
    let catalog = RuntimeAudioCatalog::decode(
        &assets::encode_audio_catalog([0; 32], [0; 32], &[definition(name, category)])
            .expect("catalog"),
    )
    .expect("decode");
    // One PCM16 mono FSB5 of two frames at 48 kHz.
    let mut fsb = b"FSB5".to_vec();
    let mode = (9_u64 << 1) | (2_u64 << 34);
    for value in [1_u32, 1, 8, 0, 4, 2, 0, 0] {
        fsb.extend(value.to_le_bytes());
    }
    fsb.resize(60, 0);
    fsb.extend(mode.to_le_bytes());
    fsb.extend([0, 0x40, 0, 0xc0]);
    let files: Vec<_> = std::iter::once((format!("sounds/{name}"), fsb.clone()))
        .chain((0..others).map(|i| (format!("sounds/waiting{i}"), fsb.clone())))
        .collect();
    let bytes = assets::encode_sound_bank(b"{}", b"{}", b"{}", &files).expect("bank");
    let path = std::env::temp_dir().join(format!(
        "cinnabar-engine-{}-{name}.mcbesnd",
        std::process::id()
    ));
    std::fs::write(&path, bytes).expect("write");
    let bank = SoundBank::open(&path, Some(Arc::new(catalog)))
        .expect("open")
        .expect("present");
    (AudioEngine::new(Some(bank)), path)
}

/// A first play queues a background decode instead of decoding inside the frame.
#[test]
fn undecoded_sounds_start_on_a_later_pump() {
    let (mut engine, path) = engine_with_encoded("random.pop", "player");
    let settings = AudioSettings::default();
    engine.enqueue(SoundRequest::new("random.pop"));
    assert!(engine.pump(None, 0.01, &settings).is_empty());
    assert_eq!(engine.pending.len(), 1);
    let mut started = Vec::new();
    for _ in 0..2000 {
        started = engine.pump(None, 0.0, &settings);
        if !started.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(started.len(), 1);
    assert!(engine.pending.is_empty());
    engine.enqueue(SoundRequest::new("random.pop"));
    assert_eq!(
        engine.pump(None, 0.0, &settings).len(),
        1,
        "decoded once, then cached"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_full_decode_backlog_defers_a_sound_until_it_can_start() {
    let capacity = crate::audio::bank::MAX_QUEUED_DECODES;
    let (mut engine, path) = engine_with_encoded_files("random.pop.backlog", "ui", capacity);
    let settings = AudioSettings::default();
    // Other entries own the finite backlog; the requested sound has not been queued yet.
    let bank = engine.bank.as_mut().unwrap();
    for i in 0..capacity {
        assert!(matches!(
            bank.lookup(&format!("sounds/waiting{i}"), false),
            PcmLookup::Pending
        ));
    }
    assert!(
        engine
            .start_rolled(
                &SoundRequest::new("random.pop.backlog"),
                None,
                &settings,
                None,
                [0.0; 3]
            )
            .is_none()
    );
    assert_eq!(engine.stats.decode_backlog, 1);
    assert_eq!(engine.pending.len(), 1);
    assert!(
        engine
            .bank
            .as_mut()
            .unwrap()
            .pcm("sounds/random.pop.backlog", false)
            .is_some()
    );
    assert_eq!(engine.pump(None, 0.0, &settings).len(), 1);
    assert!(engine.pending.is_empty());
    let _ = std::fs::remove_file(path);
}

// A burst of starts for one still-decoding sound must not queue without bound.
#[test]
fn pending_starts_are_coalesced_and_bounded() {
    let (mut engine, path) = engine_with_encoded("random.burst", "player");
    let settings = AudioSettings::default();
    for _ in 0..MAX_QUEUED {
        engine.enqueue(SoundRequest::new("random.burst"));
    }
    engine.pump(None, 0.0, &settings);
    assert!(engine.pending.len() <= MAX_SAME_SOUND);
    assert!(engine.stats.voice_limit > 0);
    let _ = std::fs::remove_file(path);
}

/// A loop waiting on its decode is not requested again every pump.
#[test]
fn pending_loops_are_not_duplicated() {
    let (mut engine, path) = engine_with_encoded("ambient.loop", "ambient");
    let settings = AudioSettings::default();
    engine.set_loop(
        "underwater",
        Some(LoopSpec {
            name: "ambient.loop".into(),
            volume: 0.5,
        }),
    );
    // Held so the started voice stays alive.
    let mut started = Vec::new();
    for _ in 0..2000 {
        started.extend(engine.pump(None, 0.0, &settings));
        assert!(engine.pending.len() <= 1);
        if !started.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(started.len(), 1);
    assert!(engine.pump(None, 0.0, &settings).is_empty());
    let _ = std::fs::remove_file(path);
}

const LISTENER: Listener = Listener {
    position: [0.0; 3],
    right: [1.0, 0.0, 0.0],
};

#[test]
fn positional_sounds_attenuate_and_drop_beyond_max_distance() {
    let mut engine = engine(&[("dig.stone", "block")]);
    let settings = AudioSettings::default();
    engine.enqueue(SoundRequest::new("dig.stone").at([8.0, 0.0, 0.0]));
    engine.enqueue(SoundRequest::new("dig.stone").at([40.0, 0.0, 0.0]));
    let started = engine.pump(Some(LISTENER), 0.05, &settings);
    assert_eq!(started.len(), 1);
    assert_eq!(engine.stats.out_of_range, 1);
    assert!((engine.voices[0].gain - 0.5).abs() < 1e-3);
}

// Thunder's volume 1000 widened its range but also became a 1000x output gain.
#[test]
fn loud_requests_reach_further_without_exceeding_unit_gain() {
    let mut engine = engine(&[("ambient.weather.thunder", "weather")]);
    let loud = FloatRange {
        min: 1000.0,
        max: 1000.0,
    };
    engine.enqueue(
        SoundRequest::new("ambient.weather.thunder")
            .at([200.0, 0.0, 0.0])
            .with_ranges(loud, FloatRange::ONE),
    );
    let started = engine.pump(Some(LISTENER), 0.05, &AudioSettings::default());
    assert_eq!(started.len(), 1, "admitted beyond the default 16 blocks");
    assert!(engine.voices[0].gain > 0.0 && engine.voices[0].gain <= 1.0);
}

#[test]
fn interface_sounds_ignore_position_and_listener() {
    let mut engine = engine(&[("random.click", "ui")]);
    engine.enqueue(SoundRequest::new("random.click").at([500.0, 0.0, 0.0]));
    let started = engine.pump(None, 0.05, &AudioSettings::default());
    assert_eq!(started.len(), 1);
    assert_eq!(engine.voices[0].gain, 1.0);
}

#[test]
fn category_sliders_scale_gain_and_unknown_definitions_are_counted() {
    let mut engine = engine(&[("dig.stone", "block")]);
    let mut settings = AudioSettings::default();
    settings.set(AudioCategory::Blocks, 0.25);
    engine.enqueue(SoundRequest::new("dig.stone").at([0.0; 3]));
    engine.enqueue(SoundRequest::new("missing.sound"));
    engine.pump(Some(LISTENER), 0.05, &settings);
    assert!((engine.voices[0].gain - 0.25).abs() < 1e-6);
    assert_eq!(engine.stats.no_definition, 1);
}

#[test]
fn identical_sounds_are_capped() {
    let mut engine = engine(&[("dig.stone", "block")]);
    for _ in 0..10 {
        engine.enqueue(SoundRequest::new("dig.stone").at([1.0, 0.0, 0.0]));
    }
    assert_eq!(
        engine
            .pump(Some(LISTENER), 0.05, &AudioSettings::default())
            .len(),
        MAX_SAME_SOUND
    );
    assert_eq!(engine.stats.voice_limit, 4);
}

#[test]
fn managed_loops_fade_in_and_retire_after_fading_out() {
    let mut engine = engine(&[("ambient.loop", "ambient")]);
    let settings = AudioSettings::default();
    let spec = LoopSpec {
        name: "ambient.loop".into(),
        volume: 0.5,
    };
    engine.set_loop("underwater", Some(spec));
    let mut held = engine.pump(None, 0.5, &settings);
    assert_eq!(held.len(), 1);
    assert!(engine.voices[0].gain > 0.0 && engine.voices[0].gain < 0.5);
    held.extend(engine.pump(None, 5.0, &settings));
    assert_eq!(held.len(), 1);
    assert!((engine.voices[0].gain - 0.5).abs() < 1e-6);
    engine.set_loop("underwater", None);
    engine.pump(None, 5.0, &settings);
    assert!(held[0].next().is_none());
}

// Admitted server alternatives can sum past u32; the pick must neither panic nor wrap.
#[test]
fn huge_aggregate_weights_pick_without_overflow() {
    let mut engine = engine(&[]);
    let alternative = AudioAlternative {
        weight: u16::MAX,
        ..definition("x", "ui").alternatives[0].clone()
    };
    let mut heavy = definition("heavy", "ui");
    heavy.alternatives = vec![alternative; 65_538].into();
    let mut pack = crate::audio::server::ServerSoundPack::default();
    pack.definitions.insert(Box::from("heavy"), heavy);
    engine.install_server(Some(Arc::new(pack)));
    let request = SoundRequest::new("heavy");
    let settings = AudioSettings::default();
    assert!(
        engine
            .start_rolled(&request, None, &settings, None, [0.999_999, 0.0, 0.0])
            .is_none()
    );
    assert_eq!(engine.stats.no_pcm, 1, "the pick reached PCM lookup");
}

// A PlaySound then StopSound in one ingestion pass still started the sound on the next pump.
#[test]
fn stops_cancel_starts_queued_before_them() {
    let mut engine = engine(&[("mob.cat", "neutral"), ("music.game", "music")]);
    let settings = AudioSettings::default();
    engine.enqueue(SoundRequest::new("mob.cat"));
    engine.stop_named("mob.cat");
    engine.enqueue(SoundRequest::new("music.game"));
    engine.stop_category(AudioCategory::Music);
    assert!(engine.pump(None, 0.05, &settings).is_empty());
}

#[test]
fn legacy_music_stop_leaves_effects_playing() {
    let mut engine = engine(&[("mob.cat", "neutral"), ("music.game", "music")]);
    engine.enqueue(SoundRequest::new("mob.cat"));
    engine.enqueue(SoundRequest::new("music.game"));
    let mut sources = engine.pump(None, 0.05, &AudioSettings::default());
    engine.stop_category(AudioCategory::Music);
    assert!(sources[0].next().is_some());
    assert!(sources[1].next().is_none());
}

#[test]
fn stop_named_cancels_matching_voices() {
    let mut engine = engine(&[("dig.stone", "block")]);
    engine.enqueue(SoundRequest::new("dig.stone").at([1.0, 0.0, 0.0]));
    let mut sources = engine.pump(Some(LISTENER), 0.05, &AudioSettings::default());
    engine.stop_named("dig.stone");
    assert!(sources[0].next().is_none());
}

#[test]
fn interface_definition_override_changes_the_played_sample_and_gain() {
    let mut engine = engine(&[(client_ui::sound_requests::UI_CLICK, "ui")]);
    engine.enqueue(SoundRequest::new(client_ui::sound_requests::UI_CLICK).scaled(0.5, 1.25));
    let base = engine.pump(None, 0.0, &AudioSettings::default());
    assert_eq!(base.len(), 1);
    assert_eq!(engine.voices[0].gain, 0.5);
    assert!(
        base.into_iter()
            .next()
            .unwrap()
            .take(128)
            .any(|sample| sample > 0.0)
    );
    engine.stop_all();

    let mut replacement = definition(client_ui::sound_requests::UI_CLICK, "ui");
    replacement.volume = Some(0.25);
    replacement.pitch = Some(2.0);
    replacement.alternatives[0].name = "sounds/custom/click".into();
    let mut pack = super::super::server::ServerSoundPack::default();
    pack.definitions
        .insert(client_ui::sound_requests::UI_CLICK.into(), replacement);
    engine.install_server(Some(Arc::new(pack)));
    engine.bank.as_mut().unwrap().insert_test_pcm(
        "sounds/custom/click",
        Arc::new(Pcm {
            channels: 1,
            rate: 48_000,
            samples: vec![-2000; 4800].into(),
        }),
    );
    engine.enqueue(SoundRequest::new(client_ui::sound_requests::UI_CLICK).scaled(0.5, 1.25));
    let overridden = engine.pump(None, 0.0, &AudioSettings::default());
    assert_eq!(overridden.len(), 1);
    assert_eq!(engine.voices.last().unwrap().gain, 0.125);
    let samples: Vec<_> = overridden.into_iter().next().unwrap().collect();
    assert!(samples.iter().any(|sample| *sample < 0.0));
    assert!(samples.iter().all(|sample| *sample <= 0.0));
    assert_eq!(
        samples.len(),
        3840,
        "the definition and request pitches multiply"
    );
}
