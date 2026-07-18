use std::path::Path;

use asset_compiler::{FontCompileError, OutlineFontConfig, compile_outline_font};

#[test]
fn invalid_outline_font_fails_closed() {
    let error = compile_outline_font(
        Path::new("font/Inter.ttf"),
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
            Path::new("font/Inter.ttf"),
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
            Path::new("font/Inter.ttf"),
            b"not a font",
            [0x42; 32],
            oversized,
        ),
        Err(FontCompileError::InvalidDescriptor { .. })
    ));
}
