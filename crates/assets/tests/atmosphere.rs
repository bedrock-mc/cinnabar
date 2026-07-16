use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assets::{
    AssetError, AtmosphereCompileOptions, AtmosphereRole, AtmosphereTexture, CelestialTile,
    CompiledAtmosphereAssets, RuntimeAtmosphereAssets, compile_atmosphere_assets,
    compile_atmosphere_assets_with_options, composite_celestial, encode_atmosphere_blob,
};
use image::{Rgba, RgbaImage};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const MANIFEST: &[u8] = br#"{"schema":1,"tag":"test","commit":"0123456789abcdef0123456789abcdef01234567","archive":"test.zip","url":"https://github.com/Mojang/bedrock-samples/releases/download/test/test.zip","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","artifact_policy":"local-only","cache_dir":".local/test"}"#;

const SOURCES: [(&str, u32, u32, u8); 3] = [
    ("textures/environment/sun.png", 32, 32, 0x11),
    ("textures/environment/moon_phases.png", 128, 64, 0x22),
    ("textures/environment/clouds.png", 256, 256, 0x33),
];

const NATIVE_CLOUD_SOURCE_SHA256: &str =
    "f19b2f3a483af3a67568dfed4387c7b59fed215edf1cb02bef0470f2b72982a0";
const NATIVE_CLOUD_PIXELS_SHA256: &str =
    "95f8808115fcc28c8665324bba1b72dcb1350fbfebd1c9a30009691326695136";

#[test]
fn exact_native_cloud_override_replaces_only_clouds_and_retains_logical_provenance() {
    let (Ok(pack), Ok(clouds_override)) = (
        std::env::var("PINNED_VANILLA_PACK"),
        std::env::var("CINNABAR_CLOUDS_PNG"),
    ) else {
        eprintln!(
            "skipping: PINNED_VANILLA_PACK and CINNABAR_CLOUDS_PNG must point at ignored local inputs"
        );
        return;
    };
    let manifest = tracked_manifest();
    let compiled = compile_atmosphere_assets_with_options(
        Path::new(&pack),
        &manifest,
        AtmosphereCompileOptions {
            clouds_override: Some(Path::new(&clouds_override)),
        },
    )
    .unwrap();

    for (texture, expected) in compiled.textures[..2].iter().zip([
        "f7273544b691f08aaef76373d526e00793cf1e1aa0e1df8518f738d44a8e526b",
        "01c566d48e0cc8618cf6fdce811b61175fc246f12f2e8f2c567d6acd3a2b35d8",
    ]) {
        assert_eq!(
            texture.source_sha256,
            Sha256::digest(fs::read(Path::new(&pack).join(texture.source_path.as_ref())).unwrap())
                .as_slice()
        );
        assert_eq!(hex(&texture.source_sha256), expected);
    }
    let cloud = &compiled.textures[2];
    assert_eq!(cloud.role, AtmosphereRole::Clouds);
    assert_eq!(
        cloud.source_path.as_ref(),
        "textures/environment/clouds.png"
    );
    assert_eq!((cloud.width, cloud.height), (256, 256));
    assert_eq!(cloud.source_bytes, 7_880);
    assert_eq!(hex(&cloud.source_sha256), NATIVE_CLOUD_SOURCE_SHA256);
    assert_eq!(hex(&cloud.pixels_sha256), NATIVE_CLOUD_PIXELS_SHA256);
    assert_eq!(
        cloud
            .rgba8
            .chunks_exact(4)
            .filter(|pixel| pixel[3] >= 128)
            .count(),
        13_356
    );
    assert!(!cloud.source_path.contains(&clouds_override));
}

#[test]
fn production_compiler_rejects_any_manifest_bytes_other_than_the_tracked_pin() {
    let pack = synthetic_pack();
    let manifest = canonical_tracked_manifest();

    let mut appended = manifest.clone();
    appended.push(b' ');
    assert!(compile_atmosphere_assets(pack.path(), &appended).is_err());

    let mut bare_cr = manifest.clone();
    *bare_cr.iter_mut().find(|byte| **byte == b'\n').unwrap() = b'\r';
    assert!(compile_atmosphere_assets(pack.path(), &bare_cr).is_err());

    let mut mixed = manifest.clone();
    let first_lf = mixed.iter().position(|byte| *byte == b'\n').unwrap();
    mixed.insert(first_lf, b'\r');
    assert!(compile_atmosphere_assets(pack.path(), &mixed).is_err());

    let mut lines = std::str::from_utf8(&manifest)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    lines.swap(1, 2);
    let reordered = format!("{}\n", lines.join("\n"));
    assert!(compile_atmosphere_assets(pack.path(), reordered.as_bytes()).is_err());

    let changed = std::str::from_utf8(&manifest)
        .unwrap()
        .replace("v1.26.30.32-preview", "v1.26.30.31-preview");
    assert!(compile_atmosphere_assets(pack.path(), changed.as_bytes()).is_err());
}

