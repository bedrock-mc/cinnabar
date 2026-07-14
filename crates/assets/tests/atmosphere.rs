use std::{fs, path::Path, process::Command};

use assets::{
    AssetError, AtmosphereRole, RuntimeAtmosphereAssets, compile_atmosphere_assets,
    encode_atmosphere_blob,
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

#[test]
fn compiler_carries_exact_sources_in_canonical_order_with_hashes() {
    let pack = synthetic_pack();
    let compiled = compile_atmosphere_assets(pack.path(), MANIFEST).expect("compile atmosphere");

    assert_eq!(
        compiled.source_manifest_sha256,
        Sha256::digest(MANIFEST).as_slice()
    );
    assert_eq!(compiled.textures.len(), 3);
    for (index, texture) in compiled.textures.iter().enumerate() {
        let (path, width, height, value) = SOURCES[index];
        assert_eq!(texture.role, AtmosphereRole::ALL[index]);
        assert_eq!(texture.source_path.as_ref(), path);
        assert_eq!((texture.width, texture.height), (width, height));
        assert_eq!(
            texture.source_bytes as u64,
            fs::metadata(pack.path().join(path)).unwrap().len()
        );
        assert_eq!(
            texture.rgba8.as_ref(),
            vec![value; width as usize * height as usize * 4]
        );
        assert_eq!(
            texture.source_sha256,
            Sha256::digest(fs::read(pack.path().join(path)).unwrap()).as_slice()
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

    let missing = synthetic_pack();
    fs::remove_file(missing.path().join(SOURCES[0].0)).unwrap();
    assert!(matches!(
        compile_atmosphere_assets(missing.path(), MANIFEST),
        Err(AssetError::AtmosphereTextureIo { .. })
    ));

    let malformed = synthetic_pack();
    fs::write(malformed.path().join(SOURCES[0].0), b"not a png").unwrap();
    assert!(matches!(
        compile_atmosphere_assets(malformed.path(), MANIFEST),
        Err(AssetError::AtmosphereTextureDecode { .. })
    ));

    let oversized = synthetic_pack();
    fs::write(
        oversized.path().join(SOURCES[0].0),
        vec![0_u8; 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(matches!(
        compile_atmosphere_assets(oversized.path(), MANIFEST),
        Err(AssetError::AtmosphereTextureTooLarge { .. })
    ));

    let wrong_size = synthetic_pack();
    write_png(&wrong_size.path().join(SOURCES[0].0), 31, 32, 0x44);
    assert!(matches!(
        compile_atmosphere_assets(wrong_size.path(), MANIFEST),
        Err(AssetError::WrongAtmosphereTextureDimensions { .. })
    ));
}

#[test]
fn blob_is_deterministic_and_runtime_round_trips_every_record() {
    let pack = synthetic_pack();
    let compiled = compile_atmosphere_assets(pack.path(), MANIFEST).unwrap();
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
    let pack = synthetic_pack();
    let outputs = tempfile::tempdir().unwrap();
    let manifest_path = outputs.path().join("vanilla-source.json");
    fs::write(&manifest_path, MANIFEST).unwrap();
    let first_blob = outputs.path().join("first.mcbeatm");
    let first_report = outputs.path().join("first.json");
    let second_blob = outputs.path().join("second.mcbeatm");
    let second_report = outputs.path().join("second.json");

    run_assetc(pack.path(), &manifest_path, &first_blob, &first_report);
    run_assetc(pack.path(), &manifest_path, &second_blob, &second_report);
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
        serde_json::from_slice::<serde_json::Value>(MANIFEST).unwrap()
    );
    assert_eq!(
        report["source_manifest_sha256"],
        format!("{:x}", Sha256::digest(MANIFEST))
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
    assert!(!report_text.contains(&pack.path().to_string_lossy().to_string()));
}

#[test]
fn assetc_atmosphere_preserves_existing_output_when_report_cannot_publish() {
    let pack = synthetic_pack();
    let outputs = tempfile::tempdir().unwrap();
    let manifest_path = outputs.path().join("vanilla-source.json");
    fs::write(&manifest_path, MANIFEST).unwrap();
    let blob = outputs.path().join("vanilla.mcbeatm");
    let report = outputs.path().join("report-destination");
    fs::write(&blob, b"old-blob").unwrap();
    fs::create_dir(&report).unwrap();
    fs::write(report.join("old-report-marker"), b"old-report").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["atmosphere", "--pack"])
        .arg(pack.path())
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
        format!("{:x}", Sha256::digest(&manifest)),
        "0cc3e494d634cf3f9c0795d526b9f91e973dfe1009aae50b8db4418f2386304d"
    );
    assert_eq!(blob.len(), 299_599);
    assert_eq!(
        format!("{:x}", Sha256::digest(&blob)),
        "0fef7cab3c6b420af08517f8f0c7b5c98556ba15aeb2961df9fcd16c3df3470c"
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
    let pack = synthetic_pack();
    let compiled = compile_atmosphere_assets(pack.path(), MANIFEST).unwrap();
    encode_atmosphere_blob(&compiled).unwrap().into_vec()
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

fn write_png(path: &Path, width: u32, height: u32, value: u8) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    RgbaImage::from_pixel(width, height, Rgba([value; 4]))
        .save(path)
        .unwrap();
}
