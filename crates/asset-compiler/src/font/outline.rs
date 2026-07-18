use std::path::Path;

use assets::{
    FONT_CARRIER_SCHEMA, FontTexturePage, GlyphMetrics, MAX_FONT_PAGE_SIDE, MAX_FONT_SOURCE_BYTES,
    encode_font_catalog,
};
use fontdue::{Font, FontSettings};
use sha2::{Digest, Sha256};

use super::{CompiledFontCarrier, FontCompileError, FontCompileReport, invalid};

const ATLAS_PADDING: u32 = 1;
const REQUIRED_REPLACEMENT: char = '\u{fffd}';
const REVIEWED_RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007e),
    (0x00a0, 0x024f),
    (0x0370, 0x052f),
    (0x2000, 0x206f),
    (0x20a0, 0x214f),
    (0x2190, 0x21ff),
    (0x2500, 0x25ff),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutlineFontConfig {
    pub pixel_height: u32,
    pub atlas_side: u32,
    pub replacement_codepoint: char,
}

impl Default for OutlineFontConfig {
    fn default() -> Self {
        Self {
            pixel_height: 32,
            atlas_side: 2_048,
            replacement_codepoint: REQUIRED_REPLACEMENT,
        }
    }
}

struct RasterizedGlyph {
    codepoint: char,
    width: u32,
    height: u32,
    bearing: [i16; 2],
    advance_64: i16,
    alpha: Box<[u8]>,
}