#[test]
fn production_compiler_accepts_the_exact_pin_with_lf_or_crlf() {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        eprintln!("skipping: PINNED_VANILLA_PACK does not point at the ignored pinned pack");
        return;
    };
    let lf = canonical_tracked_manifest();
    let crlf = std::str::from_utf8(&lf)
        .unwrap()
        .replace('\n', "\r\n")
        .into_bytes();

    let from_lf = compile_atmosphere_assets(Path::new(&pack), &lf).unwrap();
    let from_crlf = compile_atmosphere_assets(Path::new(&pack), &crlf).unwrap();
    assert_eq!(from_crlf, from_lf);
    assert_eq!(
        from_crlf.source_manifest_sha256,
        Sha256::digest(&lf).as_slice()
    );
}

#[test]
fn production_compiler_rejects_each_modified_pinned_png() {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        eprintln!("skipping: PINNED_VANILLA_PACK does not point at the ignored pinned pack");
        return;
    };
    let manifest = tracked_manifest();

    for (mutated_path, _, _, _) in SOURCES {
        let candidate = tempfile::tempdir().unwrap();
        for (source_path, _, _, _) in SOURCES {
            let destination = candidate.path().join(source_path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(Path::new(&pack).join(source_path), &destination).unwrap();
        }
        let path = candidate.path().join(mutated_path);
        let mut image = image::open(&path).unwrap().into_rgba8();
        image.get_pixel_mut(0, 0).0[0] ^= 1;
        image.save(&path).unwrap();

        assert!(
            compile_atmosphere_assets(candidate.path(), &manifest).is_err(),
            "modified pinned source was accepted: {mutated_path}"
        );
    }
}

#[test]
fn compiler_carries_exact_sources_in_canonical_order_with_hashes() {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        eprintln!("skipping: PINNED_VANILLA_PACK does not point at the ignored pinned pack");
        return;
    };
    let manifest = tracked_manifest();
    let compiled =
        compile_atmosphere_assets(Path::new(&pack), &manifest).expect("compile atmosphere");

    assert_eq!(
        compiled.source_manifest_sha256,
        Sha256::digest(canonical_tracked_manifest()).as_slice()
    );
    assert_eq!(compiled.textures.len(), 3);
    for (index, texture) in compiled.textures.iter().enumerate() {
        let (path, width, height, _) = SOURCES[index];
        assert_eq!(texture.role, AtmosphereRole::ALL[index]);
        assert_eq!(texture.source_path.as_ref(), path);
        assert_eq!((texture.width, texture.height), (width, height));
        assert_eq!(
            texture.source_bytes as u64,
            fs::metadata(Path::new(&pack).join(path)).unwrap().len()
        );
        assert_eq!(
            texture.source_sha256,
            Sha256::digest(fs::read(Path::new(&pack).join(path)).unwrap()).as_slice()
        );
        assert_eq!(
            texture.pixels_sha256,
            Sha256::digest(&texture.rgba8).as_slice()
        );
    }
}

