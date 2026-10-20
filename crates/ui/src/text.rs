use std::{collections::BTreeMap, fmt, mem::size_of, ops::Deref, sync::Arc};

use assets::{CompiledFontCatalog, FontRendering};
use sha2::{Digest, Sha256};

use crate::UiScale;

mod cache;
pub use cache::TextLayoutCache;
mod colors;
#[cfg(test)]
mod colors_tests;
mod invisible;
mod layout;
mod line_metrics;
mod palette;
mod parse;
mod single_line;
mod units;

pub use colors::{ColorDescriptor, FORMATTING_COLORS};
pub use palette::FormattingPalette;

use layout::build_layout;
pub use parse::parse_bedrock_text;

pub const MAX_TEXT_SPANS: usize = 4_096;
pub const MAX_GLYPHS_PER_LAYOUT: usize = 16_384;
pub const MAX_WRAP_LINES: usize = 1_024;

// The compiled Cinnangles Sans atlas is rasterized at 18 px/em (see
// `assets/cinnangles-sans-source.json`). Its reviewed pixel grid uses two texels
// per design pixel: ASCII ink is 16 texels
// tall, 14 of them above the baseline, and the widest advance is 12. That makes
// `UiScale` 1 already equal to Mojang's GUI scale 2, and only whole numbers of
// physical pixels per texel keep every design pixel on a pixel boundary.
pub const FONT_DESIGN_PIXEL_TEXELS: u32 = 2;
pub const FONT_ASCENT_TEXELS: u32 = 14;
pub const FONT_INK_TEXELS: u32 = 16;
/// Mojang pitches chat one design pixel below the font's ink height -- 9 px for
/// an 8 px font. The same ratio against Cinnangles Sans's 16 texels gives 18.
pub const TEXT_LINE_HEIGHT_64: u32 = (FONT_INK_TEXELS + FONT_DESIGN_PIXEL_TEXELS) * 64;
/// Distance from the top of a line box down to the baseline, so glyphs sit
/// inside the box instead of hanging above its origin.
pub const TEXT_BASELINE_64: u32 = FONT_ASCENT_TEXELS * 64;
/// Mojang offsets the shadow by exactly one design pixel on both axes.
pub const TEXT_SHADOW_OFFSET_64: u32 = FONT_DESIGN_PIXEL_TEXELS * 64;
/// One design pixel for bold measurement and the compiled open-font duplicate.
pub const TEXT_BOLD_OFFSET_64: u32 = FONT_DESIGN_PIXEL_TEXELS * 64;

