//! Required localization carrier startup coverage: provenance over both the
//! canonical source manifest and the exact pinned `texts/en_US.lang` bytes.

use std::{fs, path::PathBuf, sync::Arc};

use assets::{LangEntry, VANILLA_EN_US_LANG_SHA256, encode_lang_catalog};
use bedrock_client::asset_startup::{
    AssetStartupError, DEFAULT_ASSET_PATH, LANG_ASSETS_COMPILE_COMMAND,
    canonical_source_manifest_sha256, lang_asset_path, lang_assets_rebuild_command,
    require_lang_assets, vanilla_source_manifest_json,
};

fn temporary_directory(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rust-mcbe-lang-assets-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn entries() -> Vec<LangEntry> {
    vec![
        LangEntry {
            key: "commands.op.success".into(),
            value: Arc::from("Opped: %s"),
        },
        LangEntry {
            key: "item.apple.name".into(),
            value: Arc::from("Apple"),
        },
    ]
}

fn write_carrier(root: &PathBuf, lang_source_sha256: [u8; 32]) -> PathBuf {
    let world_assets = root.join("vanilla-v1.mcbew");
    let manifest_sha256 = canonical_source_manifest_sha256(vanilla_source_manifest_json());
    let bytes = encode_lang_catalog(manifest_sha256, lang_source_sha256, &entries()).unwrap();
    fs::write(lang_asset_path(&world_assets), bytes).unwrap();
    world_assets
}

#[test]
fn valid_sibling_lang_carrier_loads_with_both_provenance_identities() {
    let root = temporary_directory("valid");
    let world_assets = write_carrier(&root, VANILLA_EN_US_LANG_SHA256);
    let loaded = require_lang_assets(&world_assets, vanilla_source_manifest_json()).unwrap();
    assert_eq!(loaded.runtime().len(), 2);
    let summary = loaded.startup_summary();
    assert!(summary.contains("source_manifest_sha256="));
    assert!(summary.contains("lang_source_sha256="));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn tampered_lang_source_bytes_beside_the_canonical_manifest_fail_closed() {
    let root = temporary_directory("tampered");
    // The carrier embeds the canonical manifest identity but was compiled
    // from language bytes that are not the pinned `texts/en_US.lang`.
    let world_assets = write_carrier(&root, [0xEE; 32]);
    let error = require_lang_assets(&world_assets, vanilla_source_manifest_json())
        .expect_err("tampered language source bytes must fail startup closed");
    match &error {
        AssetStartupError::LangAssetsSourceProvenance {
            path,
            rebuild_command,
            ..
        } => {
            assert_eq!(path, &lang_asset_path(&world_assets));
            assert_eq!(
                rebuild_command,
                &lang_assets_rebuild_command(&lang_asset_path(&world_assets))
            );
        }
        other => panic!("expected the source-bytes provenance error, got {other:?}"),
    }
    let message = error.to_string();
    assert!(message.contains("texts/en_US.lang"));
    assert!(message.contains(LANG_ASSETS_COMPILE_COMMAND));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn custom_asset_paths_get_recovery_commands_naming_their_exact_siblings() {
    // The default location keeps the bare make target.
    let default_carrier = lang_asset_path(std::path::Path::new(DEFAULT_ASSET_PATH));
    assert_eq!(
        lang_assets_rebuild_command(&default_carrier),
        LANG_ASSETS_COMPILE_COMMAND
    );

    // A custom --assets location names the exact blob and report siblings
    // through the Makefile's own variables.
    let custom = PathBuf::from("D:/packs/world assets/vanilla-v1.mcbelang");
    let command = lang_assets_rebuild_command(&custom);
    assert!(command.starts_with(LANG_ASSETS_COMPILE_COMMAND));
    assert!(command.contains("LANG_ASSET_BLOB='D:/packs/world assets/vanilla-v1.mcbelang'"));
    assert!(command.contains("LANG_ASSET_REPORT='D:/packs/world assets/lang-assets.json'"));

    // Every failure case for a custom path carries that same exact command:
    // missing, malformed, and tampered-provenance carriers alike.
    let root = temporary_directory("custom-recovery");
    let world_assets = root.join("vanilla-v1.mcbew");
    let carrier_path = lang_asset_path(&world_assets);
    let expected_command = lang_assets_rebuild_command(&carrier_path);

    let missing = require_lang_assets(&world_assets, vanilla_source_manifest_json())
        .expect_err("no carrier exists yet");
    match &missing {
        AssetStartupError::LangAssetsMissing {
            rebuild_command, ..
        } => assert_eq!(rebuild_command, &expected_command),
        other => panic!("expected the missing-carrier error, got {other:?}"),
    }
    assert!(missing.to_string().contains(&expected_command));

    fs::write(&carrier_path, b"not a carrier").unwrap();
    let malformed = require_lang_assets(&world_assets, vanilla_source_manifest_json())
        .expect_err("malformed bytes must fail closed");
    match &malformed {
        AssetStartupError::LangAssetsDecode {
            rebuild_command, ..
        } => assert_eq!(rebuild_command, &expected_command),
        other => panic!("expected the decode error, got {other:?}"),
    }

    let manifest_sha256 = canonical_source_manifest_sha256(vanilla_source_manifest_json());
    let stale = encode_lang_catalog([0xAB; 32], VANILLA_EN_US_LANG_SHA256, &entries()).unwrap();
    fs::write(&carrier_path, stale).unwrap();
    let provenance = require_lang_assets(&world_assets, vanilla_source_manifest_json())
        .expect_err("a stale manifest identity must fail closed");
    match &provenance {
        AssetStartupError::LangAssetsProvenance {
            rebuild_command,
            manifest,
            ..
        } => {
            assert_eq!(rebuild_command, &expected_command);
            assert!(manifest.contains(&format!("{:02x}", manifest_sha256[0])));
        }
        other => panic!("expected the manifest provenance error, got {other:?}"),
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_version_1_carriers_fail_closed_with_the_rebuild_command() {
    let root = temporary_directory("stale-version");
    let world_assets = root.join("vanilla-v1.mcbew");
    // A well-formed version-1 header (no lang-source identity) is a stale
    // format after the pinning upgrade and must not load.
    let manifest_sha256 = canonical_source_manifest_sha256(vanilla_source_manifest_json());
    let mut bytes =
        encode_lang_catalog(manifest_sha256, VANILLA_EN_US_LANG_SHA256, &entries()).unwrap();
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    fs::write(lang_asset_path(&world_assets), bytes).unwrap();
    let error = require_lang_assets(&world_assets, vanilla_source_manifest_json())
        .expect_err("a version-1 carrier is stale after the source-byte pinning upgrade");
    assert!(matches!(error, AssetStartupError::LangAssetsDecode { .. }));
    fs::remove_dir_all(root).unwrap();
}