#[test]
fn compiler_rejects_missing_malformed_oversized_and_wrong_dimensions() {
    let bad_manifest = synthetic_pack();
    assert!(matches!(
        compile_atmosphere_assets(bad_manifest.path(), b"[]"),
        Err(AssetError::InvalidAtmosphereManifest { .. })
    ));

    let oversized_manifest = synthetic_pack();
    assert!(matches!(
        compile_atmosphere_assets(oversized_manifest.path(), &vec![b' '; 1024 * 1024 + 1]),
        Err(AssetError::AtmosphereManifestTooLarge { .. })
    ));

    let untrusted_manifest = String::from_utf8(MANIFEST.to_vec())
        .unwrap()
        .replace("https://github.com/Mojang", "https://example.invalid");
    assert!(matches!(
        compile_atmosphere_assets(bad_manifest.path(), untrusted_manifest.as_bytes()),
        Err(AssetError::InvalidAtmosphereProvenance { .. })
    ));

    let manifest = tracked_manifest();
    let missing = synthetic_pack();
    fs::remove_file(missing.path().join(SOURCES[0].0)).unwrap();
    assert!(matches!(
        compile_atmosphere_assets(missing.path(), &manifest),
        Err(AssetError::AtmosphereTextureIo { .. })
    ));

    let malformed = synthetic_pack();
    fs::write(malformed.path().join(SOURCES[0].0), b"not a png").unwrap();
    assert!(matches!(
        compile_atmosphere_assets(malformed.path(), &manifest),
        Err(AssetError::AtmosphereTextureHashMismatch { .. })
    ));

    let oversized = synthetic_pack();
    fs::write(
        oversized.path().join(SOURCES[0].0),
        vec![0_u8; 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(matches!(
        compile_atmosphere_assets(oversized.path(), &manifest),
        Err(AssetError::AtmosphereTextureTooLarge { .. })
    ));

    let wrong_size = synthetic_pack();
    write_png(&wrong_size.path().join(SOURCES[0].0), 31, 32, 0x44);
    assert!(matches!(
        compile_atmosphere_assets(wrong_size.path(), &manifest),
        Err(AssetError::AtmosphereTextureHashMismatch { .. })
    ));
}

#[test]
fn blob_is_deterministic_and_runtime_round_trips_every_record() {
    let compiled = synthetic_compiled();
    let first = encode_atmosphere_blob(&compiled).expect("encode atmosphere blob");
    let second = encode_atmosphere_blob(&compiled).expect("repeat atmosphere encoding");
    assert_eq!(first, second);

    let runtime = RuntimeAtmosphereAssets::decode(&first).expect("decode atmosphere blob");
    assert_eq!(
        runtime.source_manifest_sha256(),
        compiled.source_manifest_sha256
    );
    assert_eq!(runtime.textures(), compiled.textures.as_ref());
    for role in AtmosphereRole::ALL {
        assert_eq!(
            runtime.texture(role),
            compiled.textures.iter().find(|item| item.role == role)
        );
    }
}

#[test]
fn celestial_borders_decode_once_and_add_without_darkening_the_sky() {
    let blob = encode_atmosphere_blob(&synthetic_celestial_compiled()).unwrap();
    let runtime = RuntimeAtmosphereAssets::decode(&blob).unwrap();
    let borders = runtime
        .celestial_border_texels()
        .unwrap()
        .collect::<Vec<_>>();

    assert_eq!(borders.len(), 9 * (4 * 32 - 4));
    let mut seen = HashSet::new();
    for texel in &borders {
        assert!(
            seen.insert((texel.tile, texel.coordinate)),
            "duplicate celestial border coordinate: {texel:?}"
        );
        let expected = match texel.tile {
            CelestialTile::Sun => [1, 1, 0, 255],
            CelestialTile::MoonPhase(_) => [0, 0, 1, 255],
        };
        assert_eq!(texel.rgba8, expected);

        let source = [
            f32::from(texel.rgba8[0]) / 255.0,
            f32::from(texel.rgba8[1]) / 255.0,
            f32::from(texel.rgba8[2]) / 255.0,
        ];
        for destination in [[0.02, 0.03, 0.04], [0.8, 0.7, 0.6]] {
            let composed = composite_celestial(destination, source, 1.0);
            for channel in 0..3 {
                assert!(composed[channel] >= destination[channel]);
            }
        }
    }

    for phase in 0_u8..8 {
        assert_eq!(
            borders
                .iter()
                .filter(|texel| texel.tile == CelestialTile::MoonPhase(phase))
                .count(),
            4 * 32 - 4
        );
    }
    assert_eq!(
        borders
            .iter()
            .filter(|texel| texel.tile == CelestialTile::Sun)
            .count(),
        4 * 32 - 4
    );
}

#[test]
fn celestial_composition_retains_dark_lunar_detail_and_hdr_energy() {
    let destination = [0.8, 0.7, 0.6];
    let source = [2.0 / 255.0, 3.0 / 255.0, 4.0 / 255.0];
    let composed = composite_celestial(destination, source, 0.5);
    assert_eq!(
        composed,
        [
            destination[0] + source[0] * 0.5,
            destination[1] + source[1] * 0.5,
            destination[2] + source[2] * 0.5,
        ]
    );

    let hdr = composite_celestial([0.8, 0.7, 0.6], [1.0, 0.5, 0.25], 0.5);
    assert!(hdr[0] > 1.0, "celestial energy was clamped early: {hdr:?}");
}

#[test]
fn blob_rejects_noncanonical_or_corrupt_envelopes() {
    let original = valid_blob();

    let mut bad_magic = original.clone();
    bad_magic[0] ^= 1;
    assert!(RuntimeAtmosphereAssets::decode(&bad_magic).is_err());

    let mut bad_count = original.clone();
    bad_count[12..16].copy_from_slice(&2_u32.to_le_bytes());
    reseal(&mut bad_count);
    assert!(RuntimeAtmosphereAssets::decode(&bad_count).is_err());

    let mut bad_offset = original.clone();
    bad_offset[48..56].copy_from_slice(&129_u64.to_le_bytes());
    reseal(&mut bad_offset);
    assert!(RuntimeAtmosphereAssets::decode(&bad_offset).is_err());

    let descriptors = read_u64(&original, 48) as usize;
    let mut bad_dimensions = original.clone();
    bad_dimensions[descriptors + 4..descriptors + 8].copy_from_slice(&31_u32.to_le_bytes());
    reseal(&mut bad_dimensions);
    assert!(RuntimeAtmosphereAssets::decode(&bad_dimensions).is_err());

    let mut zero_source_hash = original.clone();
    zero_source_hash[descriptors + 48..descriptors + 80].fill(0);
    reseal(&mut zero_source_hash);
    assert!(RuntimeAtmosphereAssets::decode(&zero_source_hash).is_err());

    let mut zero_source_length = original.clone();
    zero_source_length[descriptors + 28..descriptors + 32].fill(0);
    reseal(&mut zero_source_length);
    assert!(RuntimeAtmosphereAssets::decode(&zero_source_length).is_err());

    let paths = read_u64(&original, 56) as usize;
    let mut bad_path = original.clone();
    bad_path[paths] = b'x';
    reseal(&mut bad_path);
    assert!(RuntimeAtmosphereAssets::decode(&bad_path).is_err());

    let payload = read_u64(&original, 64) as usize;
    let mut bad_pixels = original.clone();
    bad_pixels[payload] ^= 1;
    reseal(&mut bad_pixels);
    assert!(RuntimeAtmosphereAssets::decode(&bad_pixels).is_err());

    let mut trailing = original;
    trailing.push(0);
    assert!(RuntimeAtmosphereAssets::decode(&trailing).is_err());
}

#[test]
fn assetc_atmosphere_writes_deterministic_blob_and_provenance_report() {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        eprintln!("skipping: PINNED_VANILLA_PACK does not point at the ignored pinned pack");
        return;
    };
    let outputs = tempfile::tempdir().unwrap();
    let manifest_path = outputs.path().join("vanilla-source.json");
    let manifest = tracked_manifest();
    fs::write(&manifest_path, &manifest).unwrap();
    let first_blob = outputs.path().join("first.mcbeatm");
    let first_report = outputs.path().join("first.json");
    let second_blob = outputs.path().join("second.mcbeatm");
    let second_report = outputs.path().join("second.json");

    run_assetc(Path::new(&pack), &manifest_path, &first_blob, &first_report);
    run_assetc(
        Path::new(&pack),
        &manifest_path,
        &second_blob,
        &second_report,
    );
    assert_eq!(
        fs::read(&first_blob).unwrap(),
        fs::read(&second_blob).unwrap()
    );
    assert_eq!(
        fs::read(&first_report).unwrap(),
        fs::read(&second_report).unwrap()
    );

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&first_report).unwrap()).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(
        report["source"],
        serde_json::from_slice::<serde_json::Value>(&manifest).unwrap()
    );
    assert_eq!(
        report["source_manifest_sha256"],
        format!("{:x}", Sha256::digest(canonical_tracked_manifest()))
    );
    assert_eq!(report["textures"].as_array().unwrap().len(), 3);
    assert_eq!(report["textures"][0]["role"], "sun");
    assert_eq!(report["textures"][1]["role"], "moon_phases");
    assert_eq!(report["textures"][2]["role"], "clouds");
    assert_eq!(
        report["textures"][0]["source_path"],
        "textures/environment/sun.png"
    );
    assert!(
        report["blob_sha256"]
            .as_str()
            .is_some_and(|value| value.len() == 64)
    );
    let report_text = fs::read_to_string(first_report).unwrap();
    assert!(!report_text.contains(&pack));
}

