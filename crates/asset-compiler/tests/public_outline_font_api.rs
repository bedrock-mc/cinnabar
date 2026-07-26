use std::{fs, path::Path, process::Command};

use asset_compiler::{FontCompileError, GlyphAdvances, OutlineFontConfig, compile_outline_font};
use sha2::{Digest, Sha256};

#[test]
fn invalid_outline_font_fails_closed() {
    let error = compile_outline_font(
        Path::new("font/Monocraft.ttf"),
        b"not a font",
        [0x42; 32],
        OutlineFontConfig::default(),
    )
    .unwrap_err();

    assert!(matches!(error, FontCompileError::OutlineFont { .. }));
}

#[test]
fn outline_font_config_rejects_unbounded_or_incomplete_ranges() {
    let missing_replacement = OutlineFontConfig {
        replacement_codepoint: 'A',
        ..OutlineFontConfig::default()
    };
    assert!(matches!(
        compile_outline_font(
            Path::new("font/Monocraft.ttf"),
            b"not a font",
            [0x42; 32],
            missing_replacement,
        ),
        Err(FontCompileError::InvalidDescriptor { .. })
    ));

    let oversized = OutlineFontConfig {
        pixel_height: 4_097,
        ..OutlineFontConfig::default()
    };
    assert!(matches!(
        compile_outline_font(
            Path::new("font/Monocraft.ttf"),
            b"not a font",
            [0x42; 32],
            oversized,
        ),
        Err(FontCompileError::InvalidDescriptor { .. })
    ));
}

#[test]
fn outline_font_cli_rejects_bytes_outside_the_manifest_pin_before_rasterization() {
    let directory = tempfile::tempdir().unwrap();
    let font = directory.path().join("tampered.ttf");
    let manifest = directory.path().join("ui-font-source.json");
    let out = directory.path().join("font.mcbefont");
    let report = directory.path().join("font.json");
    let tampered = b"not the pinned font";
    fs::write(&font, tampered).unwrap();
    fs::write(
        &manifest,
        format!(
            concat!(
                "{{\n",
                "  \"font_size_bytes\": {},\n",
                "  \"font_sha256\": \"{}\",\n",
                "  \"rasterization\": {{\n",
                "    \"pixel_height\": 32,\n",
                "    \"atlas_side\": 2048,\n",
                "    \"replacement_codepoint\": 65533\n",
                "  }}\n",
                "}}\n"
            ),
            tampered.len(),
            "00".repeat(32)
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["outline-font-assets", "--font"])
        .arg(&font)
        .arg("--source-manifest")
        .arg(&manifest)
        .arg("--out")
        .arg(&out)
        .arg("--report")
        .arg(&report)
        .output()
        .expect("run outline-font-assets");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("outline font SHA-256 does not match the source manifest"),
        "unexpected rejection:\n{stderr}"
    );
    assert!(!out.exists());
    assert!(!report.exists());
}

#[test]
fn tracked_monocraft_license_is_the_exact_pinned_upstream_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("assets/licenses/Monocraft-OFL-1.1.txt");
    let bytes = fs::read(path).unwrap();

    assert_eq!(bytes.len(), 4_377);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "f69c147003e052dbc9d96c40a9f73647e72766cfda95a597b94ed827fe25acb1"
    );
    let attributes = fs::read_to_string(root.join(".gitattributes"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        attributes.lines().any(|line| {
            line == concat!(
                "assets/licenses/Monocraft-OFL-1.1.txt text eol=lf ",
                "whitespace=-blank-at-eol"
            )
        }),
        "the byte-exact license must retain LF bytes in fresh Windows checkouts"
    );
}

#[test]
fn proportional_advance_configuration_fails_closed() {
    for advances in [
        GlyphAdvances::InkPlusGap {
            gap_px: 0,
            blank_advance_px: None,
        },
        GlyphAdvances::InkPlusGap {
            gap_px: 19,
            blank_advance_px: None,
        },
        GlyphAdvances::InkPlusGap {
            gap_px: 2,
            blank_advance_px: Some(0),
        },
        GlyphAdvances::InkPlusGap {
            gap_px: 2,
            blank_advance_px: Some(37),
        },
    ] {
        assert!(
            matches!(
                compile_outline_font(
                    Path::new("font/Monocraft.ttf"),
                    b"not a font",
                    [0x42; 32],
                    OutlineFontConfig {
                        pixel_height: 18,
                        advances,
                        ..OutlineFontConfig::default()
                    },
                ),
                Err(FontCompileError::InvalidDescriptor { .. })
            ),
            "{advances:?} must be rejected"
        );
    }
}

