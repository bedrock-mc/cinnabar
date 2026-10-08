use assets::{
    AudioAlternative, AudioDefinition, MAX_AUDIO_ALTERNATIVES_PER_DEFINITION,
    MAX_AUDIO_IDENTIFIER_BYTES, RuntimeAudioCatalog, encode_audio_catalog,
};
use sha2::{Digest, Sha256};

fn alternative(name: &str, object_form: bool) -> AudioAlternative {
    AudioAlternative {
        object_form,
        name: name.into(),
        weight: 7,
        volume: Some(1000.0),
        pitch: Some(0.1),
        is_3d: Some(false),
        stream: Some(true),
        load_on_low_memory: Some(false),
    }
}

fn definition(identifier: &str) -> AudioDefinition {
    AudioDefinition {
        identifier: identifier.into(),
        category: Some("ambient".into()),
        subtitle: Some("subtitle.example".into()),
        min_distance: Some(0.0),
        max_distance: Some(256.0),
        volume: None,
        pitch: Some(4.0),
        use_legacy_max_distance: Some("true".into()),
        alternatives: vec![
            alternative("sounds/z", false),
            alternative("sounds/a", true),
        ]
        .into_boxed_slice(),
    }
}

#[test]
fn audio_carrier_round_trips_canonical_lookup_and_source_identity() {
    let bytes = encode_audio_catalog([1; 32], [2; 32], &[definition("z"), definition("a")])
        .expect("encode catalog");
    let catalog = RuntimeAudioCatalog::decode(&bytes).expect("decode catalog");
    assert_eq!(catalog.source_manifest_sha256(), [1; 32]);
    assert_eq!(catalog.sound_definitions_sha256(), [2; 32]);
    assert_eq!(catalog.definitions()[0].identifier.as_ref(), "a");
    assert_eq!(
        catalog.lookup("z").unwrap().alternatives[0].name.as_ref(),
        "sounds/a"
    );
    assert!(catalog.lookup("unknown.custom").is_none());
}

#[test]
fn source_and_alternative_order_compile_to_identical_bytes() {
    let left = definition("a");
    let mut right = left.clone();
    right.alternatives.reverse();
    let first = encode_audio_catalog([1; 32], [2; 32], &[definition("z"), left.clone()]).unwrap();
    let second = encode_audio_catalog([1; 32], [2; 32], &[right, definition("z")]).unwrap();
    assert_eq!(first, second);
}

#[test]
fn corruption_truncation_noncanonical_order_and_trailing_bytes_fail_closed() {
    let bytes = encode_audio_catalog([1; 32], [2; 32], &[definition("a")]).unwrap();
    let mut corrupted = bytes.clone();
    corrupted[88] ^= 1;
    assert!(RuntimeAudioCatalog::decode(&corrupted).is_err());
    assert!(RuntimeAudioCatalog::decode(&bytes[..bytes.len() - 1]).is_err());

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(RuntimeAudioCatalog::decode(&trailing).is_err());

    // The first payload byte pair is the identifier length. Replace `a` with
    // invalid UTF-8 and repair the envelope digest so payload validation runs.
    let mut malformed = bytes;
    malformed[90] = 0xff;
    repair_envelope_hash(&mut malformed);
    assert!(RuntimeAudioCatalog::decode(&malformed).is_err());
}

#[test]
fn encoder_rejects_duplicate_oversized_nonfinite_and_excessive_records() {
    let duplicate = definition("same");
    assert!(encode_audio_catalog([0; 32], [0; 32], &[duplicate.clone(), duplicate]).is_err());

    let mut oversized = definition("a");
    oversized.identifier = "x".repeat(MAX_AUDIO_IDENTIFIER_BYTES + 1).into_boxed_str();
    assert!(encode_audio_catalog([0; 32], [0; 32], &[oversized]).is_err());

    let mut nonfinite = definition("a");
    nonfinite.alternatives[0].volume = Some(f32::INFINITY);
    assert!(encode_audio_catalog([0; 32], [0; 32], &[nonfinite]).is_err());

    let mut excessive = definition("a");
    excessive.alternatives = (0..=MAX_AUDIO_ALTERNATIVES_PER_DEFINITION)
        .map(|index| alternative(&format!("sounds/{index}"), true))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    assert!(encode_audio_catalog([0; 32], [0; 32], &[excessive]).is_err());
}

fn repair_envelope_hash(bytes: &mut [u8]) {
    let hash_offset = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..hash_offset]);
    bytes[hash_offset..].copy_from_slice(&digest);
}