#[test]
fn assetc_cloud_override_report_uses_only_canonical_logical_provenance() {
    let (Ok(pack), Ok(clouds_override)) = (
        std::env::var("PINNED_VANILLA_PACK"),
        std::env::var("CINNABAR_CLOUDS_PNG"),
    ) else {
        eprintln!(
            "skipping: PINNED_VANILLA_PACK and CINNABAR_CLOUDS_PNG must point at ignored local inputs"
        );
        return;
    };
    let outputs = tempfile::tempdir().unwrap();
    let manifest_path = outputs.path().join("vanilla-source.json");
    fs::write(&manifest_path, tracked_manifest()).unwrap();
    let blob = outputs.path().join("native.mcbeatm");
    let report = outputs.path().join("native.json");
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["atmosphere", "--pack"])
        .arg(&pack)
        .arg("--source-manifest")
        .arg(&manifest_path)
        .arg("--clouds-override")
        .arg(&clouds_override)
        .arg("--out")
        .arg(&blob)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "assetc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = fs::read_to_string(&report).unwrap();
    let report: serde_json::Value = serde_json::from_str(&report_text).unwrap();
    assert_eq!(
        report["textures"][2]["source_path"],
        "textures/environment/clouds.png"
    );
    assert_eq!(report["textures"][2]["source_bytes"], 7_880);
    assert_eq!(
        report["textures"][2]["source_sha256"],
        NATIVE_CLOUD_SOURCE_SHA256
    );
    assert_eq!(
        report["textures"][2]["pixels_sha256"],
        NATIVE_CLOUD_PIXELS_SHA256
    );
    assert!(!report_text.contains(&clouds_override));
}

