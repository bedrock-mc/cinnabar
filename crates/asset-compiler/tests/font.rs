#[path = "../../assets/src/font.rs"]
mod font_carrier;

mod font_compiler {
    use crate::font_carrier as assets;

    include!("../src/font.rs");
}

use std::{fs, path::Path};

use font_compiler::{FontCompileError, compile_fonts};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

const PINNED_SOURCE_SHA256: &str =
    "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6";

#[test]
fn font_carrier_is_deterministic_and_bounded() {
    let pack = fixture_pack_with_ascii_and_unicode_pages();
    let a = compile_fonts(pack.path()).unwrap();
    let b = compile_fonts(pack.path()).unwrap();

    assert_eq!(a.bytes, b.bytes);
    assert_eq!(&a.bytes[..9], b"MCBEFONT1");
    assert_eq!(a.report.glyphs, 2);
    assert_eq!(a.report.pages, 2);
    assert!(a.report.glyphs <= 65_536);
    assert!(a.report.pages <= 256);
    let runtime =
        font_carrier::RuntimeFontCatalog::decode(&a.bytes, a.report.source_manifest_sha256)
            .unwrap();
    assert_eq!(runtime.identity().carrier_sha256, a.report.carrier_sha256);
    assert_eq!(runtime.glyphs().len(), 2);
    assert_eq!(runtime.glyph('A').unwrap().advance_64, 512);
    assert_eq!(runtime.pages().len(), 2);
    assert!(
        runtime.pages()[0]
            .rgba8
            .chunks_exact(4)
            .all(|pixel| pixel == [0x7f; 4])
    );
}

#[test]
fn malformed_metrics_and_oversized_pages_fail_closed() {
    let malformed = fixture_pack(
        r#"{
            "schema": 1,
            "source_manifest_sha256": "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6",
            "pages": [{"source": "font/default8.png"}],
            "glyphs": [{
                "codepoint": 65,
                "page": "font/default8.png",
                "uv": [0, 0, 8, 8],
                "bearing": [0, 0],
                "advance": "NaN"
            }]
        }"#,
        &[("font/default8.png", 16, 16)],
    );
    assert!(matches!(
        compile_fonts(malformed.path()),
        Err(FontCompileError::NonFiniteMetric {
            codepoint: 65,
            field: "advance"
        })
    ));

    let oversized = fixture_pack(
        descriptor_for_single_page("font/glyph_01.png", 0x100).as_str(),
        &[("font/glyph_01.png", 4_097, 1)],
    );
    assert!(matches!(
        compile_fonts(oversized.path()),
        Err(FontCompileError::PageTooLarge {
            width: 4_097,
            height: 1,
            ..
        })
    ));
}

#[test]
fn duplicate_semantic_glyphs_and_wrong_provenance_fail_closed() {
    let duplicate = fixture_pack(
        r#"{
            "schema": 1,
            "source_manifest_sha256": "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6",
            "pages": [
                {"source": "font/default8.png"},
                {"source": "font/glyph_00.png"}
            ],
            "glyphs": [
                {"codepoint": 65, "page": "font/default8.png", "uv": [0,0,8,8], "bearing": [0,0], "advance": 8},
                {"codepoint": 65, "page": "font/glyph_00.png", "uv": [0,0,8,8], "bearing": [0,0], "advance": 8}
            ]
        }"#,
        &[("font/default8.png", 16, 16), ("font/glyph_00.png", 16, 16)],
    );
    assert!(matches!(
        compile_fonts(duplicate.path()),
        Err(FontCompileError::DuplicateGlyph { codepoint: 65 })
    ));

    let wrong_provenance = fixture_pack(
        &descriptor_for_single_page("font/default8.png", 65)
            .replace(PINNED_SOURCE_SHA256, &"00".repeat(32)),
        &[("font/default8.png", 16, 16)],
    );
    assert!(matches!(
        compile_fonts(wrong_provenance.path()),
        Err(FontCompileError::SourceManifestMismatch)
    ));
}

