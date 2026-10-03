use std::{collections::BTreeMap, fmt, mem::size_of, ops::Deref, sync::Arc};

use assets::CompiledFontCatalog;
use sha2::{Digest, Sha256};

use crate::UiScale;

mod invisible;
mod layout;
mod palette;
mod parse;

pub use palette::FormattingPalette;

use layout::build_layout;
pub use parse::parse_bedrock_text;

pub const MAX_TEXT_SPANS: usize = 4_096;
pub const MAX_GLYPHS_PER_LAYOUT: usize = 16_384;
pub const MAX_WRAP_LINES: usize = 1_024;

// The compiled Monocraft atlas is rasterized at 18 px/em (see
// `assets/ui-font-source.json`). Monocraft draws on a 60-font-unit grid against
// a 1080-unit em, so one design pixel is two texels: ASCII ink is 16 texels
// tall, 14 of them above the baseline, and the widest advance is 12. That makes
// `UiScale` 1 already equal to Mojang's GUI scale 2, and only whole numbers of
// physical pixels per texel keep every design pixel on a pixel boundary.
pub const FONT_DESIGN_PIXEL_TEXELS: u32 = 2;
pub const FONT_ASCENT_TEXELS: u32 = 14;
pub const FONT_INK_TEXELS: u32 = 16;
/// Mojang pitches chat one design pixel below the font's ink height -- 9 px for
/// an 8 px font. The same ratio against Monocraft's 16 texels gives 18.
pub const TEXT_LINE_HEIGHT_64: u32 = (FONT_INK_TEXELS + FONT_DESIGN_PIXEL_TEXELS) * 64;
/// Distance from the top of a line box down to the baseline, so glyphs sit
/// inside the box instead of hanging above its origin.
pub const TEXT_BASELINE_64: u32 = FONT_ASCENT_TEXELS * 64;
/// Mojang offsets the shadow by exactly one design pixel on both axes.
pub const TEXT_SHADOW_OFFSET_64: u32 = FONT_DESIGN_PIXEL_TEXELS * 64;

const FIXED_POINT_DENOMINATOR: i64 = 64;
const REPLACEMENT_CODEPOINT: char = '\u{fffd}';
// A std B-tree node currently holds several keys, values, and edge pointers.
// Charging a full 4 KiB node to every retained entry deliberately overcounts
// shared and partially occupied nodes, keeping the public byte cap conservative.
const CONSERVATIVE_BTREE_NODE_BYTES: usize = 4_096;
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

impl BedrockColor {
    /// The `§` colour's RGB; `None` when no colour is in force, which keeps the
    /// text's own colour.
    #[must_use]
    pub const fn rgb(self) -> Option<[u8; 3]> {
        Some(match self {
            Self::Base => return None,
            Self::White => [255, 255, 255],
            Self::Black => [0, 0, 0],
            Self::DarkBlue => [0, 0, 170],
            Self::DarkGreen => [0, 170, 0],
            Self::DarkAqua => [0, 170, 170],
            Self::DarkRed => [170, 0, 0],
            Self::DarkPurple => [170, 0, 170],
            Self::Gold => [255, 170, 0],
            Self::Gray => [170, 170, 170],
            Self::DarkGray => [85, 85, 85],
            Self::Blue => [85, 85, 255],
            Self::Green => [85, 255, 85],
            Self::Aqua => [85, 255, 255],
            Self::Red => [255, 85, 85],
            Self::LightPurple => [255, 85, 255],
            Self::Yellow => [255, 255, 85],
            Self::MinecoinGold => [221, 214, 5],
            Self::MaterialQuartz => [227, 212, 209],
            Self::MaterialIron => [206, 202, 202],
            Self::MaterialNetherite => [68, 58, 59],
            Self::MaterialRedstone => [151, 22, 7],
            Self::MaterialCopper => [180, 104, 77],
            Self::MaterialGold => [222, 177, 45],
            Self::MaterialEmerald => [17, 160, 54],
            Self::MaterialDiamond => [44, 186, 168],
            Self::MaterialLapis => [35, 98, 180],
            Self::MaterialAmethyst => [154, 92, 198],
            Self::MaterialResin => [237, 105, 52],
            Self::PartyBlue => [140, 179, 255],
        })
    }
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
    /// Before the first glyph that overflows; a glyph wider than the line errors.
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
    /// Extra pitch between lines in output 1/64 pixels (not scaled again).
    pub line_padding_64: i32,
    pub chop: WordChop,
    /// Lines past this drop and the last kept one ends in `...`.
    pub max_lines: Option<u16>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayout {
    id: u64,
    key: TextLayoutKey,
    glyphs: Box<[GlyphQuad]>,
    line_count: u16,
    size_64: [u32; 2],
    ellipsized: bool,
}

impl TextLayout {
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

struct CacheEntry {
    layout: Arc<TextLayout>,
    retained_bytes: usize,
    last_used: u64,
}

pub struct TextLayoutCache {
    entry_cap: usize,
    byte_cap: usize,
    retained_bytes: usize,
    next_id: u64,
    clock: u64,
    entries: BTreeMap<TextLayoutKey, CacheEntry>,
}

impl TextLayoutCache {
    pub fn new(entry_cap: usize, byte_cap: usize) -> Self {
        Self {
            entry_cap,
            byte_cap,
            retained_bytes: 0,
            next_id: 1,
            clock: 0,
            entries: BTreeMap::new(),
        }
    }

