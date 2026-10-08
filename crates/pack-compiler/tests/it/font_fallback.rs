use pack_compiler::{
    GlyphAdvances, OutlineFontConfig, compile_outline_font, compile_outline_font_with_fallback,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn verify_input(bytes: &[u8], source: &serde_json::Value, field: &str) -> [u8; 32] {
    assert_eq!(
        bytes.len() as u64,
        source[format!("{field}_size_bytes")].as_u64().unwrap()
    );
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        source[format!("{field}_sha256")].as_str().unwrap()
    );
    digest
}

#[test]
#[ignore = "requires both explicitly fetched pinned outline sources"]
fn two_provider_carrier_is_deterministic_and_preserves_primary_page_and_metrics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = fs::read(root.join("assets/ui-font-source.json")).unwrap();
    let source: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    let path = |prefix: &str| {
        root.join(".local/assets/ui-font")
            .join(source[format!("{prefix}commit")].as_str().unwrap())
            .join(source[format!("{prefix}font_file")].as_str().unwrap())
    };
    let primary_path = path("");
    let fallback_path = path("fallback_");
    if !primary_path.is_file() || !fallback_path.is_file() {
        eprintln!("skipping: pinned outline font sources are absent");
        return;
    }
    let primary = fs::read(&primary_path).unwrap();
    let fallback = fs::read(&fallback_path).unwrap();
    let primary_hash = verify_input(&primary, &source, "font");
    let fallback_hash = verify_input(&fallback, &source, "fallback_font");
    for (font_path, prefix) in [(&primary_path, ""), (&fallback_path, "fallback_")] {
        let license = fs::read(
            font_path
                .parent()
                .unwrap()
                .join(source[format!("{prefix}license_file")].as_str().unwrap()),
        )
        .unwrap();
        verify_input(&license, &source, &format!("{prefix}license"));
    }
    let identity = assets::canonical_source_manifest_sha256(&manifest);
    let config = OutlineFontConfig {
        advances: GlyphAdvances::InkPlusGap {
            gap_px: 2,
            blank_advance_px: Some(8),
        },
        ..OutlineFontConfig::default()
    };
    let original = compile_outline_font(&primary_path, &primary, identity, config).unwrap();
    let merged = compile_outline_font_with_fallback(
        &primary_path,
        &primary,
        &fallback_path,
        &fallback,
        identity,
        config,
    )
    .unwrap();
    let repeat = compile_outline_font_with_fallback(
        &primary_path,
        &primary,
        &fallback_path,
        &fallback,
        identity,
        config,
    )
    .unwrap();
    assert_eq!(merged.bytes, repeat.bytes);
    let old = assets::RuntimeFontCatalog::decode(&original.bytes, identity).unwrap();
    let new = assets::RuntimeFontCatalog::decode(&merged.bytes, identity).unwrap();
    assert_eq!(old.pages()[0].pixels, new.pages()[0].pixels);
    assert_eq!(new.pages()[0].source_sha256, primary_hash);
    assert_eq!(new.pages()[0].source_bytes as usize, primary.len());
    for glyph in old.glyphs() {
        assert_eq!(Some(glyph), new.glyph(glyph.codepoint));
    }
    assert!(new.pages().len() <= 4);
    for page in &new.pages()[1..] {
        assert_eq!(page.source_bytes as usize, fallback.len());
        assert_eq!(page.source_sha256, fallback_hash);
    }
    // Exact present coverage of this pinned source, not universal Unicode support.
    for (first, last, present) in [
        (0x2190, 0x23ff, 129),
        (0x2460, 0x27bf, 489),
        (0x3000, 0x30ff, 253),
        (0x3400, 0x4dbf, 6582),
        (0x4e00, 0x9fff, 20976),
    ] {
        assert_eq!(
            new.glyphs()
                .iter()
                .filter(|glyph| (first..=last).contains(&u32::from(glyph.codepoint)))
                .count(),
            present
        );
    }
}

#[test]
#[ignore = "requires the explicitly built, hash-bound local font carrier"]
fn compiled_ui_font_covers_declared_native_samples() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let carrier = root.join(".local/assets/compiled/ui-monocraft-v1.mcbefont");
    if !carrier.is_file() {
        eprintln!("skipping: local font qualification carrier is absent");
        return;
    }
    let bytes = fs::read(carrier).expect("read the local font qualification carrier");
    let manifest = fs::read(root.join("assets/ui-font-source.json")).unwrap();
    let identity = assets::canonical_source_manifest_sha256(&manifest);
    let catalog = assets::RuntimeFontCatalog::decode(&bytes, identity).unwrap();
    for codepoint in [
        '\u{2713}', '\u{2694}', '\u{2620}', '\u{4e16}', '\u{754c}', '\u{7b2c}', '\u{4e8c}',
    ] {
        let glyph = catalog.glyph(codepoint).unwrap_or_else(|| {
            panic!(
                "required U+{:04X} is absent from the carrier",
                u32::from(codepoint)
            )
        });
        assert!(glyph.advance_64 > 0);
        assert!(glyph.uv[0] < glyph.uv[2] && glyph.uv[1] < glyph.uv[3]);
    }
}