#[test]
fn glyph_and_page_order_do_not_change_carrier_bytes() {
    let ordered = fixture_pack_with_ascii_and_unicode_pages();
    let reversed = fixture_pack(
        r#"{
            "schema": 1,
            "source_manifest_sha256": "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6",
            "pages": [
                {"source": "font/glyph_01.png"},
                {"source": "font/default8.png"}
            ],
            "glyphs": [
                {"codepoint": 256, "page": "font/glyph_01.png", "uv": [8,0,16,8], "bearing": [1,-1], "advance": 8.5},
                {"codepoint": 65, "page": "font/default8.png", "uv": [0,0,8,8], "bearing": [0,0], "advance": 8}
            ]
        }"#,
        &[("font/default8.png", 16, 16), ("font/glyph_01.png", 16, 16)],
    );

    assert_eq!(
        compile_fonts(ordered.path()).unwrap().bytes,
        compile_fonts(reversed.path()).unwrap().bytes
    );
}

#[test]
fn intermediate_directory_link_cannot_escape_the_pack_root() {
    let directory = tempfile::tempdir().unwrap();
    let font = directory.path().join("font");
    let outside = directory.path().join("outside");
    fs::create_dir(&font).unwrap();
    fs::create_dir(&outside).unwrap();
    write_png(&outside.join("escaped.png"), 1, 1);
    create_directory_link(&font.join("nested"), &outside).unwrap();
    fs::write(
        font.join("catalog.json"),
        descriptor_for_single_page("font/nested/escaped.png", 65),
    )
    .unwrap();

    assert!(compile_fonts(directory.path()).is_err());
}

#[test]
fn nested_normal_page_source_is_rejected_by_the_flat_font_contract() {
    let directory = tempfile::tempdir().unwrap();
    let font = directory.path().join("font");
    fs::create_dir_all(font.join("nested")).unwrap();
    write_png(&font.join("nested/page.png"), 1, 1);
    fs::write(
        font.join("catalog.json"),
        descriptor_for_single_page("font/nested/page.png", 65),
    )
    .unwrap();

    assert!(matches!(
        compile_fonts(directory.path()),
        Err(FontCompileError::InvalidDescriptor { .. })
    ));
}

fn fixture_pack_with_ascii_and_unicode_pages() -> tempfile::TempDir {
    fixture_pack(
        r#"{
            "schema": 1,
            "source_manifest_sha256": "c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6",
            "pages": [
                {"source": "font/default8.png"},
                {"source": "font/glyph_01.png"}
            ],
            "glyphs": [
                {"codepoint": 65, "page": "font/default8.png", "uv": [0,0,8,8], "bearing": [0,0], "advance": 8},
                {"codepoint": 256, "page": "font/glyph_01.png", "uv": [8,0,16,8], "bearing": [1,-1], "advance": 8.5}
            ]
        }"#,
        &[("font/default8.png", 16, 16), ("font/glyph_01.png", 16, 16)],
    )
}

fn descriptor_for_single_page(path: &str, codepoint: u32) -> String {
    format!(
        r#"{{
            "schema": 1,
            "source_manifest_sha256": "{PINNED_SOURCE_SHA256}",
            "pages": [{{"source": "{path}"}}],
            "glyphs": [{{"codepoint": {codepoint}, "page": "{path}", "uv": [0,0,1,1], "bearing": [0,0], "advance": 1}}]
        }}"#
    )
}

fn fixture_pack(descriptor: &str, pages: &[(&str, u32, u32)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("font")).unwrap();
    fs::write(directory.path().join("font/catalog.json"), descriptor).unwrap();
    for (relative, width, height) in pages {
        write_png(&directory.path().join(relative), *width, *height);
    }
    directory
}

fn write_png(path: &Path, width: u32, height: u32) {
    let pixels = vec![0x7f; (width * height * 4) as usize];
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(path, encoded).unwrap();
}

#[cfg(unix)]
fn create_directory_link(link: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link(link: &Path, target: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "mklink /J failed with {status}"
        )))
    }
}
