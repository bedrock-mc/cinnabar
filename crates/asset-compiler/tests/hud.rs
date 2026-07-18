use std::{fs, path::Path};

use asset_compiler::{HudCompileError, compile_hud_assets};
use assets::{HudTextureRole, RuntimeHudCatalog};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../assets/vanilla-source.json");

#[test]
fn compiler_reads_only_exact_pinned_heart_assets() {
    let pack = fixture_pack(9, 9);
    let compiled = compile_hud_assets(pack.path(), MANIFEST).unwrap();
    let runtime = RuntimeHudCatalog::decode(
        &compiled.bytes,
        compiled.source_manifest_sha256,
    )
    .unwrap();

    assert_eq!(runtime.textures().len(), 3);
    assert_eq!(runtime.texture(HudTextureRole::HeartFull).width, 9);
    assert_eq!(
        runtime.texture(HudTextureRole::HeartBackground).rgba8[0],
        1
    );
}

#[test]
fn compiler_rejects_wrong_geometry_and_manifest() {
    let pack = fixture_pack(8, 9);
    assert!(matches!(
        compile_hud_assets(pack.path(), MANIFEST),
        Err(HudCompileError::WrongDimensions { width: 8, .. })
    ));
    let pack = fixture_pack(9, 9);
    assert!(matches!(
        compile_hud_assets(pack.path(), b"{}"),
        Err(HudCompileError::SourceManifestMismatch)
    ));
}

fn fixture_pack(width: u32, height: u32) -> TempDir {
    let directory = TempDir::new().unwrap();
    for (index, role) in HudTextureRole::ALL.into_iter().enumerate() {
        let path = directory.path().join(role.source_path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_png(&path, width, height, u8::try_from(index + 1).unwrap());
    }
    directory
}

fn write_png(path: &Path, width: u32, height: u32, value: u8) {
    let rgba = vec![value; usize::try_from(width * height * 4).unwrap()];
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(path, bytes).unwrap();
}
