use std::{ffi::OsString, fs};

use assets::{
    AtmosphereRole, AtmosphereTexture, CompiledAtmosphereAssets, RuntimeAtmosphereAssets,
};
use clap::Parser;
use sha2::{Digest, Sha256};

use super::{Cli, Command, canonical_source_manifest_sha256, compile_atmosphere_command};

#[test]
fn outline_manifest_identity_is_portable_across_checkout_line_endings() {
    let lf = include_bytes!("../../../../../assets/ui-font-source.json");
    assert!(!lf.contains(&b'\r'));
    let crlf = String::from_utf8(lf.to_vec())
        .unwrap()
        .replace('\n', "\r\n");

    assert_eq!(
        canonical_source_manifest_sha256(lf),
        canonical_source_manifest_sha256(crlf.as_bytes())
    );
}

#[test]
fn synthetic_cli_override_builds_canonical_path_only_report() {
    let directory = tempfile::tempdir().unwrap();
    let pack = directory.path().join("pack");
    let manifest = directory.path().join("manifest.json");
    let physical_override = directory.path().join("private-clouds.png");
    let blob = directory.path().join("atmosphere.mcbeatm");
    let report = directory.path().join("atmosphere.json");
    fs::write(&manifest, br#"{"artifact_policy":"local-only"}"#).unwrap();
    let cli = Cli::try_parse_from([
        OsString::from("assetc"),
        OsString::from("atmosphere"),
        OsString::from("--pack"),
        pack.as_os_str().to_owned(),
        OsString::from("--source-manifest"),
        manifest.as_os_str().to_owned(),
        OsString::from("--clouds-override"),
        physical_override.as_os_str().to_owned(),
        OsString::from("--out"),
        blob.as_os_str().to_owned(),
        OsString::from("--report"),
        report.as_os_str().to_owned(),
    ])
    .unwrap();
    let Command::Atmosphere {
        pack,
        source_manifest,
        clouds_override,
        out,
        report,
    } = cli.command
    else {
        panic!("expected atmosphere command");
    };
    let compiled = synthetic_compiled();
    compile_atmosphere_command(
        &pack,
        &source_manifest,
        clouds_override.as_deref(),
        &out,
        &report,
        |actual_pack, manifest_bytes, options| {
            assert_eq!(actual_pack, pack);
            assert_eq!(manifest_bytes, br#"{"artifact_policy":"local-only"}"#);
            assert_eq!(options.clouds_override, Some(physical_override.as_path()));
            Ok(compiled.clone())
        },
    )
    .unwrap();

    let blob_bytes = fs::read(&out).unwrap();
    let runtime = RuntimeAtmosphereAssets::decode(&blob_bytes).unwrap();
    assert_eq!(runtime.textures(), compiled.textures.as_ref());
    let report_text = fs::read_to_string(&report).unwrap();
    let value: serde_json::Value = serde_json::from_str(&report_text).unwrap();
    assert_eq!(
        value["textures"][2]["source_path"],
        "textures/environment/clouds.png"
    );
    assert_eq!(
        value["textures"][2]["source_sha256"],
        hex(&compiled.textures[2].source_sha256)
    );
    assert_eq!(
        value["textures"][2]["pixels_sha256"],
        hex(&compiled.textures[2].pixels_sha256)
    );
    assert!(!report_text.contains(&physical_override.display().to_string()));
}

fn synthetic_compiled() -> CompiledAtmosphereAssets {
    let specs = [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
        ),
    ];
    let textures = specs
        .into_iter()
        .enumerate()
        .map(|(index, (role, source_path, width, height))| {
            let rgba8 =
                vec![index as u8 + 1; width as usize * height as usize * 4].into_boxed_slice();
            AtmosphereTexture {
                role,
                source_path: source_path.into(),
                source_bytes: index as u32 + 1,
                source_sha256: [index as u8 + 1; 32],
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width,
                height,
                rgba8,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    CompiledAtmosphereAssets {
        source_manifest_sha256: [0x44; 32],
        textures,
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
