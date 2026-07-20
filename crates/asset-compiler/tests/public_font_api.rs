use std::{fs, path::Path, process::Command};

use asset_compiler::{CompiledFontCarrier, FontCompileError, FontCompileReport, compile_fonts};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

fn assert_public_type<T>() {}

#[test]
fn font_compiler_surface_is_public() {
    assert_public_type::<CompiledFontCarrier>();
    assert_public_type::<FontCompileError>();
    assert_public_type::<FontCompileReport>();
    let _ = compile_fonts;
}

#[test]
fn assetc_registers_the_source_manifest_aware_font_assets_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["font-assets", "--help"])
        .output()
        .expect("run assetc font-assets help");
    assert!(
        output.status.success(),
        "font-assets help failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    for required in ["--pack", "--source-manifest", "--out", "--report"] {
        assert!(
            stdout.contains(required),
            "missing {required} in:\n{stdout}"
        );
    }
}

#[test]
fn assetc_font_assets_binds_outputs_to_the_exact_source_manifest() {
    let directory = tempfile::tempdir().unwrap();
    let pack = directory.path().join("pack");
    let font = pack.join("font");
    fs::create_dir_all(&font).unwrap();
    fs::write(
        font.join("catalog.json"),
        r#"{
            "schema": 1,
            "source_manifest_sha256": "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6",
            "pages": [{"source": "font/default8.png"}],
            "glyphs": [{"codepoint": 65, "page": "font/default8.png", "uv": [0,0,1,1], "bearing": [0,0], "advance": 1}]
        }"#,
    )
    .unwrap();
    write_png(&font.join("default8.png"));
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/vanilla-source.json");
    let out = directory.path().join("font.mcbefont");
    let report = directory.path().join("font-assets.json");
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["font-assets", "--pack"])
        .arg(&pack)
        .arg("--source-manifest")
        .arg(&manifest)
        .arg("--out")
        .arg(&out)
        .arg("--report")
        .arg(&report)
        .output()
        .expect("run assetc font-assets");
    assert!(
        output.status.success(),
        "font-assets failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(&fs::read(&out).unwrap()[..9], b"MCBEFONT1");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report).unwrap()).expect("font report JSON");
    assert_eq!(
        report["source_manifest_sha256"],
        "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6"
    );
    assert_eq!(report["counts"]["glyphs"], 1);
    assert_eq!(report["counts"]["pages"], 1);

    let wrong_manifest = directory.path().join("wrong-source.json");
    fs::write(&wrong_manifest, b"{}\n").unwrap();
    let rejected_out = directory.path().join("rejected.mcbefont");
    let rejected_report = directory.path().join("rejected.json");
    let rejected = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["font-assets", "--pack"])
        .arg(&pack)
        .arg("--source-manifest")
        .arg(&wrong_manifest)
        .arg("--out")
        .arg(&rejected_out)
        .arg("--report")
        .arg(&rejected_report)
        .output()
        .expect("run mismatched assetc font-assets");
    assert!(!rejected.status.success());
    assert!(!rejected_out.exists());
    assert!(!rejected_report.exists());
}

fn write_png(path: &Path) {
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&[1, 2, 3, 4], 1, 1, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(path, encoded).unwrap();
}
