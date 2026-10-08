use assets::{
    AudioAlternative, AudioDefinition, AudioPcmExpectedIdentity, RuntimeAudioCatalog,
    RuntimeAudioPcm, encode_audio_catalog, encode_audio_pcm, validate_audio_pcm_catalog,
};
use sha2::{Digest, Sha256};

fn definition() -> AudioDefinition {
    AudioDefinition {
        identifier: "test:finite".into(),
        category: None,
        subtitle: None,
        min_distance: None,
        max_distance: None,
        volume: None,
        pitch: None,
        use_legacy_max_distance: None,
        alternatives: vec![AudioAlternative {
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
    }
}

fn identity(catalog: &[u8]) -> AudioPcmExpectedIdentity {
    AudioPcmExpectedIdentity::new(
        "test:finite",
        "sounds/test/finite.fsb",
        Sha256::digest(catalog).into(),
        [1; 32],
        [2; 32],
        [3; 32],
        Sha256::digest([1, 0, 2, 0]).into(),
        208,
        1,
        48000,
        2,
    )
    .unwrap()
}

#[test]
fn public_pcm_api_roundtrip_and_catalog_route_rejection() {
    let original = definition();
    let bytes = encode_audio_catalog([1; 32], [2; 32], std::slice::from_ref(&original)).unwrap();
    let expected = identity(&bytes);
    let pcm = encode_audio_pcm(&expected, &[1, 2]).unwrap();
    let catalog = RuntimeAudioCatalog::decode(&bytes).unwrap();
    assert_eq!(
        RuntimeAudioPcm::decode(&pcm, &catalog, Sha256::digest(&bytes).into(), &expected)
            .unwrap()
            .samples(),
        &[1, 2]
    );
    for variant in 0..9 {
        let mut changed = original.clone();
        match variant {
            0 => changed.identifier = "test:other".into(),
            1 => changed.volume = Some(0.2),
            2 => changed.pitch = Some(2.0),
            3 => changed.alternatives[0].is_3d = Some(true),
            4 => changed.alternatives[0].is_3d = None,
            5 => changed.alternatives[0].stream = Some(false),
            6 => changed.alternatives[0].name = "sounds/test/other".into(),
            7 => changed.alternatives[0].volume = Some(0.2),
            8 => changed.alternatives = vec![changed.alternatives[0].clone(); 2].into_boxed_slice(),
            _ => unreachable!(),
        }
        let changed_bytes = encode_audio_catalog([1; 32], [2; 32], &[changed]).unwrap();
        let changed_catalog = RuntimeAudioCatalog::decode(&changed_bytes).unwrap();
        // Independently authorizing the changed catalog hash must not bypass route checks.
        let changed_expected = identity(&changed_bytes);
        assert!(
            validate_audio_pcm_catalog(
                &changed_catalog,
                Sha256::digest(&changed_bytes).into(),
                &changed_expected
            )
            .is_err(),
            "variant {variant}"
        );
        assert!(
            RuntimeAudioPcm::decode(
                &pcm,
                &changed_catalog,
                Sha256::digest(&changed_bytes).into(),
                &expected
            )
            .is_err()
        );
    }
}

#[test]
fn reviewed_profile_is_finite_and_oracle_pinned_not_self_declared() {
    let expected = assets::reviewed_audio_pcm_identity();
    assert_eq!(
        (
            expected.channels(),
            expected.sample_rate(),
            expected.frames()
        ),
        (2, 29015, 795136)
    );
    assert_eq!(expected.source_bytes(), 869792);
    assert_eq!(expected.pcm_sha256()[..4], [0x91, 0xf8, 0x71, 0x6f]);
    assert!(encode_audio_pcm(&expected, &[0; 2]).is_err());
}
