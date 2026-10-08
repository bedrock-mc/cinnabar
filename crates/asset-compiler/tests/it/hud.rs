use std::{fs, path::PathBuf, process::Command};

use assets::{HUD_SOURCE_MANIFEST_SHA256, HudTextureRole, RuntimeHudCatalog};
use pack_compiler::compile_hud_assets;
use sha2::{Digest, Sha256};

const SOURCE_MANIFEST: &[u8] = include_bytes!("../../../../assets/hud-source-v2193.json");

#[test]
fn tracked_hud_manifest_is_the_reviewed_protocol_2193_identity() {
    let canonical = SOURCE_MANIFEST
        .split(|byte| *byte == b'\r')
        .flat_map(|part| part.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(&canonical)),
        HUD_SOURCE_MANIFEST_SHA256
    );
    let text = std::str::from_utf8(SOURCE_MANIFEST).unwrap();
    let pinned = assets::vanilla_source();
    for evidence in [
        pinned.tag.as_ref(),
        pinned.commit.as_ref(),
        pinned.archive.as_ref(),
        pinned.sha256.as_ref(),
        pinned.url.as_ref(),
        "\"protocol\": 2193",
        "ui/scoreboards.json",
        "ui/hud_screen.json",
        "textures/ui/heart.png",
        "textures/ui/filled_progress_bar.png",
    ] {
        assert!(text.contains(evidence), "missing evidence {evidence}");
    }
}

#[test]
fn modified_or_custom_hud_manifest_is_rejected_before_pack_ingestion() {
    let directory = tempfile::tempdir().unwrap();
    let mut modified = SOURCE_MANIFEST.to_vec();
    let index = modified.iter().position(|byte| *byte == b'1').unwrap();
    modified[index] = b'2';

    let error = compile_hud_assets(directory.path(), &modified).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("reviewed protocol-2193 identity")
    );
    assert!(!error.to_string().contains("could not be read"));
}

#[test]
fn stale_or_custom_pack_is_rejected_against_the_reviewed_source_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    fs::write(&manifest, b"not the pinned Mojang sample manifest").unwrap();

    let error = compile_hud_assets(directory.path(), SOURCE_MANIFEST).unwrap_err();

    assert!(error.to_string().contains("manifest.json"));
    assert!(
        error
            .to_string()
            .contains("does not match the pinned Mojang bedrock-samples pack")
    );
}

#[test]
#[ignore = "requires PINNED_BEDROCK_SAMPLES_PACK pointing at the pinned full sample resource_pack"]
fn exact_official_sample_pack_compiles_all_reviewed_hud_sources() {
    let pack = PathBuf::from(std::env::var_os("PINNED_BEDROCK_SAMPLES_PACK").unwrap());
    let compiled = compile_hud_assets(&pack, SOURCE_MANIFEST).unwrap();
    let runtime = RuntimeHudCatalog::decode(&compiled.bytes).unwrap();

    assert_eq!(
        compiled.report.source_manifest_sha256,
        HUD_SOURCE_MANIFEST_SHA256
    );
    assert_eq!(compiled.report.textures, HudTextureRole::ALL.len());
    assert_eq!(runtime.textures().len(), HudTextureRole::ALL.len());
}

#[test]
fn hud_assets_cli_requires_the_reviewed_source_manifest() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["hud-assets", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for flag in ["--pack", "--source-manifest", "--out", "--report"] {
        assert!(stdout.contains(flag), "missing {flag} from:\n{stdout}");
    }
}
