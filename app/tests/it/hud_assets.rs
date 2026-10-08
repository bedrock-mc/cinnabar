use std::{fs, path::PathBuf};

use assets::{HUD_SOURCE_MANIFEST_SHA256, HudTexture, HudTextureRole, encode_hud_catalog};
use bedrock_client::asset_startup::{
    AssetStartupError, DEFAULT_ASSET_PATH, HUD_ASSETS_COMPILE_COMMAND, hud_asset_path,
    hud_assets_missing_notice, hud_assets_rebuild_command, load_hud_assets, require_hud_assets,
};
use sha2::{Digest, Sha256};

fn temporary_directory(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rust-mcbe-hud-assets-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn fixture_carrier() -> Box<[u8]> {
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| {
            let [width, height] = role.expected_size();
            let rgba8 = vec![role as u8; width as usize * height as usize * 4].into_boxed_slice();
            HudTexture {
                role,
                source_bytes: rgba8.len() as u32,
                source_sha256: Sha256::digest(&rgba8).into(),
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width,
                height,
                rgba8,
            }
        })
        .collect::<Vec<_>>();
    encode_hud_catalog(HUD_SOURCE_MANIFEST_SHA256, &textures)
        .unwrap()
        .into_boxed_slice()
}

#[test]
fn optional_hud_probe_reports_absence_without_inventing_fallback_art() {
    let directory = temporary_directory("absent");
    let world_assets = directory.join("vanilla-v1001.mcbea");

    assert!(load_hud_assets(&world_assets).unwrap().is_none());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn hud_recovery_uses_the_automatic_official_sample_target() {
    assert_eq!(HUD_ASSETS_COMPILE_COMMAND, "make hud-assets");
    let default_hud_path = hud_asset_path(PathBuf::from(DEFAULT_ASSET_PATH).as_path());
    assert_eq!(
        hud_assets_rebuild_command(&default_hud_path),
        HUD_ASSETS_COMPILE_COMMAND
    );
}

#[test]
fn absent_hud_carrier_fails_startup_closed_with_official_sample_notice() {
    let directory = temporary_directory("absent-required");
    let world_assets = directory.join("vanilla-v1001.mcbea");

    let error = match require_hud_assets(&world_assets) {
        Ok(_) => panic!("absent HUD carrier unexpectedly satisfied the required startup contract"),
        Err(error) => error,
    };
    let expected_hud_path = hud_asset_path(&world_assets);
    match &error {
        AssetStartupError::HudAssetsMissing {
            path,
            rebuild_command,
            ..
        } => {
            assert_eq!(path, &expected_hud_path);
            assert_eq!(rebuild_command, &hud_assets_rebuild_command(path));
        }
        other => panic!("unexpected missing-HUD error: {other}"),
    }
    let error = error.to_string();
    // The fatal error is the single shared notice, so startup guidance never drifts from it.
    assert_eq!(error, hud_assets_missing_notice(&expected_hud_path));
    assert!(error.contains(&expected_hud_path.display().to_string()));
    assert!(error.contains("client will not start"));
    assert!(error.contains(HUD_ASSETS_COMPILE_COMMAND));
    assert!(error.contains("HUD_ASSET_BLOB="));
    assert!(!error.contains("make assets"));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn custom_hud_recovery_command_writes_the_exact_lookup_sibling() {
    let world_assets = PathBuf::from("custom asset root/compiled/world.mcbea");
    let hud_path = hud_asset_path(&world_assets);
    let command = hud_assets_rebuild_command(&hud_path);

    assert!(command.starts_with("make hud-assets "));
    assert!(command.contains("HUD_ASSET_BLOB='custom asset root/compiled/vanilla-v1.mcbehud'"));
    assert!(command.contains("HUD_ASSET_REPORT='custom asset root/compiled/hud-assets.json'"));
    assert!(!command.contains("make assets"));

    let notice = hud_assets_missing_notice(&hud_path);
    assert!(notice.contains(&command));
    assert!(!notice.contains("refresh every required carrier with `make assets`"));
}

#[test]
fn custom_hud_recovery_command_quotes_shell_sensitive_paths() {
    let hud_path = PathBuf::from("custom player's assets/vanilla-v1.mcbehud");
    let command = hud_assets_rebuild_command(&hud_path);

    #[cfg(windows)]
    assert!(command.contains("HUD_ASSET_BLOB='custom player''s assets/vanilla-v1.mcbehud'"));
    #[cfg(not(windows))]
    assert!(command.contains("HUD_ASSET_BLOB='custom player'\"'\"'s assets/vanilla-v1.mcbehud'"));
}

#[test]
fn missing_hud_notice_identifies_the_official_sample_and_automatic_repairs() {
    let path = hud_asset_path(PathBuf::from(DEFAULT_ASSET_PATH).as_path());
    let notice = hud_assets_missing_notice(&path);
    assert!(notice.contains("pinned official Mojang sample HUD carrier"));
    assert!(notice.contains(&path.display().to_string()));
    assert!(notice.contains("make hud-assets"));
    assert!(notice.contains("make assets"));
    for stale_claim in [
        "installed/owned Bedrock client",
        "bedrock-samples pack does not contain",
        "Export resource_packs/vanilla",
    ] {
        assert!(
            !notice.contains(stale_claim),
            "stale claim returned: {stale_claim}"
        );
    }
}

#[test]
fn valid_sibling_hud_carrier_loads_with_provenance() {
    let directory = temporary_directory("valid");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    let hud_path = hud_asset_path(&world_assets);
    fs::write(&hud_path, fixture_carrier()).unwrap();

    let loaded = load_hud_assets(&world_assets).unwrap().unwrap();
    assert_eq!(
        loaded.runtime().source_manifest_sha256(),
        HUD_SOURCE_MANIFEST_SHA256
    );
    assert!(
        loaded
            .startup_summary()
            .contains(&hud_path.display().to_string())
    );
    assert!(
        loaded
            .startup_summary()
            .contains("pinned official Mojang sample HUD assets")
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_hud_carrier_fails_closed_with_rebuild_command() {
    let directory = temporary_directory("malformed");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    fs::write(hud_asset_path(&world_assets), b"not-a-hud-carrier").unwrap();

    let error = match load_hud_assets(&world_assets) {
        Ok(_) => panic!("malformed HUD carrier unexpectedly loaded"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains(HUD_ASSETS_COMPILE_COMMAND));
    assert!(error.contains("invalid HUD texture carrier"));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stale_source_identity_carrier_is_rejected_at_startup() {
    let directory = temporary_directory("stale-source");
    let world_assets = directory.join("vanilla-v1001.mcbea");
    let mut carrier = fixture_carrier().into_vec();
    carrier[16..48].fill(0x42);
    let payload_end =
        usize::try_from(u64::from_le_bytes(carrier[64..72].try_into().unwrap())).unwrap();
    let envelope_hash = Sha256::digest(&carrier[..payload_end]);
    carrier[payload_end..].copy_from_slice(&envelope_hash);
    fs::write(hud_asset_path(&world_assets), carrier).unwrap();

    let error = match load_hud_assets(&world_assets) {
        Ok(_) => panic!("stale HUD source identity unexpectedly loaded"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("noncanonical HUD carrier layout"));
    assert!(error.contains(HUD_ASSETS_COMPILE_COMMAND));

    fs::remove_dir_all(directory).unwrap();
}
