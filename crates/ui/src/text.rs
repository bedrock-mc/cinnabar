use std::{collections::BTreeMap, fmt, mem::size_of, ops::Deref, sync::Arc};

use assets::{CompiledFontCatalog, GlyphMetrics};
use sha2::{Digest, Sha256};

use crate::UiScale;

pub const MAX_TEXT_SPANS: usize = 4_096;
pub const MAX_GLYPHS_PER_LAYOUT: usize = 16_384;
pub const MAX_WRAP_LINES: usize = 1_024;

const FIXED_POINT_DENOMINATOR: i64 = 64;
const SCALE_DENOMINATOR: i64 = 1_024;
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
    #[default]
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
    pub scale_1024: u16,
    pub font_identity: [u8; 32],
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
}

impl TextLayout {
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

#[derive(Clone, Copy)]
pub struct TextLayoutRequest<'a> {
    pub text: &'a str,
    pub style: TextStyle,
    pub width_64: u32,
    pub scale: UiScale,
    pub font: &'a CompiledFontCatalog,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TextError {
    TextBytesExceeded { actual: usize, limit: usize },
    SpanLimitExceeded { actual: usize, limit: usize },
    GlyphLimitExceeded { actual: usize, limit: usize },
    WrapLineLimitExceeded { actual: usize, limit: usize },
    VisualWidthExceeded { actual_64: u64, limit_64: u64 },
    ZeroWrapWidth,
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
            Self::MissingReplacementGlyph => {
                formatter.write_str("font has no replacement glyph for a missing codepoint")
            }
            Self::FixedPointOverflow => formatter.write_str("text fixed-point layout overflowed"),
            Self::CacheCounterOverflow => formatter.write_str("text cache counter overflowed"),
        }
    }
}

impl std::error::Error for TextError {}

pub fn parse_bedrock_text(text: &str, max_bytes: usize) -> Result<TextSpans, TextError> {
    parse_bedrock_text_with_style(text, max_bytes, TextStyle::default())
}

fn parse_bedrock_text_with_style(
    text: &str,
    max_bytes: usize,
    base_style: TextStyle,
) -> Result<TextSpans, TextError> {
    if text.len() > max_bytes {
        return Err(TextError::TextBytesExceeded {
            actual: text.len(),
            limit: max_bytes,
        });
    }

    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut style = base_style;
    let mut characters = NormalizedChars::new(text).peekable();
    while let Some(character) = characters.next() {
        if character != '§' {
            buffer.push(character);
            continue;
        }

        let Some(code) = characters.peek().copied() else {
            buffer.push(character);
            continue;
        };
        let Some(change) = formatting_change(code) else {
            buffer.push(character);
            buffer.push(code);
            characters.next();
            continue;
        };

        push_span(&mut spans, &mut buffer, style)?;
        characters.next();
        match change {
            FormattingChange::Color(color) => {
                style.color = color;
            }
            FormattingChange::Obfuscated => style.obfuscated = true,
            FormattingChange::Bold => style.bold = true,
            FormattingChange::Italic => style.italic = true,
            FormattingChange::Reset => style = base_style,
        }
    }
    push_span(&mut spans, &mut buffer, style)?;
    Ok(TextSpans(spans))
}

struct NormalizedChars<'a> {
    characters: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> NormalizedChars<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            characters: text.chars().peekable(),
        }
    }
}

impl Iterator for NormalizedChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let character = self.characters.next()?;
        if character == '\r' && self.characters.peek() == Some(&'\n') {
            self.characters.next();
            return Some('\n');
        }
        Some(character)
    }
}

fn push_span(
    spans: &mut Vec<TextSpan>,
    buffer: &mut String,
    style: TextStyle,
) -> Result<(), TextError> {
    if buffer.is_empty() {
        return Ok(());
    }
    if let Some(previous) = spans.last_mut().filter(|span| span.style == style) {
        let mut joined = String::with_capacity(previous.text.len() + buffer.len());
        joined.push_str(&previous.text);
        joined.push_str(buffer);
        previous.text = joined.into_boxed_str();
        buffer.clear();
        return Ok(());
    }
    let actual = spans
        .len()
        .checked_add(1)
        .ok_or(TextError::FixedPointOverflow)?;
    if actual > MAX_TEXT_SPANS {
        return Err(TextError::SpanLimitExceeded {
            actual,
            limit: MAX_TEXT_SPANS,
        });
    }
    spans.push(TextSpan {
        text: std::mem::take(buffer).into_boxed_str(),
        style,
    });
    Ok(())
}

#[derive(Clone, Copy)]
enum FormattingChange {
    Color(BedrockColor),
    Obfuscated,
    Bold,
    Italic,
    Reset,
}