    pub fn layout(&mut self, request: TextLayoutRequest<'_>) -> Result<Arc<TextLayout>, TextError> {
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
        if request.text.len() > crate::UiLimits::MAX_TEXT_BYTES {
            return Err(TextError::TextBytesExceeded {
                actual: request.text.len(),
                limit: crate::UiLimits::MAX_TEXT_BYTES,
            });
        }
        let key = layout_key(request);
        let now = self.advance_clock()?;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = now;
            return Ok(Arc::clone(&entry.layout));
        }

        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(TextError::CacheCounterOverflow)?;
        let layout = Arc::new(build_layout(id, key.clone(), request)?);
        let retained_bytes = retained_layout_bytes(&layout)?;
        if self.entry_cap == 0 || retained_bytes > self.byte_cap {
            return Ok(layout);
        }

        while self.entries.len() >= self.entry_cap
            || self
                .retained_bytes
                .checked_add(retained_bytes)
                .is_none_or(|bytes| bytes > self.byte_cap)
        {
            if !self.evict_lru() {
                return Ok(layout);
            }
        }
        let new_retained_bytes = self
            .retained_bytes
            .checked_add(retained_bytes)
            .ok_or(TextError::FixedPointOverflow)?;
        self.entries.insert(
            key,
            CacheEntry {
                layout: Arc::clone(&layout),
                retained_bytes,
                last_used: now,
            },
        );
        self.retained_bytes = new_retained_bytes;
        Ok(layout)
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn advance_clock(&mut self) -> Result<u64, TextError> {
        self.clock = self
            .clock
            .checked_add(1)
            .ok_or(TextError::CacheCounterOverflow)?;
        Ok(self.clock)
    }

    fn evict_lru(&mut self) -> bool {
        let Some(key) = self
            .entries
            .iter()
            .min_by(|(left_key, left), (right_key, right)| {
                (left.last_used, left.layout.id(), *left_key).cmp(&(
                    right.last_used,
                    right.layout.id(),
                    *right_key,
                ))
            })
            .map(|(key, _)| key.clone())
        else {
            return false;
        };
        if let Some(removed) = self.entries.remove(&key) {
            self.retained_bytes -= removed.retained_bytes;
        }
        true
    }
}

fn layout_key(request: TextLayoutRequest<'_>) -> TextLayoutKey {
    TextLayoutKey {
        content_sha256: Sha256::digest(request.text.as_bytes()).into(),
        style: request.style,
        width_64: request.width_64,
        line_height_64: request.line_height_64,
        baseline_64: request.baseline_64,
        scale_1024: (request.scale.get() * UiScale::SCALE_DENOMINATOR as f32).round() as u16,
        font_identity: request.font.identity().carrier_sha256,
        wrap: request.wrap,
    }
}

fn retained_layout_bytes(layout: &TextLayout) -> Result<usize, TextError> {
    let glyph_bytes = layout
        .glyphs
        .len()
        .checked_mul(size_of::<GlyphQuad>())
        .ok_or(TextError::FixedPointOverflow)?;
    let arc_allocation = conservative_allocation_bytes(
        size_of::<TextLayout>()
            .checked_add(2 * size_of::<usize>())
            .ok_or(TextError::FixedPointOverflow)?,
    )?;
    let glyph_allocation = conservative_allocation_bytes(glyph_bytes)?;
    [
        arc_allocation,
        glyph_allocation,
        // BTreeMap duplicates the key and retains a CacheEntry value.
        size_of::<TextLayoutKey>(),
        size_of::<CacheEntry>(),
        CONSERVATIVE_BTREE_NODE_BYTES,
        // Node allocator metadata is charged separately from its full page.
        ALLOCATOR_METADATA_BYTES,
    ]
    .into_iter()
    .try_fold(0usize, |total, bytes| total.checked_add(bytes))
    .ok_or(TextError::FixedPointOverflow)
}

fn conservative_allocation_bytes(payload_bytes: usize) -> Result<usize, TextError> {
    payload_bytes
        .checked_add(ALLOCATOR_METADATA_BYTES)
        .and_then(|bytes| bytes.checked_add(CONSERVATIVE_ALLOCATION_GRANULARITY - 1))
        .map(|bytes| bytes / CONSERVATIVE_ALLOCATION_GRANULARITY)
        .and_then(|pages| pages.checked_mul(CONSERVATIVE_ALLOCATION_GRANULARITY))
        .ok_or(TextError::FixedPointOverflow)
}