/// The pinned font is a fetched artifact under gitignored `.local/`, so a fresh
/// checkout has nothing to measure until `make font-assets` runs.
fn pinned_font() -> Option<Vec<u8>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(concat!(
        "../../.local/assets/ui-font/",
        "e498bf70aeb25b4bdcff1e44d878fb2cb4f7c2a9/Monocraft.ttf"
    ));
    fs::read(path).ok()
}

fn compile_pinned(advances: GlyphAdvances) -> Option<assets::RuntimeFontCatalog> {
    let bytes = pinned_font()?;
    let manifest = [0x51; 32];
    let compiled = compile_outline_font(
        Path::new("font/Monocraft.ttf"),
        &bytes,
        manifest,
        OutlineFontConfig {
            pixel_height: 18,
            atlas_side: 1_024,
            advances,
            ..OutlineFontConfig::default()
        },
    )
    .expect("the pinned font compiles at its native pixel height");
    Some(assets::RuntimeFontCatalog::decode(&compiled.bytes, manifest).unwrap())
}

#[test]
fn pinned_monocraft_rasterizes_on_its_native_texel_grid() {
    let Some(catalog) = compile_pinned(GlyphAdvances::Source) else {
        return;
    };
    // Monocraft's outline coordinates are multiples of 60 against a 1080-unit
    // em, so 18 px/em puts every edge on a texel boundary and coverage is
    // strictly binary. An off-grid pixel height reintroduces partial alpha.
    let page = &catalog.pages()[0];
    let mut partial = 0usize;
    for glyph in catalog.glyphs() {
        for y in glyph.uv[1]..glyph.uv[3] {
            for x in glyph.uv[0]..glyph.uv[2] {
                let alpha = page.rgba8[(u32::from(y) * page.width + u32::from(x)) as usize * 4 + 3];
                if alpha != 0 && alpha != 255 {
                    partial += 1;
                }
            }
        }
    }
    assert_eq!(partial, 0, "18 px/em must not antialias a pixel font");
}

#[test]
fn packed_glyphs_carry_no_blank_border_row_or_column() {
    let Some(catalog) = compile_pinned(GlyphAdvances::Source) else {
        return;
    };
    let page = &catalog.pages()[0];
    let inked = |x: u16, y: u16| {
        page.rgba8[(u32::from(y) * page.width + u32::from(x)) as usize * 4 + 3] != 0
    };
    for glyph in catalog.glyphs() {
        let [x0, y0, x1, y1] = glyph.uv;
        if x1 - x0 == 1 && y1 - y0 == 1 {
            continue;
        }
        assert!(
            (x0..x1).any(|x| inked(x, y0))
                && (x0..x1).any(|x| inked(x, y1 - 1))
                && (y0..y1).any(|y| inked(x0, y))
                && (y0..y1).any(|y| inked(x1 - 1, y)),
            "glyph {:?} kept a fully transparent border",
            glyph.codepoint
        );
    }
}

#[test]
fn proportional_advances_measure_trimmed_ink_plus_the_gap() {
    let Some(monospace) = compile_pinned(GlyphAdvances::Source) else {
        return;
    };
    let proportional = compile_pinned(GlyphAdvances::InkPlusGap {
        gap_px: 2,
        blank_advance_px: Some(8),
    })
    .expect("the pinned font was already readable");

    // Monocraft is monospace across ASCII -- one design pixel is two texels and
    // every printable advance is six design pixels. Wider cells exist outside
    // ASCII, which is exactly why the reflow below has to clamp.
    for codepoint in ' '..='~' {
        assert_eq!(
            monospace.glyph(codepoint).unwrap().advance_64,
            12 * 64,
            "glyph {codepoint:?}"
        );
    }

    for (codepoint, expected_px) in [('.', 4), ('!', 4), (':', 4), ('I', 8), ('a', 12), (' ', 8)] {
        let glyph = proportional.glyph(codepoint).unwrap();
        assert_eq!(
            glyph.advance_64,
            expected_px * 64,
            "advance for {codepoint:?}"
        );
    }

    // The reflow only ever tightens. Accented, symbol, arrow, and box-drawing
    // glyphs whose ink already fills their cell must keep the font's advance
    // instead of gaining the gap and drifting out of alignment.
    for glyph in proportional.glyphs() {
        let source = monospace.glyph(glyph.codepoint).unwrap();
        assert!(
            glyph.advance_64 <= source.advance_64,
            "glyph {:?} widened from {} to {}",
            glyph.codepoint,
            source.advance_64,
            glyph.advance_64
        );
    }
}