fn formatting_change(code: char) -> Option<FormattingChange> {
    use BedrockColor as Color;
    use FormattingChange as Change;
    Some(match code.to_ascii_lowercase() {
        '0' => Change::Color(Color::Black),
        '1' => Change::Color(Color::DarkBlue),
        '2' => Change::Color(Color::DarkGreen),
        '3' => Change::Color(Color::DarkAqua),
        '4' => Change::Color(Color::DarkRed),
        '5' => Change::Color(Color::DarkPurple),
        '6' => Change::Color(Color::Gold),
        '7' => Change::Color(Color::Gray),
        '8' => Change::Color(Color::DarkGray),
        '9' => Change::Color(Color::Blue),
        'a' => Change::Color(Color::Green),
        'b' => Change::Color(Color::Aqua),
        'c' => Change::Color(Color::Red),
        'd' => Change::Color(Color::LightPurple),
        'e' => Change::Color(Color::Yellow),
        'f' => Change::Color(Color::White),
        'g' => Change::Color(Color::MinecoinGold),
        'h' => Change::Color(Color::MaterialQuartz),
        'i' => Change::Color(Color::MaterialIron),
        'j' => Change::Color(Color::MaterialNetherite),
        'm' => Change::Color(Color::MaterialRedstone),
        'n' => Change::Color(Color::MaterialCopper),
        'p' => Change::Color(Color::MaterialGold),
        'q' => Change::Color(Color::MaterialEmerald),
        's' => Change::Color(Color::MaterialDiamond),
        't' => Change::Color(Color::MaterialLapis),
        'u' => Change::Color(Color::MaterialAmethyst),
        'v' => Change::Color(Color::MaterialResin),
        'k' => Change::Obfuscated,
        'l' => Change::Bold,
        'o' => Change::Italic,
        'r' => Change::Reset,
        _ => return None,
    })
}

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
        scale_1024: (request.scale.get() * SCALE_DENOMINATOR as f32).round() as u16,
        font_identity: request.font.identity().carrier_sha256,
    }
}

fn build_layout(
    id: u64,
    key: TextLayoutKey,
    request: TextLayoutRequest<'_>,
) -> Result<TextLayout, TextError> {
    let spans = parse_bedrock_text_with_style(
        request.text,
        crate::UiLimits::MAX_TEXT_BYTES,
        request.style,
    )?;
    let glyph_count = spans
        .iter()
        .map(|span| {
            span.text
                .chars()
                .filter(|character| *character != '\n')
                .count()
        })
        .try_fold(0usize, |total, count| total.checked_add(count))
        .ok_or(TextError::FixedPointOverflow)?;
    if glyph_count > MAX_GLYPHS_PER_LAYOUT {
        return Err(TextError::GlyphLimitExceeded {
            actual: glyph_count,
            limit: MAX_GLYPHS_PER_LAYOUT,
        });
    }

    let scale_1024 = i64::from(key.scale_1024);
    let line_height_64 = font_line_height_64(request.font, scale_1024)?;
    let mut glyphs = Vec::with_capacity(glyph_count);
    let mut line = 0usize;
    let mut line_start = 0usize;
    let mut line_min_64 = 0i64;
    let mut line_max_64 = 0i64;
    let mut x_64 = 0i64;
    let mut maximum_width_64 = 0i64;

    for span in spans.iter() {
        for codepoint in span.text.chars() {
            if codepoint == '\n' {
                maximum_width_64 = maximum_width_64.max(finish_line(
                    &mut glyphs,
                    line_start,
                    line_min_64,
                    line_max_64,
                )?);
                line = next_line(line)?;
                line_start = glyphs.len();
                line_min_64 = 0;
                line_max_64 = 0;
                x_64 = 0;
                continue;
            }

            let (resolved_codepoint, metrics) = resolve_glyph(request.font, codepoint)?;
            let advance_64 = scale_metric(i64::from(metrics.advance_64), scale_1024)?;
            let mut candidate = line_candidate(
                metrics,
                x_64,
                line,
                line_height_64,
                scale_1024,
                line_min_64,
                line_max_64,
                advance_64,
            )?;
            if glyphs.len() > line_start && candidate.width_64 > u64::from(request.width_64) {
                maximum_width_64 = maximum_width_64.max(finish_line(
                    &mut glyphs,
                    line_start,
                    line_min_64,
                    line_max_64,
                )?);
                line = next_line(line)?;
                line_start = glyphs.len();
                line_min_64 = 0;
                line_max_64 = 0;
                x_64 = 0;
                candidate = line_candidate(
                    metrics,
                    x_64,
                    line,
                    line_height_64,
                    scale_1024,
                    line_min_64,
                    line_max_64,
                    advance_64,
                )?;
            }
            if candidate.width_64 > u64::from(request.width_64) {
                return Err(TextError::VisualWidthExceeded {
                    actual_64: candidate.width_64,
                    limit_64: u64::from(request.width_64),
                });
            }

            glyphs.push(GlyphQuad {
                codepoint,
                resolved_codepoint,
                page: metrics.page,
                uv: metrics.uv,
                bounds_64: candidate.bounds_64,
                line: u16::try_from(line).map_err(|_| TextError::FixedPointOverflow)?,
                style: span.style,
            });
            x_64 = candidate.pen_end_64;
            line_min_64 = candidate.min_64;
            line_max_64 = candidate.max_64;
        }
    }
    maximum_width_64 = maximum_width_64.max(finish_line(
        &mut glyphs,
        line_start,
        line_min_64,
        line_max_64,
    )?);
    let line_count = line.checked_add(1).ok_or(TextError::FixedPointOverflow)?;
    if line_count > MAX_WRAP_LINES {
        return Err(TextError::WrapLineLimitExceeded {
            actual: line_count,
            limit: MAX_WRAP_LINES,
        });
    }
    let nominal_height_64 = i64::try_from(line_count)
        .ok()
        .and_then(|count| count.checked_mul(line_height_64))
        .ok_or(TextError::FixedPointOverflow)?;
    let height_64 = normalize_vertical_bounds(&mut glyphs, nominal_height_64)?;

    Ok(TextLayout {
        id,
        key,
        glyphs: glyphs.into_boxed_slice(),
        line_count: u16::try_from(line_count).map_err(|_| TextError::FixedPointOverflow)?,
        size_64: [checked_u32(maximum_width_64)?, checked_u32(height_64)?],
    })
}