const FIXED_POINT_DENOMINATOR: i64 = 64;
const REPLACEMENT_CODEPOINT: char = '\u{fffd}';
// A std B-tree node currently holds several keys, values, and edge pointers.
// Charging a full 8 KiB node to every retained entry deliberately overcounts
// shared and partially occupied nodes, keeping the public byte cap conservative.
const CONSERVATIVE_BTREE_NODE_BYTES: usize = 8_192;
const ALLOCATOR_METADATA_BYTES: usize = 2 * size_of::<usize>();
const CONSERVATIVE_ALLOCATION_GRANULARITY: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum BedrockColor {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    /// No `§` colour in force: the draw's own colour.
    #[default]
    Base,
    White,
    MinecoinGold,
    MaterialQuartz,
    MaterialIron,
    MaterialNetherite,
    MaterialRedstone,
    MaterialCopper,
    MaterialGold,
    MaterialEmerald,
    MaterialDiamond,
    MaterialLapis,
    MaterialAmethyst,
    MaterialResin,
    PartyBlue,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct TextStyle {
    pub color: BedrockColor,
    pub obfuscated: bool,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextSpan {
    pub text: Box<str>,
    pub style: TextStyle,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextSpans(Vec<TextSpan>);

impl TextSpans {
    pub fn plain_text(&self) -> String {
        let bytes = self.0.iter().map(|span| span.text.len()).sum();
        let mut plain = String::with_capacity(bytes);
        for span in &self.0 {
            plain.push_str(&span.text);
        }
        plain
    }
}

impl Deref for TextSpans {
    type Target = [TextSpan];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TextLayoutKey {
    pub content_sha256: [u8; 32],
    pub style: TextStyle,
    pub width_64: u32,
    pub line_height_64: u32,
    pub baseline_64: u32,
    pub scale_1024: u16,
    pub font_identity: [u8; 32],
    pub wrap: TextWrap,
}

impl TextLayoutKey {
    /// Returns the display scale for bitmap-unit bold and italic effects.
    pub fn effect_scale(&self) -> f32 {
        let fixed = if self.wrap.effect_scale_1024 == 0 {
            u32::from(self.scale_1024)
        } else {
            self.wrap.effect_scale_1024
        };
        fixed as f32 / UiScale::SCALE_DENOMINATOR as f32
    }
}

/// Where each wrapped line sits within the wrap width.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum TextLineAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// How a word wider than the whole line breaks.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum WordChop {
    /// Before the first glyph that overflows; strict requests reject a wider glyph.
    #[default]
    Glyph,
    /// Vanilla labels: so the prefix plus `-` fits, then draw the `-`.
    Hyphen,
    /// As [`Self::Hyphen`] without drawing the hyphen (`hide_hyphen`).
    Bare,
}

/// A label's wrapping options beyond its width.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct TextWrap {
    pub align: TextLineAlign,
    /// Additional pen advance after each character, in output 1/64 pixels.
    pub letter_spacing_64: i32,
    /// Scale of bitmap-unit bold and italic effects in 1/1024 units; zero uses glyph scale.
    /// Native faces can normalize their raster independently of these display effects.
    pub effect_scale_1024: u32,
    /// Extra pitch between lines in output 1/64 pixels (not scaled again).
    pub line_padding_64: i32,
    pub chop: WordChop,
    /// Keeps a glyph wider than the line so the control can clip its ink.
    pub allow_visual_overflow: bool,
    /// Lines past this drop and the last kept one ends in `...`.
    pub max_lines: Option<u16>,
    /// The grid, in 1/65536 output pixels, that alignment offsets truncate onto, as vanilla
    /// snaps each line to the pixel grid; zero keeps them exact.
    pub align_grid_65536: u32,
    /// Device pixels per output pixel in 1/65536 units; zero means unknown.
    /// Half-device-pixel font texels use exact device units to prevent fractional-scale stroke drift.
    pub device_scale_65536: u32,
    /// Rounds final glyph vertices to device pixels without changing line or pen metrics.
    pub snap_glyphs_to_device_pixels: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphQuad {
    pub codepoint: char,
    pub resolved_codepoint: char,
    pub page: u16,
    pub uv: [u16; 4],
    pub bounds_64: [i32; 4],
    pub line: u16,
    pub style: TextStyle,
    pub linear_sampling: bool,
    pub rendering: FontRendering,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayout {
    id: u64,
    key: TextLayoutKey,
    glyphs: Box<[GlyphQuad]>,
    /// Some glyph is `§k` text, whose glyphs change every frame.
    obfuscated: bool,
    source_indices: Box<[Option<usize>]>,
    caret_stops_64: Box<[(usize, i32)]>,
    line_count: u16,
    size_64: [u32; 2],
    ellipsized: bool,
    linear_sampling: bool,
    rendering: FontRendering,
}

impl TextLayout {
    pub const fn rendering(&self) -> FontRendering {
        self.rendering
    }

    pub const fn linear_sampling(&self) -> bool {
        self.linear_sampling
    }

    /// Whether a line limit cut the text short and ended it in `...`.
    pub const fn ellipsized(&self) -> bool {
        self.ellipsized
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn key(&self) -> &TextLayoutKey {
        &self.key
    }

    pub fn glyphs(&self) -> &[GlyphQuad] {
        &self.glyphs
    }

    /// Whether any glyph is `§k` text, whose glyphs change every frame.
    pub const fn obfuscated(&self) -> bool {
        self.obfuscated
    }

    /// Character indices in formatting-stripped text; generated hyphens and ellipses have none.
    pub fn glyph_source_indices(&self) -> &[Option<usize>] {
        &self.source_indices
    }

    /// The first line's pen at a source byte boundary, in output 1/64 pixels.
    /// Hidden formatting and invisible characters retain the preceding stop.
    pub fn first_line_caret_x_64(&self, byte: usize) -> i32 {
        let end = self.caret_stops_64.partition_point(|(at, _)| *at <= byte);
        self.caret_stops_64[end.saturating_sub(1)].1
    }

    pub const fn line_count(&self) -> u16 {
        self.line_count
    }

    pub const fn size_64(&self) -> [u32; 2] {
        self.size_64
    }
}

/// Same-width glyph pools for the `§k` (obfuscation) draw-time glyph swap.
///
/// Keyed by native raster width so a replacement fills a run's existing cell
/// without distortion. The layout cache is content-hashed and cannot hold the
/// per-frame scramble, so the draw list selects a replacement each frame while
/// the pen advance stays fixed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObfuscationGlyphs {
    pools: BTreeMap<u16, Box<[ObfuscationCandidate]>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObfuscationCandidate {
    page: u16,
    uv: [u16; 4],
}

impl ObfuscationGlyphs {
    pub fn from_catalog(font: &CompiledFontCatalog) -> Self {
        let mut grouped: BTreeMap<u16, Vec<ObfuscationCandidate>> = BTreeMap::new();
        for glyph in font.glyphs() {
            let width = glyph.uv[2].saturating_sub(glyph.uv[0]);
            let height = glyph.uv[3].saturating_sub(glyph.uv[1]);
            // A zero-area raster (space, control) would blank the scrambled cell.
            if width == 0 || height == 0 {
                continue;
            }
            grouped
                .entry(width)
                .or_default()
                .push(ObfuscationCandidate {
                    page: glyph.page,
                    uv: glyph.uv,
                });
        }
        Self {
            pools: grouped
                .into_iter()
                .map(|(width, candidates)| (width, candidates.into_boxed_slice()))
                .collect(),
        }
    }

    /// A same-`width` replacement `(page, uv)` chosen by `selector`, or `None`
    /// when no visible glyph shares that width.
    pub fn pick(&self, width: u16, selector: u64) -> Option<(u16, [u16; 4])> {
        let pool = self.pools.get(&width)?;
        let candidate = pool.get((selector % pool.len() as u64) as usize)?;
        Some((candidate.page, candidate.uv))
    }
}

#[derive(Clone, Copy)]
pub struct TextLayoutRequest<'a> {
    pub text: &'a str,
    pub style: TextStyle,
    pub width_64: u32,
    /// Baseline-to-baseline pitch in unscaled 1/64 pixels, multiplied by
    /// `scale` during layout.
    ///
    /// This is a property of the text block, not of the font atlas: Mojang's
    /// client hardcodes a 9-pixel chat pitch against an 8-pixel font rather
    /// than deriving one from glyph metrics. Deriving it from the atlas made
    /// the tallest glyph anywhere in the catalog inflate every line's pitch.
    pub line_height_64: u32,
    /// Distance from the top of a line box down to the baseline, in unscaled
    /// 1/64 pixels.
    ///
    /// Glyph bearings are measured from the baseline and point upwards, so
    /// without this the baseline sits on the line box's own origin: every glyph
    /// hangs above the box while `line_height_64` extends below it, and a
    /// single-line layout reports nearly twice the height it occupies. Callers
    /// stacking one layout per row then space those rows by that inflated
    /// height.
    pub baseline_64: u32,
    pub scale: UiScale,
    pub font: &'a CompiledFontCatalog,
    pub wrap: TextWrap,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TextError {
    TextBytesExceeded {
        actual: usize,
        limit: usize,
    },
    SpanLimitExceeded {
        actual: usize,
        limit: usize,
    },
    GlyphLimitExceeded {
        actual: usize,
        limit: usize,
    },
    WrapLineLimitExceeded {
        actual: usize,
        limit: usize,
    },
    VisualWidthExceeded {
        actual_64: u64,
        limit_64: u64,
    },
    ZeroWrapWidth,
    ZeroLineHeight,
    BaselineOutsideLine {
        baseline_64: u32,
        line_height_64: u32,
    },
    MissingReplacementGlyph,
    FixedPointOverflow,
    CacheCounterOverflow,
}

impl fmt::Display for TextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextBytesExceeded { actual, limit } => {
                write!(
                    formatter,
                    "text has {actual} bytes, exceeding limit {limit}"
                )
            }
            Self::SpanLimitExceeded { actual, limit } => {
                write!(
                    formatter,
                    "text has {actual} spans, exceeding limit {limit}"
                )
            }
            Self::GlyphLimitExceeded { actual, limit } => {
                write!(
                    formatter,
                    "layout has {actual} glyphs, exceeding limit {limit}"
                )
            }
            Self::WrapLineLimitExceeded { actual, limit } => {
                write!(
                    formatter,
                    "layout has {actual} lines, exceeding limit {limit}"
                )
            }
            Self::VisualWidthExceeded {
                actual_64,
                limit_64,
            } => write!(
                formatter,
                "glyph visual width {actual_64}/64 exceeds wrap width {limit_64}/64"
            ),
            Self::ZeroWrapWidth => formatter.write_str("text wrap width must be nonzero"),
            Self::ZeroLineHeight => formatter.write_str("text line height must be nonzero"),
            Self::BaselineOutsideLine {
                baseline_64,
                line_height_64,
            } => write!(
                formatter,
                "text baseline {baseline_64}/64 falls outside line height {line_height_64}/64"
            ),
            Self::MissingReplacementGlyph => {
                formatter.write_str("font has no replacement glyph for a missing codepoint")
            }
            Self::FixedPointOverflow => formatter.write_str("text fixed-point layout overflowed"),
            Self::CacheCounterOverflow => formatter.write_str("text cache counter overflowed"),
        }
    }
}

impl std::error::Error for TextError {}

/// Borrows the longest complete UTF-8 prefix within the supplied byte allowance.
fn text_prefix(text: &str, max_bytes: usize) -> &str {
    &text[..text.floor_char_boundary(max_bytes.min(text.len()))]
}

/// Rejects invalid geometry before lookup or single-line normalization allocates.
fn validate_request(request: TextLayoutRequest<'_>) -> Result<(), TextError> {
    if request.width_64 == 0 {
        return Err(TextError::ZeroWrapWidth);
    }
    if request.line_height_64 == 0 {
        return Err(TextError::ZeroLineHeight);
    }
    if request.baseline_64 > request.line_height_64 {
        return Err(TextError::BaselineOutsideLine {
            baseline_64: request.baseline_64,
            line_height_64: request.line_height_64,
        });
    }
    Ok(())
}

/// Qualifies a content digest with every layout-affecting request parameter.
fn layout_key(request: TextLayoutRequest<'_>, content_sha256: [u8; 32]) -> TextLayoutKey {
    TextLayoutKey {
        content_sha256,
        style: request.style,
        width_64: request.width_64,
        line_height_64: request.line_height_64,
        baseline_64: request.baseline_64,
        scale_1024: request.scale.fixed_1024(),
        font_identity: request.font.identity().carrier_sha256,
        wrap: request.wrap,
    }
}

/// Rounds retained allocations up and includes allocator metadata.
fn conservative_allocation_bytes(payload_bytes: usize) -> Result<usize, TextError> {
    payload_bytes
        .checked_add(ALLOCATOR_METADATA_BYTES)
        .and_then(|bytes| bytes.checked_add(CONSERVATIVE_ALLOCATION_GRANULARITY - 1))
        .map(|bytes| bytes / CONSERVATIVE_ALLOCATION_GRANULARITY)
        .and_then(|pages| pages.checked_mul(CONSERVATIVE_ALLOCATION_GRANULARITY))
        .ok_or(TextError::FixedPointOverflow)
}