#[test]
fn assetc_atmosphere_preserves_existing_output_when_report_cannot_publish() {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        eprintln!("skipping: PINNED_VANILLA_PACK does not point at the ignored pinned pack");
        return;
    };
    let outputs = tempfile::tempdir().unwrap();
    let manifest_path = outputs.path().join("vanilla-source.json");
    fs::write(&manifest_path, tracked_manifest()).unwrap();
    let blob = outputs.path().join("vanilla.mcbeatm");
    let report = outputs.path().join("report-destination");
    fs::write(&blob, b"old-blob").unwrap();
    fs::create_dir(&report).unwrap();
    fs::write(report.join("old-report-marker"), b"old-report").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["atmosphere", "--pack"])
        .arg(&pack)
        .arg("--source-manifest")
        .arg(&manifest_path)
        .arg("--out")
        .arg(&blob)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(fs::read(blob).unwrap(), b"old-blob");
    assert_eq!(
        fs::read(report.join("old-report-marker")).unwrap(),
        b"old-report"
    );
}

#[test]
fn assetc_atmosphere_rejects_exact_and_lexically_normalized_output_aliases() {
    let Some((pack, manifest_path, outputs)) = pinned_cli_fixture() else {
        return;
    };

    let exact = outputs.path().join("exact.bin");
    fs::write(&exact, b"exact-marker").unwrap();
    assert_alias_rejected_without_write(&pack, &manifest_path, &exact, &exact, b"exact-marker");

    let directory = outputs.path().join("lexical");
    fs::create_dir_all(directory.join("child")).unwrap();
    let blob = directory.join("output.bin");
    let report = directory.join("child").join("..").join("output.bin");
    fs::write(&blob, b"lexical-marker").unwrap();
    assert_alias_rejected_without_write(&pack, &manifest_path, &blob, &report, b"lexical-marker");
}

#[test]
fn assetc_atmosphere_rejects_absent_case_variant_outputs_on_every_platform() {
    let Some((pack, manifest_path, outputs)) = pinned_cli_fixture() else {
        return;
    };
    let blob = outputs.path().join("ASSET.BIN");
    let report = outputs.path().join("asset.bin");
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["atmosphere", "--pack"])
        .arg(&pack)
        .arg("--source-manifest")
        .arg(&manifest_path)
        .arg("--out")
        .arg(&blob)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!blob.exists(), "blob was created before alias rejection");
    assert!(
        !report.exists(),
        "report was created before alias rejection"
    );
}

