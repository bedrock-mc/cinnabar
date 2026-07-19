use std::fs;
use std::process::Command;

use asset_compiler::compile_hud_assets;
use assets::{HudTextureRole, RuntimeHudCatalog};
use image::{ImageBuffer, Rgba};

#[test]
fn compiles_the_fixed_survival_hud_sprite_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let pack = directory.path();
    fs::write(
        pack.join("manifest.json"),
        br#"{"format_version":2,"header":{"name":"fixture","uuid":"00000000-0000-0000-0000-000000000001","version":[1,26,33]},"modules":[]}"#,
    )
    .unwrap();
    for role in HudTextureRole::ALL {
        let path = pack.join(role.source_path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let [width, height] = role.expected_size();
        ImageBuffer::from_pixel(width, height, Rgba([role as u8, 2, 3, 255]))
            .save(path)
            .unwrap();
    }

    let compiled = compile_hud_assets(pack).unwrap();
    let runtime = RuntimeHudCatalog::decode(&compiled.bytes).unwrap();

    assert_eq!(compiled.report.textures, HudTextureRole::ALL.len());
    assert_eq!(
        compiled.report.decoded_bytes,
        HudTextureRole::ALL
            .into_iter()
            .map(|role| {
                let [width, height] = role.expected_size();
                width as usize * height as usize * 4
            })
            .sum::<usize>()
    );
    assert_eq!(runtime.textures().len(), HudTextureRole::ALL.len());
    assert_eq!(
        &runtime.texture(HudTextureRole::HeartFull).rgba8[..4],
        &[HudTextureRole::HeartFull as u8, 2, 3, 255]
    );
}

#[test]
fn missing_required_native_sprite_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("manifest.json"), b"{}").unwrap();
    let error = compile_hud_assets(directory.path()).unwrap_err();
    assert!(error.to_string().contains("heart_background.png"));
}

#[test]
fn malformed_pack_manifest_is_rejected_before_texture_ingestion() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("manifest.json"), b"not json").unwrap();

    let error = compile_hud_assets(directory.path()).unwrap_err();

    assert!(error.to_string().contains("manifest.json"));
    assert!(error.to_string().contains("valid JSON"));
    assert!(!error.to_string().contains("heart_background.png"));
}

#[test]
fn hud_assets_cli_is_public_and_documents_local_only_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["hud-assets", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for flag in ["--pack", "--out", "--report"] {
        assert!(stdout.contains(flag), "missing {flag} from:\n{stdout}");
    }
}
