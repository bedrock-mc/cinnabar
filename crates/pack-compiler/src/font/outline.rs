use std::path::Path;

use assets::{
    FONT_CARRIER_SCHEMA, FontPixels, FontTexturePage, GlyphMetrics, MAX_FONT_PAGE_SIDE,
    MAX_FONT_SOURCE_BYTES, encode_font_catalog,
};
use fontdue::{Font, FontSettings};
use sha2::{Digest, Sha256};

use super::{CompiledFontCarrier, FontCompileError, FontCompileReport, invalid};

mod complete;
mod mathematical;
mod providers;
mod source;
pub use providers::compile_outline_font_with_fallback;
const ATLAS_PADDING: u32 = 1;
const FIXED_POINT_DENOMINATOR: i64 = 64;
const REQUIRED_REPLACEMENT: char = '\u{fffd}';

/// How a packed glyph's pen advance is derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphAdvances {
    /// Keep the outline font's own advance metrics. Correct for any font that
    /// already carries proportional widths.
    Source,
    /// Shift each inked glyph flush against its pen origin and advance by its
    /// inked width plus `gap_px`, the way Mojang's bitmap font is laid out.
    ///
    /// A blank glyph has no ink to measure, so `blank_advance_px` carries the
    /// space width explicitly; `None` keeps the source advance. Zero-advance blanks stay zero. That matters for
    /// a monospace source, where inheriting it leaves a space exactly as wide as
    /// the widest letter once every inked glyph has been tightened.
    ///
    /// The ink-derived advance never exceeds the advance the font gave a glyph.
    /// An explicit `blank_advance_px` is authoritative and is not clamped, since
    /// a monospace source may legitimately need a space wider than its ink.
    InkPlusGap {
        gap_px: u32,
        blank_advance_px: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutlineFontConfig {
    pub pixel_height: u32,
    pub atlas_side: u32,
    pub replacement_codepoint: char,
    pub advances: GlyphAdvances,
    /// Optional advance for ordinary and non-breaking spaces in raster 1/64 pixels.
    pub space_advance_64: Option<i16>,
    /// Optional left-bearing adjustment for printable ASCII and its derived variants.
    pub ascii_bearing: Option<assets::carriers::AsciiBearing>,
    /// Derives missing regular, bold and italic Latin mathematical glyphs from source artwork.
    pub synthesize_mathematical_letters: bool,
}

impl OutlineFontConfig {
    /// Uses larger shelves for the complete fallback map while respecting the requested minimum.
    pub const fn fallback_atlas_side(self) -> u32 {
        if self.atlas_side < 2048 {
            2048
        } else {
            self.atlas_side
        }
    }
}

impl Default for OutlineFontConfig {
    fn default() -> Self {
        Self {
            // Sans is the default; callers select other faces from their shared metadata.
            pixel_height: assets::FONT_RASTER_EM_PIXELS,
            atlas_side: 1_024,
            replacement_codepoint: REQUIRED_REPLACEMENT,
            advances: GlyphAdvances::Source,
            space_advance_64: None,
            ascii_bearing: None,
            synthesize_mathematical_letters: false,
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

/// Compiles every mapped Unicode scalar, retaining source metrics and bounded coverage pages.
pub fn compile_outline_font(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
) -> Result<CompiledFontCarrier, FontCompileError> {
    validate_config(source_bytes, source_manifest_sha256, config)?;
    let font = source::parse(source_path, source_bytes)?;
    complete::compile(
        &[(source_path, source_bytes, &font)],
        source_manifest_sha256,
        config,
    )
}

fn validate_config(
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
) -> Result<(), FontCompileError> {
    validate_config_minimum(source_bytes, source_manifest_sha256, config, 8)
}

fn validate_config_minimum(
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
    config: OutlineFontConfig,
    minimum_height: u32,
) -> Result<(), FontCompileError> {
    if source_bytes.is_empty() || source_bytes.len() as u64 > MAX_FONT_SOURCE_BYTES {
        return Err(FontCompileError::SourceTooLarge {
            path: "font/outline.ttf".into(),
        });
    }
    if let GlyphAdvances::InkPlusGap {
        gap_px,
        blank_advance_px,
    } = config.advances
        && (gap_px == 0
            || gap_px > config.pixel_height
            || blank_advance_px
                .is_some_and(|advance| advance == 0 || advance > config.pixel_height * 2))
    {
        return Err(invalid(
            "outline font proportional advance configuration is outside its reviewed bounds",
        ));
    }
    if source_manifest_sha256 == [0; 32]
        || config.replacement_codepoint != REQUIRED_REPLACEMENT
        || config.space_advance_64.is_some_and(|advance| advance <= 0)
        || !(minimum_height..=128).contains(&config.pixel_height)
        || config.atlas_side < 256
        || config.atlas_side > MAX_FONT_PAGE_SIDE
        || !config.atlas_side.is_power_of_two()
        || coverage_len(config.atlas_side)? as u64 > MAX_FONT_SOURCE_BYTES
    {
        return Err(invalid(
            "outline font configuration is outside its reviewed bounds",
        ));
    }
    Ok(())
}

/// Rasterizes one test glyph using the same checked coverage path as complete carriers.
#[cfg(test)]
fn rasterize(
    font: &Font,
    codepoint: char,
    pixel_height: u32,
    advances: GlyphAdvances,
) -> Result<RasterizedGlyph, FontCompileError> {
    rasterize_checked(font, codepoint, pixel_height, advances, None)
}

fn rasterize_checked(
    font: &Font,
    codepoint: char,
    pixel_height: u32,
    advances: GlyphAdvances,
    expected: Option<&fontdue::Metrics>,
) -> Result<RasterizedGlyph, FontCompileError> {
    let (metrics, bitmap) = font.rasterize(codepoint, pixel_height as f32);
    if let Some(expected) = expected
        && (metrics.width != expected.width
            || metrics.height != expected.height
            || metrics.xmin != expected.xmin
            || metrics.ymin != expected.ymin
            || metrics.advance_width.to_bits() != expected.advance_width.to_bits()
            || metrics.advance_height.to_bits() != expected.advance_height.to_bits()
            || metrics.bounds != expected.bounds
            || bitmap.len() != expected.width.saturating_mul(expected.height))
    {
        return Err(invalid("outline raster differs from its admitted metrics"));
    }
    // fontdue reports the outline's raster bounds, which can carry a fully
    // transparent row or column when an edge lands exactly on a texel
    // boundary. Trim to the inked extent so the atlas rect is tight and an
    // ink-derived advance measures ink rather than raster padding.
    let trimmed = trim_to_ink(&bitmap, metrics.width, metrics.height);
    let blank = trimmed.is_none();
    let (width, height, alpha, trim_x, trim_y) = match trimmed {
        None => (1, 1, vec![0].into_boxed_slice(), 0, 0),
        Some(ink) => (
            ink.width,
            ink.height,
            ink.alpha,
            i32::try_from(ink.x).map_err(|_| metric_error(codepoint, "bearing_x"))?,
            i32::try_from(ink.y).map_err(|_| metric_error(codepoint, "bearing_y"))?,
        ),
    };
    let bearing_y = metrics
        .ymin
        .checked_add(i32::try_from(metrics.height).map_err(|_| metric_error(codepoint, "height"))?)
        .and_then(i32::checked_neg)
        // Trimming transparent rows off the top moves the glyph down by the
        // same amount, in this y-down bearing convention.
        .and_then(|bearing| bearing.checked_add(trim_y))
        .ok_or_else(|| metric_error(codepoint, "bearing_y"))?;
    // A bitmap carrier addresses whole texels, so a pen advance is rounded to
    // one. A pixel font whose design grid does not divide its em exactly would
    // otherwise land a fraction of a texel out per glyph, and that drift
    // accumulates across a line until a glyph sits a whole pixel from where it
    // belongs. Exact pixel grids round to themselves.
    let source_advance_64 =
        f64::from(metrics.advance_width).round() * FIXED_POINT_DENOMINATOR as f64;
    if !source_advance_64.is_finite()
        || source_advance_64 < f64::from(i16::MIN)
        || source_advance_64 > f64::from(i16::MAX)
    {
        return Err(metric_error(codepoint, "advance"));
    }
    // A blank glyph has no ink to measure, so its source advance is the only
    // width available and stays authoritative in either mode.
    let (bearing_x, advance_64) = match advances {
        GlyphAdvances::InkPlusGap { gap_px, .. } if !blank => (
            0,
            i64::from(width)
                .checked_add(i64::from(gap_px))
                .and_then(|pixels| pixels.checked_mul(FIXED_POINT_DENOMINATOR))
                .and_then(|advance| i16::try_from(advance).ok())
                // Some fonts give accented, symbol, and arrow glyphs wider
                // cells, and let a few bleed past their own advance. This pass
                // only ever tightens: a glyph whose ink already fills its cell
                // keeps the advance the font gave it rather than gaining the
                // gap.
                .map(|advance| advance.min(source_advance_64 as i16))
                .ok_or_else(|| metric_error(codepoint, "advance"))?,
        ),
        GlyphAdvances::InkPlusGap {
            blank_advance_px: Some(blank_advance_px),
            ..
        } if source_advance_64 > 0.0 => (
            0,
            i64::from(blank_advance_px)
                .checked_mul(FIXED_POINT_DENOMINATOR)
                .and_then(|advance| i16::try_from(advance).ok())
                .ok_or_else(|| metric_error(codepoint, "advance"))?,
        ),
        GlyphAdvances::InkPlusGap { .. } | GlyphAdvances::Source => (
            metrics
                .xmin
                .checked_add(trim_x)
                .and_then(|bearing| i16::try_from(bearing).ok())
                .ok_or_else(|| metric_error(codepoint, "bearing_x"))?,
            source_advance_64 as i16,
        ),
    };
    Ok(RasterizedGlyph {
        codepoint,
        width,
        height,
        bearing: [
            bearing_x,
            i16::try_from(bearing_y).map_err(|_| metric_error(codepoint, "bearing_y"))?,
        ],
        advance_64,
        alpha,
    })
}

struct InkExtent {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    alpha: Box<[u8]>,
}

/// Returns the tightest sub-rectangle of `bitmap` containing a nonzero texel,
/// or `None` when the glyph has no ink at all.
fn trim_to_ink(bitmap: &[u8], width: usize, height: usize) -> Option<InkExtent> {
    if width == 0 || height == 0 || bitmap.len() < width.checked_mul(height)? {
        return None;
    }
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    for y in 0..height {
        for x in 0..width {
            if bitmap[y * width + x] != 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x == width {
        return None;
    }
    let ink_width = max_x - min_x + 1;
    let ink_height = max_y - min_y + 1;
    let mut alpha = Vec::with_capacity(ink_width * ink_height);
    for y in min_y..=max_y {
        alpha.extend_from_slice(&bitmap[y * width + min_x..y * width + max_x + 1]);
    }
    Some(InkExtent {
        x: u32::try_from(min_x).ok()?,
        y: u32::try_from(min_y).ok()?,
        width: u32::try_from(ink_width).ok()?,
        height: u32::try_from(ink_height).ok()?,
        alpha: alpha.into_boxed_slice(),
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

/// Packs raster coverage without retaining unused RGB channels.
fn pack(
    rasterized: &[RasterizedGlyph],
    side: u32,
) -> Result<(Vec<GlyphMetrics>, Box<[u8]>), FontCompileError> {
    let mut coverage = vec![0; coverage_len(side)?];
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
                coverage[target_pixel] = glyph.alpha[source];
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
    Ok((glyphs, coverage.into_boxed_slice()))
}

/// Returns the bounded single-channel page allocation.
fn coverage_len(side: u32) -> Result<usize, FontCompileError> {
    usize::try_from(side)
        .ok()
        .and_then(|side| side.checked_mul(side))
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
    mod coverage;
    use super::*;

    #[test]
    fn shipped_face_metrics_match_the_pinned_sources() {
        for carrier in assets::carriers::CARRIERS
            .iter()
            .filter(|carrier| carrier.font_face.is_some())
        {
            let profile = carrier.font_face.unwrap();
            let source: serde_json::Value = serde_json::from_slice(profile.manifest).unwrap();
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/fonts")
                .join(source["font_file"].as_str().unwrap());
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                source["font_sha256"].as_str().unwrap()
            );
            let font = Font::from_bytes(bytes, FontSettings::default()).unwrap();
            let line = font
                .horizontal_line_metrics(profile.raster_em_pixels() as f32)
                .unwrap();
            let metrics = profile.line_metrics();
            assert_eq!(metrics.ascent_64, (line.ascent * 64.0).round() as u32);
            assert_eq!(metrics.descent_64, (-line.descent * 64.0).round() as u32);
        }
    }

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