struct LineCandidate {
    bounds_64: [i32; 4],
    pen_end_64: i64,
    min_64: i64,
    max_64: i64,
    width_64: u64,
}

#[allow(clippy::too_many_arguments)]
fn line_candidate(
    metrics: GlyphMetrics,
    x_64: i64,
    line: usize,
    line_height_64: i64,
    scale_1024: i64,
    line_min_64: i64,
    line_max_64: i64,
    advance_64: i64,
) -> Result<LineCandidate, TextError> {
    let bounds_64 = glyph_bounds(metrics, x_64, line, line_height_64, scale_1024)?;
    let pen_end_64 = x_64
        .checked_add(advance_64)
        .ok_or(TextError::FixedPointOverflow)?;
    let min_64 = line_min_64.min(i64::from(bounds_64[0])).min(pen_end_64);
    let max_64 = line_max_64.max(i64::from(bounds_64[2])).max(pen_end_64);
    let width_64 = u64::try_from(
        max_64
            .checked_sub(min_64)
            .ok_or(TextError::FixedPointOverflow)?,
    )
    .map_err(|_| TextError::FixedPointOverflow)?;
    Ok(LineCandidate {
        bounds_64,
        pen_end_64,
        min_64,
        max_64,
        width_64,
    })
}

fn finish_line(
    glyphs: &mut [GlyphQuad],
    line_start: usize,
    min_64: i64,
    max_64: i64,
) -> Result<i64, TextError> {
    let shift_64 = min_64.checked_neg().ok_or(TextError::FixedPointOverflow)?;
    for glyph in glyphs
        .get_mut(line_start..)
        .ok_or(TextError::FixedPointOverflow)?
    {
        glyph.bounds_64[0] = checked_i32(
            i64::from(glyph.bounds_64[0])
                .checked_add(shift_64)
                .ok_or(TextError::FixedPointOverflow)?,
        )?;
        glyph.bounds_64[2] = checked_i32(
            i64::from(glyph.bounds_64[2])
                .checked_add(shift_64)
                .ok_or(TextError::FixedPointOverflow)?,
        )?;
    }
    max_64
        .checked_sub(min_64)
        .ok_or(TextError::FixedPointOverflow)
}

fn normalize_vertical_bounds(
    glyphs: &mut [GlyphQuad],
    nominal_height_64: i64,
) -> Result<i64, TextError> {
    let min_64 = glyphs
        .iter()
        .map(|glyph| i64::from(glyph.bounds_64[1]))
        .fold(0, i64::min);
    let max_64 = glyphs
        .iter()
        .map(|glyph| i64::from(glyph.bounds_64[3]))
        .fold(nominal_height_64, i64::max);
    let shift_64 = min_64.checked_neg().ok_or(TextError::FixedPointOverflow)?;
    for glyph in glyphs {
        glyph.bounds_64[1] = checked_i32(
            i64::from(glyph.bounds_64[1])
                .checked_add(shift_64)
                .ok_or(TextError::FixedPointOverflow)?,
        )?;
        glyph.bounds_64[3] = checked_i32(
            i64::from(glyph.bounds_64[3])
                .checked_add(shift_64)
                .ok_or(TextError::FixedPointOverflow)?,
        )?;
    }
    max_64
        .checked_sub(min_64)
        .ok_or(TextError::FixedPointOverflow)
}

