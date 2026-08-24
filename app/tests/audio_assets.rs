//! Optional pinned sound-definition carrier acceptance witnesses.
//!
//! Per the owner decision for VPA-017, this carrier binds optionally: absence
//! falls back to a bounded empty catalog with a one-time startup notice, while
//! a present-but-malformed, oversized, or stale-provenance carrier fails
//! startup closed with the exact path and rebuild command.

use std::{fs, path::PathBuf};

use assets::{AudioAlternative, AudioDefinition, RuntimeAudioCatalog, encode_audio_catalog};
use bedrock_client::asset_startup::{
    AUDIO_ASSETS_COMPILE_COMMAND, AssetStartupError, DEFAULT_ASSET_PATH, audio_asset_path,
    audio_assets_missing_notice, audio_assets_rebuild_command, load_audio_assets,
    vanilla_source_manifest_json,
};
use sha2::{Digest, Sha256};

fn temporary_directory(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rust-mcbe-audio-assets-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn pinned_manifest_sha256() -> [u8; 32] {
    bedrock_client::asset_startup::canonical_source_manifest_sha256(vanilla_source_manifest_json())
}

fn alternative(name: &str, weight: u16) -> AudioAlternative {
    AudioAlternative {
        object_form: true,
        name: name.into(),
        weight,
        volume: None,
        pitch: None,
        is_3d: None,
        stream: None,
        load_on_low_memory: None,
    }
}

fn definition(identifier: &str, alternatives: Vec<AudioAlternative>) -> AudioDefinition {
    AudioDefinition {
        identifier: identifier.into(),
        category: None,
        subtitle: None,
        min_distance: None,
        max_distance: None,
        volume: None,
        pitch: None,
        use_legacy_max_distance: None,
        alternatives: alternatives.into_boxed_slice(),
    }
}

/// Builds a well-formed MCBEAUD1 carrier through the public encoder, binding
/// the checkout-pinned source-manifest identity unless overridden.
fn fixture_carrier(manifest_sha256: [u8; 32]) -> Vec<u8> {
    let definitions = vec![
        definition(
            "random.orb",
            vec![
                alternative("sounds/orb_a", 3),
                alternative("sounds/orb_b", 7),
            ],
        ),
        definition("note.pling", vec![alternative("sounds/pling", 1)]),
    ];
    encode_audio_catalog(manifest_sha256, [0x22; 32], &definitions).unwrap()
}

#[test]
fn absent_audio_carrier_reports_optional_absence_without_failure() {
    let directory = temporary_directory("absent");
    let world_assets = directory.join("vanilla-v1001.mcbea");

    assert!(load_audio_assets(&world_assets).unwrap().is_none());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn audio_recovery_uses_the_automatic_official_sample_target() {
    assert_eq!(AUDIO_ASSETS_COMPILE_COMMAND, "make audio-assets");
    let default_audio_path = audio_asset_path(PathBuf::from(DEFAULT_ASSET_PATH).as_path());
    assert_eq!(
        audio_assets_rebuild_command(&default_audio_path),
        AUDIO_ASSETS_COMPILE_COMMAND
    );
}

#[test]
fn custom_audio_recovery_command_writes_the_exact_lookup_sibling() {
    let world_assets = PathBuf::from("custom asset root/compiled/world.mcbea");
    let audio_path = audio_asset_path(&world_assets);
    let command = audio_assets_rebuild_command(&audio_path);

    assert!(command.starts_with("make audio-assets "));
    assert!(command.contains("AUDIO_ASSET_BLOB='custom asset root/compiled/vanilla-v1.mcbeaud'"));
    assert!(command.contains("AUDIO_ASSET_REPORT='custom asset root/compiled/audio-assets.json'"));
    assert!(!command.contains("make assets"));

    let notice = audio_assets_missing_notice(&audio_path);
    assert!(notice.contains(&command));
    assert!(!notice.contains("refresh every carrier with `make assets`"));
}

#[test]
fn custom_audio_recovery_command_quotes_shell_sensitive_paths() {
    let audio_path = PathBuf::from("custom player's assets/vanilla-v1.mcbeaud");
    let command = audio_assets_rebuild_command(&audio_path);

    #[cfg(windows)]
    assert!(command.contains("AUDIO_ASSET_BLOB='custom player''s assets/vanilla-v1.mcbeaud'"));
    #[cfg(not(windows))]
    assert!(command.contains("AUDIO_ASSET_BLOB='custom player'\"'\"'s assets/vanilla-v1.mcbeaud'"));
}

#[test]
fn missing_audio_notice_names_the_exact_path_and_repairs_without_fatal_language() {
    let path = audio_asset_path(PathBuf::from(DEFAULT_ASSET_PATH).as_path());
    let notice = audio_assets_missing_notice(&path);
    assert!(notice.contains(&path.display().to_string()));
    assert!(notice.contains("make audio-assets"));
    assert!(notice.contains("make assets"));
    // Optional binding must not claim the client refuses to start.
    assert!(!notice.contains("will not start"));
    assert!(notice.contains("skips"));
}

#[test]
fn valid_sibling_audio_carrier_loads_with_pinned_provenance() {
    let directory = temporary_directory("valid");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    let audio_path = audio_asset_path(&world_assets);
    fs::write(&audio_path, fixture_carrier(pinned_manifest_sha256())).unwrap();

    let loaded = load_audio_assets(&world_assets).unwrap().expect("present");
    assert_eq!(
        loaded.runtime().source_manifest_sha256(),
        pinned_manifest_sha256()
    );
    assert_eq!(
        loaded
            .runtime()
            .lookup("random.orb")
            .expect("fixture hit")
            .alternatives
            .len(),
        2
    );
    let summary = loaded.startup_summary();
    assert!(summary.contains(&audio_path.display().to_string()));
    assert!(summary.contains("sound definitions"));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_audio_carrier_fails_startup_closed_with_rebuild_command() {
    let directory = temporary_directory("malformed");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    fs::write(audio_asset_path(&world_assets), b"not-an-audio-carrier").unwrap();

    let error = match load_audio_assets(&world_assets) {
        Ok(_) => panic!("malformed sound-definition carrier unexpectedly loaded"),
        Err(error) => error,
    };
    let text = error.to_string();
    assert!(text.contains(AUDIO_ASSETS_COMPILE_COMMAND));
    assert!(text.contains(&audio_asset_path(&world_assets).display().to_string()));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn oversized_audio_carrier_fails_startup_closed_before_decode() {
    let directory = temporary_directory("oversized");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    let oversized = vec![0u8; assets::MAX_AUDIO_CARRIER_BYTES + 1];
    fs::write(audio_asset_path(&world_assets), oversized).unwrap();

    let error = match load_audio_assets(&world_assets) {
        Ok(_) => panic!("oversized sound-definition carrier unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        AssetStartupError::AudioAssetsTooLarge { .. }
    ));
    assert!(error.to_string().contains(AUDIO_ASSETS_COMPILE_COMMAND));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stale_source_manifest_identity_is_rejected_at_startup() {
    let directory = temporary_directory("stale-provenance");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    let mut stale = [0x5au8; 32];
    stale[0] ^= 0xff;
    fs::write(audio_asset_path(&world_assets), fixture_carrier(stale)).unwrap();

    let error = match load_audio_assets(&world_assets) {
        Ok(_) => panic!("stale sound-definition provenance unexpectedly loaded"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("manifest"));
    assert!(error.contains(AUDIO_ASSETS_COMPILE_COMMAND));
    assert!(error.contains(&format!("{:02x}", stale[0])));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn envelope_hash_tampering_is_rejected_by_the_catalog_decoder() {
    let directory = temporary_directory("tampered");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    let mut carrier = fixture_carrier(pinned_manifest_sha256());
    let last = carrier.len() - 1;
    carrier[last] ^= 0xff;
    fs::write(audio_asset_path(&world_assets), carrier).unwrap();

    let error = match load_audio_assets(&world_assets) {
        Ok(_) => panic!("hash-tampered carrier unexpectedly loaded"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains(AUDIO_ASSETS_COMPILE_COMMAND));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn decoded_fixture_catalog_resolves_through_binary_search_in_canonical_order() {
    let bytes = fixture_carrier(pinned_manifest_sha256());
    let catalog = RuntimeAudioCatalog::decode(&bytes).unwrap();
    let identifiers: Vec<_> = catalog
        .definitions()
        .iter()
        .map(|definition| definition.identifier.clone())
        .collect();
    let mut sorted = identifiers.clone();
    sorted.sort();
    assert_eq!(identifiers, sorted, "definitions decode in canonical order");
    assert!(catalog.lookup("note.pling").is_some());
    assert!(catalog.lookup("absent.sound").is_none());
    assert_ne!(Sha256::digest(&bytes).as_slice(), &[0u8; 32][..]);
}
