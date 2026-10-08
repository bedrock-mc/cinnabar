use std::collections::BTreeMap;

use assets::{FontPixels, FontRendering, FontTexturePage, RuntimeFontCatalog, encode_font_catalog};
use sha2::{Digest, Sha256};

use super::super::*;

mod atlas;
mod fallback;
mod freetype;
pub use fallback::compile_native_fallback_fonts;
mod sdf;
#[cfg(test)]
mod tests;

/// Em resolution shared by runtime SDF generation and native face installation.
pub const NATIVE_SDF_EM_PIXELS: u32 = 52;
/// Auto text switches to nearest coverage below this physical pixel height.
pub const NATIVE_SDF_MIN_PIXELS: u32 = 10;

struct Positioning<'a> {
    pairs: &'a BTreeMap<(char, char), i32>,
    units: u32,
}

/// Compiles runtime-only native text pages; the ordinary HUD carrier retains its profile.
pub fn compile_native_outline_font(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
    rendering: FontRendering,
) -> Result<RuntimeFontCatalog, FontCompileError> {
    compile(
        source_path,
        source_bytes,
        source_manifest_sha256,
        config,
        rendering,
        None,
    )
}

/// Reuses source positioning pairs across the family's raster sizes.
pub fn compile_native_outline_font_sizes(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
    rendering: FontRendering,
    raster_sizes: &[u32],
) -> Result<(RuntimeFontCatalog, BTreeMap<u32, RuntimeFontCatalog>), FontCompileError> {
    let face = ttf_parser::Face::parse(source_bytes, 0)
        .map_err(|_| invalid("outline positioning tables are invalid"))?;
    let units = u32::from(face.units_per_em());
    let base = compile_native_outline_font(
        source_path,
        source_bytes,
        source_manifest_sha256,
        config,
        rendering,
    )?;
    let pairs = super::kerning::pairs(source_bytes, base.glyphs(), units)?;
    let mut sizes = BTreeMap::new();
    for &pixel_height in raster_sizes {
        if sizes.contains_key(&pixel_height) || pixel_height == config.pixel_height {
            continue;
        }
        let font = compile(
            source_path,
            source_bytes,
            source_manifest_sha256,
            OutlineFontConfig {
                pixel_height,
                ..config
            },
            FontRendering::NativeCoverage,
            Some(Positioning {
                pairs: &pairs,
                units,
            }),
        )?;
        sizes.insert(pixel_height, font);
    }
    Ok((base, sizes))
}

fn compile(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
    rendering: FontRendering,
    positioning: Option<Positioning<'_>>,
) -> Result<RuntimeFontCatalog, FontCompileError> {
    validate_config_minimum(source_bytes, source_manifest_sha256, config, 1)?;
    if config.advances != GlyphAdvances::Source
        || rendering == FontRendering::Coverage
        || (rendering == FontRendering::NativeSdf && config.pixel_height != NATIVE_SDF_EM_PIXELS)
    {
        return Err(invalid(
            "native outlines require the source metric/rendering profile",
        ));
    }
    let mut face = freetype::Face::new(source_bytes, config.pixel_height)?;
    let line_metrics = face.line_metrics()?;
    let mut codepoints = REVIEWED_RANGES
        .iter()
        .flat_map(|(first, last)| *first..=*last)
        .filter_map(char::from_u32)
        .filter(|&ch| face.has(ch))
        .collect::<Vec<_>>();
    codepoints.push(config.replacement_codepoint);
    codepoints.sort_unstable();
    codepoints.dedup();
    let rasterized = codepoints
        .into_iter()
        .map(|ch| {
            let glyph = face.rasterize(ch)?;
            if rendering == FontRendering::NativeSdf {
                sdf::glyph(glyph)
            } else {
                Ok(glyph)
            }
        })
        .collect::<Result<Vec<_>, FontCompileError>>()?;
    let atlas = atlas::pack_pages(
        rasterized,
        config.atlas_side,
        rendering == FontRendering::NativeCoverage,
    )?;
    let source_length =
        u32::try_from(source_bytes.len()).map_err(|_| FontCompileError::SourceTooLarge {
            path: source_path.into(),
        })?;
    let source_hash = Sha256::digest(source_bytes).into();
    let pages: Vec<_> = atlas
        .pages
        .into_iter()
        .enumerate()
        .map(|(index, rgba8)| FontTexturePage {
            source_path: format!("font/native-{}px-{index}.png", config.pixel_height)
                .into_boxed_str(),
            source_bytes: source_length,
            source_sha256: source_hash,
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width: atlas.side,
            height: atlas.side,
            pixels: FontPixels::Rgba8(rgba8),
        })
        .collect();
    let bytes = encode_font_catalog(source_manifest_sha256, &atlas.glyphs, &pages)?;
    let font = RuntimeFontCatalog::decode(&bytes, source_manifest_sha256)?;
    let pairs = if let Some(Positioning { pairs, units }) = positioning {
        pairs
            .iter()
            .map(|(&key, &value)| {
                (
                    key,
                    (f64::from(value) * f64::from(config.pixel_height) / f64::from(units)).round()
                        as i32,
                )
            })
            .collect()
    } else {
        super::kerning::pairs(source_bytes, font.glyphs(), config.pixel_height)?
    };
    Ok(font
        .with_kerning(pairs)?
        .with_line_metrics(line_metrics)?
        .with_rendering(rendering)
        .with_coverage_pages())
}