fn resolve_glyph(
    font: &CompiledFontCatalog,
    codepoint: char,
) -> Result<(char, GlyphMetrics), TextError> {
    if let Some(metrics) = font.glyph(codepoint) {
        return Ok((codepoint, *metrics));
    }
    font.glyph(REPLACEMENT_CODEPOINT)
        .copied()
        .map(|metrics| (REPLACEMENT_CODEPOINT, metrics))
        .ok_or(TextError::MissingReplacementGlyph)
}

fn font_line_height_64(font: &CompiledFontCatalog, scale_1024: i64) -> Result<i64, TextError> {
    let pixels = font
        .glyphs()
        .iter()
        .map(|glyph| i64::from(glyph.uv[3].saturating_sub(glyph.uv[1])))
        .max()
        .unwrap_or(1)
        .max(1);
    scale_metric(
        pixels
            .checked_mul(FIXED_POINT_DENOMINATOR)
            .ok_or(TextError::FixedPointOverflow)?,
        scale_1024,
    )
}

fn glyph_bounds(
    metrics: GlyphMetrics,
    x_64: i64,
    line: usize,
    line_height_64: i64,
    scale_1024: i64,
) -> Result<[i32; 4], TextError> {
    let bearing_x_64 = scale_metric(
        i64::from(metrics.bearing[0])
            .checked_mul(FIXED_POINT_DENOMINATOR)
            .ok_or(TextError::FixedPointOverflow)?,
        scale_1024,
    )?;
    let bearing_y_64 = scale_metric(
        i64::from(metrics.bearing[1])
            .checked_mul(FIXED_POINT_DENOMINATOR)
            .ok_or(TextError::FixedPointOverflow)?,
        scale_1024,
    )?;
    let width_64 = scale_metric(
        i64::from(metrics.uv[2].saturating_sub(metrics.uv[0]))
            .checked_mul(FIXED_POINT_DENOMINATOR)
            .ok_or(TextError::FixedPointOverflow)?,
        scale_1024,
    )?;
    let height_64 = scale_metric(
        i64::from(metrics.uv[3].saturating_sub(metrics.uv[1]))
            .checked_mul(FIXED_POINT_DENOMINATOR)
            .ok_or(TextError::FixedPointOverflow)?,
        scale_1024,
    )?;
    let line_y_64 = i64::try_from(line)
        .ok()
        .and_then(|line| line.checked_mul(line_height_64))
        .ok_or(TextError::FixedPointOverflow)?;
    let left = x_64
        .checked_add(bearing_x_64)
        .ok_or(TextError::FixedPointOverflow)?;
    let top = line_y_64
        .checked_add(bearing_y_64)
        .ok_or(TextError::FixedPointOverflow)?;
    let right = left
        .checked_add(width_64)
        .ok_or(TextError::FixedPointOverflow)?;
    let bottom = top
        .checked_add(height_64)
        .ok_or(TextError::FixedPointOverflow)?;
    Ok([
        checked_i32(left)?,
        checked_i32(top)?,
        checked_i32(right)?,
        checked_i32(bottom)?,
    ])
}

fn scale_metric(value: i64, scale_1024: i64) -> Result<i64, TextError> {
    value
        .checked_mul(scale_1024)
        .and_then(|scaled| scaled.checked_div(SCALE_DENOMINATOR))
        .ok_or(TextError::FixedPointOverflow)
}

fn next_line(line: usize) -> Result<usize, TextError> {
    let next = line.checked_add(1).ok_or(TextError::FixedPointOverflow)?;
    let actual = next.checked_add(1).ok_or(TextError::FixedPointOverflow)?;
    if actual > MAX_WRAP_LINES {
        return Err(TextError::WrapLineLimitExceeded {
            actual,
            limit: MAX_WRAP_LINES,
        });
    }
    Ok(next)
}

fn checked_i32(value: i64) -> Result<i32, TextError> {
    i32::try_from(value).map_err(|_| TextError::FixedPointOverflow)
}

fn checked_u32(value: i64) -> Result<u32, TextError> {
    u32::try_from(value).map_err(|_| TextError::FixedPointOverflow)
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