#[test]
fn assetc_case_variant_guard_is_not_platform_gated() {
    let source = include_str!("../src/bin/assetc.rs");
    assert_eq!(
        source.matches("fn paths_alias(").count(),
        1,
        "case-fold alias comparison must have one platform-independent implementation"
    );
    assert!(!source.contains("#[cfg(windows)]\nfn paths_alias"));
    assert!(!source.contains("#[cfg(not(windows))]\nfn paths_alias"));
}

#[test]
fn assetc_atmosphere_rejects_hardlink_output_aliases() {
    let Some((pack, manifest_path, outputs)) = pinned_cli_fixture() else {
        return;
    };
    let blob = outputs.path().join("hardlink-blob.bin");
    let report = outputs.path().join("hardlink-report.json");
    fs::write(&blob, b"hardlink-marker").unwrap();
    fs::hard_link(&blob, &report).unwrap();

    assert_alias_rejected_without_write(&pack, &manifest_path, &blob, &report, b"hardlink-marker");
}

#[test]
fn assetc_atmosphere_rejects_symlink_output_aliases_when_supported() {
    let Some((pack, manifest_path, outputs)) = pinned_cli_fixture() else {
        return;
    };
    let (blob, report) = match symlink_alias_paths(outputs.path()) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("skipping symlink/junction alias case: {error}");
            return;
        }
    };
    fs::write(&blob, b"symlink-marker").unwrap();

    assert_alias_rejected_without_write(&pack, &manifest_path, &blob, &report, b"symlink-marker");
}

#[test]
fn pinned_pack_atmosphere_sources_match_exact_provenance() {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        return;
    };
    let manifest_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/vanilla-source.json");
    let manifest = fs::read(manifest_path).unwrap();
    let compiled = compile_atmosphere_assets(Path::new(&pack), &manifest).unwrap();
    let blob = encode_atmosphere_blob(&compiled).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(canonical_tracked_manifest())),
        "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6"
    );
    assert_eq!(blob.len(), 299_599);
    assert_eq!(
        format!("{:x}", Sha256::digest(&blob)),
        "d2f7e935744c7497741c1e54d022e676f67125c0fb006bf030b42734ba115054"
    );
    let expected = [
        (
            AtmosphereRole::Sun,
            "textures/environment/sun.png",
            32,
            32,
            "f7273544b691f08aaef76373d526e00793cf1e1aa0e1df8518f738d44a8e526b",
        ),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
            "01c566d48e0cc8618cf6fdce811b61175fc246f12f2e8f2c567d6acd3a2b35d8",
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
            "4f57cfe866779ef82be0058e244a77b0a279ee75e9eb40ac9ce6eb372445adc8",
        ),
    ];
    for (texture, (role, path, width, height, source_sha256)) in
        compiled.textures.iter().zip(expected)
    {
        assert_eq!(texture.role, role);
        assert_eq!(texture.source_path.as_ref(), path);
        assert_eq!((texture.width, texture.height), (width, height));
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(fs::read(Path::new(&pack).join(path)).unwrap())
            ),
            source_sha256
        );
        assert_eq!(format_hash(texture.source_sha256), source_sha256);
    }
}

fn format_hash(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn tracked_manifest() -> Vec<u8> {
    fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/vanilla-source.json"))
        .unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical_tracked_manifest() -> Vec<u8> {
    std::str::from_utf8(&tracked_manifest())
        .unwrap()
        .replace("\r\n", "\n")
        .into_bytes()
}

fn pinned_cli_fixture() -> Option<(String, std::path::PathBuf, TempDir)> {
    let Ok(pack) = std::env::var("PINNED_VANILLA_PACK") else {
        eprintln!("skipping: PINNED_VANILLA_PACK does not point at the ignored pinned pack");
        return None;
    };
    let outputs = tempfile::tempdir().unwrap();
    let manifest_path = outputs.path().join("vanilla-source.json");
    fs::write(&manifest_path, tracked_manifest()).unwrap();
    Some((pack, manifest_path, outputs))
}

fn assert_alias_rejected_without_write(
    pack: &str,
    manifest: &Path,
    blob: &Path,
    report: &Path,
    marker: &[u8],
) {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["atmosphere", "--pack"])
        .arg(pack)
        .arg("--source-manifest")
        .arg(manifest)
        .arg("--out")
        .arg(blob)
        .arg("--report")
        .arg(report)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "aliased outputs were accepted: {} / {}",
        blob.display(),
        report.display()
    );
    assert_eq!(fs::read(blob).unwrap(), marker);
    assert_eq!(fs::read(report).unwrap(), marker);
}

