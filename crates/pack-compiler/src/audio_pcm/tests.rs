use super::*;

fn fixture(root: &Path) -> (Vec<u8>, AudioPcmExpectedIdentity) {
    let mut source = Vec::from(*b"FSB5");
    for value in [1_u32, 1, 8, 0, 140, 16, 0, 0] {
        source.extend(value.to_le_bytes());
    }
    source.resize(60, 0);
    source.extend(((2_u64 << 34) | (9 << 1)).to_le_bytes());
    source.resize(208, 0);
    source[80] = 0x21;
    let definition = assets::AudioDefinition {
        identifier: "test:finite".into(),
        category: Some("ambient".into()),
        subtitle: None,
        min_distance: None,
        max_distance: None,
        volume: None,
        pitch: None,
        use_legacy_max_distance: Some("true".into()),
        alternatives: vec![assets::AudioAlternative {
            object_form: true,
            name: "sounds/test/finite".into(),
            weight: 1,
            volume: None,
            pitch: None,
            is_3d: Some(false),
            stream: Some(true),
            load_on_low_memory: None,
        }]
        .into_boxed_slice(),
    };
    let catalog = assets::encode_audio_catalog([1; 32], [2; 32], &[definition]).unwrap();
    let pcm = [1_i16, 2]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect::<Vec<_>>();
    let expected = AudioPcmExpectedIdentity::new(
        "test:finite",
        "sounds/test/finite.fsb",
        Sha256::digest(&catalog).into(),
        [1; 32],
        [2; 32],
        Sha256::digest(&source).into(),
        Sha256::digest(&pcm).into(),
        source.len() as u32,
        1,
        48000,
        2,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("sounds/test")).unwrap();
    std::fs::write(root.join(expected.source_path()), source).unwrap();
    (catalog, expected)
}

#[test]
fn real_synthetic_producer_to_authenticated_consumer_is_deterministic() {
    let root = tempfile::tempdir().unwrap();
    let (catalog, expected) = fixture(root.path());
    let first = compile_with_expected(root.path(), &catalog, &expected).unwrap();
    let second = compile_with_expected(root.path(), &catalog, &expected).unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.report, second.report);
    let catalog = RuntimeAudioCatalog::decode(&catalog).unwrap();
    let runtime = assets::RuntimeAudioPcm::decode(
        &first.bytes,
        &catalog,
        expected.catalog_sha256(),
        &expected,
    )
    .unwrap();
    assert_eq!(runtime.samples(), &[1, 2]);
    assert_eq!(runtime.mode(), assets::AudioPcmMode::FinitePredecodedNoLoop);
    assert!(first.report.source_streaming);
    assert!(!first.report.exact_target_sample_identity);
}

#[test]
fn missing_changed_oversized_and_wrong_codec_sources_reject() {
    let root = tempfile::tempdir().unwrap();
    let (catalog, expected) = fixture(root.path());
    let path = root.path().join(expected.source_path());
    let original = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(compile_with_expected(root.path(), &catalog, &expected).is_err());
    let mut changed = original.clone();
    changed[24] = 15;
    std::fs::write(&path, &changed).unwrap();
    assert!(compile_with_expected(root.path(), &catalog, &expected).is_err());
    let bad_codec = AudioPcmExpectedIdentity::new(
        expected.identifier(),
        expected.source_path(),
        expected.catalog_sha256(),
        [1; 32],
        [2; 32],
        Sha256::digest(&changed).into(),
        expected.pcm_sha256(),
        changed.len() as u32,
        1,
        48000,
        2,
    )
    .unwrap();
    assert!(matches!(
        compile_with_expected(root.path(), &catalog, &bad_codec),
        Err(AudioPcmCompileError::Decode(_))
    ));
    std::fs::write(&path, vec![0; MAX_AUDIO_PCM_SOURCE_BYTES as usize + 1]).unwrap();
    assert!(compile_with_expected(root.path(), &catalog, &expected).is_err());
    std::fs::write(&path, &original).unwrap();
    let wrong_pcm = AudioPcmExpectedIdentity::new(
        expected.identifier(),
        expected.source_path(),
        expected.catalog_sha256(),
        [1; 32],
        [2; 32],
        expected.source_sha256(),
        [9; 32],
        original.len() as u32,
        1,
        48000,
        2,
    )
    .unwrap();
    assert!(compile_with_expected(root.path(), &catalog, &wrong_pcm).is_err());
    let wrong_metadata = AudioPcmExpectedIdentity::new(
        expected.identifier(),
        expected.source_path(),
        expected.catalog_sha256(),
        [1; 32],
        [2; 32],
        expected.source_sha256(),
        expected.pcm_sha256(),
        original.len() as u32,
        2,
        48000,
        1,
    )
    .unwrap();
    assert!(matches!(
        compile_with_expected(root.path(), &catalog, &wrong_metadata),
        Err(AudioPcmCompileError::Invalid(
            "decoded metadata differs from reviewed expectation"
        ))
    ));
    let mut altered_catalog = catalog.clone();
    altered_catalog[0] ^= 1;
    assert!(compile_with_expected(root.path(), &altered_catalog, &expected).is_err());
}

#[test]
fn official_entry_point_does_not_accept_a_synthetic_catalog_or_manifest() {
    let root = tempfile::tempdir().unwrap();
    let (catalog, _) = fixture(root.path());
    let manifest = include_bytes!("../../../../assets/vanilla-source.json");
    assert!(compile_audio_pcm_assets(root.path(), &catalog, manifest).is_err());
    assert!(compile_audio_pcm_assets(root.path(), &catalog, b"{}").is_err());
}

#[cfg(unix)]
#[test]
fn sibling_uses_the_existing_secured_reader_for_final_and_parent_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let (catalog, expected) = fixture(root.path());
    let outside = tempfile::tempdir().unwrap();
    let original = std::fs::read(root.path().join(expected.source_path())).unwrap();
    std::fs::write(outside.path().join("finite.fsb"), &original).unwrap();
    std::fs::remove_file(root.path().join(expected.source_path())).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("finite.fsb"),
        root.path().join(expected.source_path()),
    )
    .unwrap();
    assert!(compile_with_expected(root.path(), &catalog, &expected).is_err());
    std::fs::remove_file(root.path().join(expected.source_path())).unwrap();
    std::fs::remove_dir(root.path().join("sounds/test")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("sounds/test")).unwrap();
    assert!(compile_with_expected(root.path(), &catalog, &expected).is_err());
}
