use assets::{FontLineMetrics, RuntimeFontCatalog};

use super::*;

mod kerning;
mod native;
pub use native::{
    NATIVE_SDF_EM_PIXELS, NATIVE_SDF_MIN_PIXELS, compile_native_fallback_fonts,
    compile_native_outline_font, compile_native_outline_font_sizes,
};

/// Rasterizes an outline face for runtime labels while retaining its source line metrics.
pub fn compile_runtime_outline_font(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
) -> Result<RuntimeFontCatalog, FontCompileError> {
    validate_config(source_bytes, source_manifest_sha256, config)?;
    if config.advances != GlyphAdvances::Source {
        return Err(invalid("runtime outline faces require source advances"));
    }
    let source = Font::from_bytes(source_bytes, FontSettings::default()).map_err(|detail| {
        FontCompileError::OutlineFont {
            path: source_path.to_path_buf(),
            detail: detail.to_string().into_boxed_str(),
        }
    })?;
    let carrier = compile_parsed_outline(
        source_path,
        source_bytes,
        source_manifest_sha256,
        config,
        &source,
        true,
    )?;
    let line = source
        .horizontal_line_metrics(config.pixel_height as f32)
        .ok_or_else(|| invalid("outline face has no horizontal line metrics"))?;
    let fixed = |value: f32| -> Result<u32, FontCompileError> {
        if !value.is_finite() || value < 0.0 || value > MAX_FONT_PAGE_SIDE as f32 * 4.0 {
            return Err(invalid("outline face line metrics exceed bounds"));
        }
        Ok((value * 64.0).round() as u32)
    };
    let font = RuntimeFontCatalog::decode(&carrier.bytes, source_manifest_sha256)?;
    let pairs = kerning::pairs(source_bytes, font.glyphs(), config.pixel_height)?;
    font.with_kerning(pairs)?
        .with_line_metrics(FontLineMetrics {
            em_64: config.pixel_height * 64,
            ascent_64: fixed(line.ascent)?,
            descent_64: fixed(-line.descent)?,
        })
        .map(|font| font.with_linear_sampling())
        .map_err(FontCompileError::from)
}
