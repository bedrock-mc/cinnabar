use super::tests::tone;
use super::*;
use assets::AudioAlternative;

/// Builds one finite definition, optionally marked for streamed playback.
fn definition(name: &str, category: &str, stream: bool) -> AudioDefinition {
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
            object_form: true,
            name: format!("sounds/{name}").into(),
            weight: 1,
            volume: None,
            pitch: None,
            is_3d: None,
            stream: Some(stream),
            load_on_low_memory: None,
        }]),
    }
}

/// Opens an isolated bank containing these definitions and encoded test sounds.
fn bank(
    name: &str,
    definitions: &[AudioDefinition],
    files: &[(String, Vec<u8>)],
) -> (SoundBank, PathBuf) {
    let catalog = RuntimeAudioCatalog::decode(
        &assets::encode_audio_catalog([0; 32], [0; 32], definitions).unwrap(),
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "cinnabar-prewarm-{name}-{}.mcbesnd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        assets::encode_sound_bank(b"{}", b"{}", b"{}", files).unwrap(),
    )
    .unwrap();
    (
        SoundBank::open(&path, Some(Arc::new(catalog)))
            .unwrap()
            .unwrap(),
        path,
    )
}

#[test]
fn prewarming_caches_common_sounds_and_leaves_streams_and_music_alone() {
    let definitions = [
        definition("ui.accept", "ui", false),
        definition("step.stone", "block", false),
        definition("dig.stone", "block", false),
        definition("ui.stream", "ui", true),
        definition("music.test", "music", false),
    ];
    let files: Vec<_> = definitions
        .iter()
        .map(|definition| (definition.alternatives[0].name.to_string(), tone()))
        .collect();
    let (mut bank, path) = bank("common", &definitions, &files);
    bank.prewarm_common();
    assert_eq!(bank.in_flight.len(), 3);
    assert!(!bank.is_decoding("sounds/ui.stream"));
    assert!(!bank.is_decoding("sounds/music.test"));
    let mut cache_bytes = 0;
    for name in ["ui.accept", "step.stone", "dig.stone"] {
        let path = format!("sounds/{name}");
        let pcm = bank.pcm(&path, false).unwrap();
        assert!(Arc::ptr_eq(&pcm, bank.cache.get(path.as_str()).unwrap()));
        cache_bytes += pcm.samples.len() * std::mem::size_of::<i16>();
    }
    assert_eq!(bank.cache_bytes, cache_bytes);
    drop(bank);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn prewarming_bounds_each_group_and_skips_oversized_inputs() {
    let mut definitions = Vec::new();
    for (prefix, category) in [("ui", "ui"), ("step", "block"), ("dig", "block")] {
        for index in 0..PREWARM_SOUNDS_PER_GROUP + 1 {
            definitions.push(definition(
                &format!("{prefix}.sound{index}"),
                category,
                false,
            ));
        }
    }
    definitions.push(definition("ui.large", "ui", false));
    let files: Vec<_> = definitions
        .iter()
        .map(|definition| {
            let mut bytes = tone();
            if &*definition.identifier == "ui.large" {
                bytes.resize(PREWARM_BYTES + 1, 0);
            }
            (definition.alternatives[0].name.to_string(), bytes)
        })
        .collect();
    let (mut bank, path) = bank("bounds", &definitions, &files);
    bank.prewarm_common();
    assert_eq!(bank.in_flight.len(), 3 * PREWARM_SOUNDS_PER_GROUP);
    assert!(bank.in_flight_bytes <= PREWARM_BYTES);
    assert!(!bank.is_decoding("sounds/ui.large"));
    drop(bank);
    std::fs::remove_file(path).unwrap();
}