#[cfg(unix)]
fn symlink_alias_paths(root: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    let original = root.join("symlink-blob.bin");
    let link = root.join("symlink-report.json");
    std::os::unix::fs::symlink(&original, &link)?;
    Ok((original, link))
}

#[cfg(windows)]
fn symlink_alias_paths(root: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    let original = root.join("symlink-blob.bin");
    let link = root.join("symlink-report.json");
    match std::os::windows::fs::symlink_file(&original, &link) {
        Ok(()) => return Ok((original, link)),
        Err(source) if source.raw_os_error() == Some(1314) => {}
        Err(source) => return Err(source),
    }

    let target_directory = root.join("junction-target");
    let alias_directory = root.join("junction-alias");
    fs::create_dir(&target_directory)?;
    let output = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&alias_directory)
        .arg(&target_directory)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok((
        target_directory.join("output.bin"),
        alias_directory.join("output.bin"),
    ))
}

fn run_assetc(pack: &Path, manifest: &Path, out: &Path, report: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["atmosphere", "--pack"])
        .arg(pack)
        .arg("--source-manifest")
        .arg(manifest)
        .arg("--out")
        .arg(out)
        .arg("--report")
        .arg(report)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "assetc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn valid_blob() -> Vec<u8> {
    encode_atmosphere_blob(&synthetic_compiled())
        .unwrap()
        .into_vec()
}

fn reseal(bytes: &mut [u8]) {
    let payload_end = read_u64(bytes, 72) as usize;
    let digest = Sha256::digest(&bytes[..payload_end]);
    bytes[payload_end..payload_end + 32].copy_from_slice(&digest);
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn synthetic_pack() -> TempDir {
    let pack = tempfile::tempdir().unwrap();
    for (path, width, height, value) in SOURCES {
        write_png(&pack.path().join(path), width, height, value);
    }
    pack
}

fn synthetic_compiled() -> CompiledAtmosphereAssets {
    let pack = synthetic_pack();
    let textures = SOURCES
        .into_iter()
        .enumerate()
        .map(|(index, (source_path, width, height, value))| {
            let source = fs::read(pack.path().join(source_path)).unwrap();
            let rgba8 = vec![value; width as usize * height as usize * 4].into_boxed_slice();
            AtmosphereTexture {
                role: AtmosphereRole::ALL[index],
                source_path: source_path.into(),
                source_bytes: source.len() as u32,
                source_sha256: Sha256::digest(&source).into(),
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width,
                height,
                rgba8,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    CompiledAtmosphereAssets {
        source_manifest_sha256: Sha256::digest(MANIFEST).into(),
        textures,
    }
}

fn synthetic_celestial_compiled() -> CompiledAtmosphereAssets {
    let mut compiled = synthetic_compiled();
    let sun = compiled
        .textures
        .iter_mut()
        .find(|texture| texture.role == AtmosphereRole::Sun)
        .unwrap();
    fill_rgba(&mut sun.rgba8, [12, 16, 20, 255]);
    paint_tile_border(&mut sun.rgba8, sun.width, [0, 0], [1, 1, 0, 255]);
    sun.pixels_sha256 = Sha256::digest(&sun.rgba8).into();

    let moon = compiled
        .textures
        .iter_mut()
        .find(|texture| texture.role == AtmosphereRole::MoonPhases)
        .unwrap();
    fill_rgba(&mut moon.rgba8, [2, 3, 4, 255]);
    for phase in 0..8 {
        let origin = [(phase % 4) * 32, (phase / 4) * 32];
        paint_tile_border(&mut moon.rgba8, moon.width, origin, [0, 0, 1, 255]);
    }
    moon.pixels_sha256 = Sha256::digest(&moon.rgba8).into();
    compiled
}

fn fill_rgba(pixels: &mut [u8], rgba: [u8; 4]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&rgba);
    }
}

fn paint_tile_border(pixels: &mut [u8], width: u32, origin: [u32; 2], rgba: [u8; 4]) {
    for y in 0..32 {
        for x in 0..32 {
            if x == 0 || x == 31 || y == 0 || y == 31 {
                let atlas_x = origin[0] + x;
                let atlas_y = origin[1] + y;
                let offset = ((atlas_y * width + atlas_x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&rgba);
            }
        }
    }
}

fn write_png(path: &Path, width: u32, height: u32, value: u8) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    RgbaImage::from_pixel(width, height, Rgba([value; 4]))
        .save(path)
        .unwrap();
}
