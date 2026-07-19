use std::{fs, path::PathBuf};

use assets::{HUD_SOURCE_MANIFEST_SHA256, HudTexture, HudTextureRole, encode_hud_catalog};
use bedrock_client::asset_startup::{HUD_ASSETS_COMPILE_COMMAND, hud_asset_path, load_hud_assets};
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
fn absent_hud_carrier_is_optional_and_does_not_invent_fallback_art() {
    let directory = temporary_directory("absent");
    let world_assets = directory.join("vanilla-v1001.mcbea");

    assert!(load_hud_assets(&world_assets).unwrap().is_none());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn hud_recovery_uses_the_automatic_official_sample_target() {
    assert_eq!(HUD_ASSETS_COMPILE_COMMAND, "make hud-assets");
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