pub fn compile_outline_font(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
) -> Result<CompiledFontCarrier, FontCompileError> {
    validate_config(source_bytes, source_manifest_sha256, config)?;
    let font = Font::from_bytes(source_bytes, FontSettings::default()).map_err(|detail| {
        FontCompileError::OutlineFont {
            path: source_path.to_path_buf(),
            detail: detail.to_string().into_boxed_str(),
        }
    })?;
    let mut codepoints = REVIEWED_RANGES
        .iter()
        .flat_map(|(first, last)| *first..=*last)
        .filter_map(char::from_u32)
        .filter(|codepoint| font.lookup_glyph_index(*codepoint) != 0)
        .collect::<Vec<_>>();
    codepoints.push(config.replacement_codepoint);
    codepoints.sort_unstable();
    codepoints.dedup();

    let rasterized = codepoints
        .into_iter()
        .map(|codepoint| {
            if codepoint == config.replacement_codepoint && font.lookup_glyph_index(codepoint) == 0
            {
                synthetic_replacement(config.pixel_height)
            } else {
                rasterize(&font, codepoint, config.pixel_height)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (glyphs, rgba8) = pack(&rasterized, config.atlas_side)?;
    let source_sha256 = Sha256::digest(source_bytes).into();
    let pixels_sha256 = Sha256::digest(&rgba8).into();
    let page = FontTexturePage {
        source_path: format!("font/inter-{}px.png", config.pixel_height).into_boxed_str(),
        source_bytes: u32::try_from(source_bytes.len()).map_err(|_| {
            FontCompileError::SourceTooLarge {
                path: source_path.to_path_buf(),
            }
        })?,
        source_sha256,
        pixels_sha256,
        width: config.atlas_side,
        height: config.atlas_side,
        rgba8,
    };
    let pages = [page];
    let bytes = encode_font_catalog(source_manifest_sha256, &glyphs, &pages)?;
    let carrier_sha256 = bytes
        .get(bytes.len().saturating_sub(32)..)
        .and_then(|digest| digest.try_into().ok())
        .ok_or_else(|| invalid("encoded font carrier lacks its SHA-256"))?;
    Ok(CompiledFontCarrier {
        report: FontCompileReport {
            schema: FONT_CARRIER_SCHEMA,
            glyphs: glyphs.len(),
            pages: 1,
            source_bytes: source_bytes.len() as u64,
            decoded_bytes: rgba_len(config.atlas_side)? as u64,
            source_manifest_sha256,
            carrier_sha256,
        },
        bytes,
    })
}

fn validate_config(
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
) -> Result<(), FontCompileError> {
    if source_bytes.is_empty() || source_bytes.len() as u64 > MAX_FONT_SOURCE_BYTES {
        return Err(FontCompileError::SourceTooLarge {
            path: "font/Inter.ttf".into(),
        });
    }
    if source_manifest_sha256 == [0; 32]
        || config.replacement_codepoint != REQUIRED_REPLACEMENT
        || !(8..=128).contains(&config.pixel_height)
        || config.atlas_side < 256
        || config.atlas_side > MAX_FONT_PAGE_SIDE
        || !config.atlas_side.is_power_of_two()
        || rgba_len(config.atlas_side)? as u64 > MAX_FONT_SOURCE_BYTES
    {
        return Err(invalid(
            "outline font configuration is outside its reviewed bounds",
        ));
    }
    Ok(())
}

fn rasterize(
    font: &Font,
    codepoint: char,
    pixel_height: u32,
) -> Result<RasterizedGlyph, FontCompileError> {
    let (metrics, bitmap) = font.rasterize(codepoint, pixel_height as f32);
    let (width, height, alpha) = if metrics.width == 0 || metrics.height == 0 {
        (1, 1, vec![0].into_boxed_slice())
    } else {
        (
            u32::try_from(metrics.width).map_err(|_| metric_error(codepoint, "width"))?,
            u32::try_from(metrics.height).map_err(|_| metric_error(codepoint, "height"))?,
            bitmap.into_boxed_slice(),
        )
    };
    let bearing_y = metrics
        .ymin
        .checked_add(i32::try_from(metrics.height).map_err(|_| metric_error(codepoint, "height"))?)
        .and_then(i32::checked_neg)
        .ok_or_else(|| metric_error(codepoint, "bearing_y"))?;
    let advance_64 = (f64::from(metrics.advance_width) * 64.0).round();
    if !advance_64.is_finite()
        || advance_64 < f64::from(i16::MIN)
        || advance_64 > f64::from(i16::MAX)
    {
        return Err(metric_error(codepoint, "advance"));
    }
    Ok(RasterizedGlyph {
        codepoint,
        width,
        height,
        bearing: [
            i16::try_from(metrics.xmin).map_err(|_| metric_error(codepoint, "bearing_x"))?,
            i16::try_from(bearing_y).map_err(|_| metric_error(codepoint, "bearing_y"))?,
        ],
        advance_64: advance_64 as i16,
        alpha,
    })
}

fn synthetic_replacement(pixel_height: u32) -> Result<RasterizedGlyph, FontCompileError> {
    let width = pixel_height
        .checked_mul(5)
        .and_then(|value| value.checked_div(8))
        .filter(|value| *value >= 4)
        .ok_or_else(|| invalid("replacement glyph width is invalid"))?;
    let height = pixel_height
        .checked_mul(7)
        .and_then(|value| value.checked_div(8))
        .filter(|value| *value >= 4)
        .ok_or_else(|| invalid("replacement glyph height is invalid"))?;
    let border = (pixel_height / 16).max(1);
    let mut alpha = vec![
        0;
        usize::try_from(width * height)
            .map_err(|_| { invalid("replacement glyph allocation overflows") })?
    ];
    for y in 0..height {
        for x in 0..width {
            if x < border || y < border || x >= width - border || y >= height - border {
                alpha[usize::try_from(y * width + x)
                    .map_err(|_| invalid("replacement glyph offset overflows"))?] = 255;
            }
        }
    }
    Ok(RasterizedGlyph {
        codepoint: REQUIRED_REPLACEMENT,
        width,
        height,
        bearing: [
            0,
            i16::try_from(height)
                .map_err(|_| metric_error(REQUIRED_REPLACEMENT, "bearing_y"))?
                .saturating_neg(),
        ],
        advance_64: i16::try_from((width + border * 2) * 64)
            .map_err(|_| metric_error(REQUIRED_REPLACEMENT, "advance"))?,
        alpha: alpha.into_boxed_slice(),
    })
}

fn pack(
    rasterized: &[RasterizedGlyph],
    side: u32,
) -> Result<(Vec<GlyphMetrics>, Box<[u8]>), FontCompileError> {
    let mut rgba8 = vec![0; rgba_len(side)?];
    let mut glyphs = Vec::with_capacity(rasterized.len());
    let mut x = ATLAS_PADDING;
    let mut y = ATLAS_PADDING;
    let mut row_height = 0;
    for glyph in rasterized {
        if glyph.width + ATLAS_PADDING * 2 > side || glyph.height + ATLAS_PADDING * 2 > side {
            return Err(FontCompileError::OutlineAtlasFull { side });
        }
        if x + glyph.width + ATLAS_PADDING > side {
            x = ATLAS_PADDING;
            y = y
                .checked_add(row_height + ATLAS_PADDING)
                .ok_or(FontCompileError::OutlineAtlasFull { side })?;
            row_height = 0;
        }
        if y + glyph.height + ATLAS_PADDING > side {
            return Err(FontCompileError::OutlineAtlasFull { side });
        }
        for source_y in 0..glyph.height {
            for source_x in 0..glyph.width {
                let source = usize::try_from(source_y * glyph.width + source_x)
                    .map_err(|_| FontCompileError::OutlineAtlasFull { side })?;
                let target_pixel = usize::try_from((y + source_y) * side + x + source_x)
                    .map_err(|_| FontCompileError::OutlineAtlasFull { side })?;
                let target = target_pixel
                    .checked_mul(4)
                    .ok_or(FontCompileError::OutlineAtlasFull { side })?;
                rgba8[target] = 255;
                rgba8[target + 1] = 255;
                rgba8[target + 2] = 255;
                rgba8[target + 3] = glyph.alpha[source];
            }
        }
        glyphs.push(GlyphMetrics {
            codepoint: glyph.codepoint,
            page: 0,
            uv: [
                u16::try_from(x).map_err(|_| metric_error(glyph.codepoint, "uv"))?,
                u16::try_from(y).map_err(|_| metric_error(glyph.codepoint, "uv"))?,
                u16::try_from(x + glyph.width).map_err(|_| metric_error(glyph.codepoint, "uv"))?,
                u16::try_from(y + glyph.height).map_err(|_| metric_error(glyph.codepoint, "uv"))?,
            ],
            bearing: glyph.bearing,
            advance_64: glyph.advance_64,
        });
        x += glyph.width + ATLAS_PADDING;
        row_height = row_height.max(glyph.height);
    }
    Ok((glyphs, rgba8.into_boxed_slice()))
}

fn rgba_len(side: u32) -> Result<usize, FontCompileError> {
    usize::try_from(side)
        .ok()
        .and_then(|side| side.checked_mul(side))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("outline font atlas size overflows"))
}

fn metric_error(codepoint: char, field: &'static str) -> FontCompileError {
    FontCompileError::MetricOutOfRange {
        codepoint: codepoint as u32,
        field,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_replacement_is_visible_bounded_tofu() {
        let glyph = synthetic_replacement(32).unwrap();
        assert_eq!(glyph.codepoint, '\u{fffd}');
        assert_eq!([glyph.width, glyph.height], [20, 28]);
        assert_eq!(glyph.alpha.len(), 20 * 28);
        assert!(glyph.alpha.contains(&255));
        assert!(glyph.alpha.contains(&0));
        assert!(glyph.advance_64 > 0);
    }
}
